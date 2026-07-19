use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use gateway::{
    HTTP_CLIENT,
    config::GatewayConfig,
    jwks::JwksManager,
    rate_limiter::memory::InMemoryRateLimiter,
    service::{GatewayService, HttpBody, ProxyService},
};
use http_body_util::Full;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode, body::Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), MainError> {
    tracing_subscriber::fmt::init();

    let config = GatewayConfig::load()?;
    let listener = TcpListener::bind(&config.addr).await?;
    let mut jwks = JwksManager::new(config.jwks_url.clone(), Duration::from_secs(60)).await?;
    jwks.start_background_refresh();

    // REVIEW: Make more choices, but in-memory now
    let rate_limiter = Arc::new(InMemoryRateLimiter::new(Duration::from_secs(300)));
    let connection_semaphore = Arc::new(Semaphore::new(config.max_connections));

    let proxy = ProxyService::new(HTTP_CLIENT.clone(), Duration::from_secs(config.upstream_timeout_secs));
    let gateway = GatewayService::new(proxy, config.routes, jwks, rate_limiter);
    let service = Arc::new(gateway);

    info!(addr = %config.addr, max_connections = config.max_connections, "gateway listening");

    let mut handles = JoinSet::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, remote) = result?;
                match connection_semaphore.clone().try_acquire_owned() {
                    Ok(_permit) => {
                        info!(?remote, "accepted connection");
                        let svc = service.clone();
                        handles.spawn(async move {
                            handle_connection(stream, svc).await;
                        });
                    }
                    Err(_) => {
                        warn!(?remote, "connection rejected: max connections reached");
                        handles.spawn(send_503(stream));
                    }
                }
            },
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT");
                break;
            },
        }
    }

    // Release the port immediately
    drop(listener);

    // Wait for in-flight connections with a timeout
    let drain_timeout = Duration::from_secs(config.drain_timeout_secs);
    info!(
        pending = handles.len(),
        timeout_secs = config.drain_timeout_secs,
        "waiting for in-flight connections to drain"
    );
    let drain_result = tokio::time::timeout(drain_timeout, async { while handles.join_next().await.is_some() {} }).await;

    match drain_result {
        Ok(()) => info!("all connections drained, shutdown complete"),
        Err(_) => {
            let remaining = handles.len();
            error!(remaining, "drain timed out, forcing shutdown");
        }
    }
    Ok(())
}

/// Thin wrapper that injects the TCP peer address into request extensions before delegating to the inner service
struct ConnectionService<S> {
    inner: Arc<S>,
    peer_addr: SocketAddr,
}

impl<S> Service<Request<Incoming>> for ConnectionService<S>
where
    S: Service<Request<Incoming>, Response = Response<HttpBody>, Error = std::convert::Infallible> + Send + Sync + 'static,
    S::Future: Send,
{
    type Response = Response<HttpBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, mut req: Request<Incoming>) -> Self::Future {
        req.extensions_mut().insert(self.peer_addr);
        Box::pin(self.inner.call(req))
    }
}

async fn send_503(stream: TcpStream) {
    let svc = hyper::service::service_fn(|_req| async {
        Ok::<_, hyper::http::Error>(
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(http::header::CONTENT_TYPE, "text/plain")
                .header(http::header::CONNECTION, "close")
                .body(Full::new(Bytes::from_static(b"503 Service Unavailable\n")))
                .unwrap(),
        )
    });
    let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), svc).await;
}

async fn handle_connection<S>(stream: TcpStream, service: Arc<S>)
where
    S: Service<Request<Incoming>, Response = Response<HttpBody>, Error = std::convert::Infallible> + Send + Sync + 'static,
    S::Future: Send,
{
    let peer_addr = match stream.peer_addr() {
        Ok(addr) => addr,
        Err(e) => {
            error!(?e, "failed to get peer address, dropping connection");
            return;
        }
    };

    let svc = ConnectionService { inner: service, peer_addr };

    if let Err(err) = AutoBuilder::new(TokioExecutor::new()).serve_connection(TokioIo::new(stream), svc).await {
        error!(?err, "connection error")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MainError {
    #[error(transparent)]
    Config(#[from] gateway::config::GatewayConfigError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("JWKS error: {0}")]
    Jwks(#[from] gateway::jwks::JwksError),
}
