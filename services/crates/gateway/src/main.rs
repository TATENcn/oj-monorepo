use bytes::Bytes;
use gateway::config::GatewayConfig;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use tokio::io;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing::{error, info};

async fn handle_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::http::Error> {
    info!(method = ?req.method(), uri = ?req.uri(), version = ?req.version(), "received request");

    let body = format!("Hello world!\n");

    Response::builder()
        .status(200)
        .header("Content-Type", "text/plain")
        .body(Full::new(Bytes::from(body)))
}

#[tokio::main]
async fn main() -> Result<(), GatewayError> {
    tracing_subscriber::fmt::init();

    let config = GatewayConfig::load()?;
    let listener = TcpListener::bind(config.addr).await?;

    let mut handles = JoinSet::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, addr) = result?;
                info!(?addr, "accepted connection");

                handles.spawn(async move {
                    if let Err(err) = AutoBuilder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(stream), hyper::service::service_fn(handle_request))
                        .await
                    {
                        error!(?err, "unexpected error occurred")
                    }
                });
            },
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT");
                break;
            },
        }
    }

    // Release the port immediately
    drop(listener);

    // Wait for in-flight connections
    info!(pending = handles.len(), "waiting for in-flight connections to drain");
    while let Some(_) = handles.join_next().await {}

    info!("shutdown complete");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error(transparent)]
    Config(#[from] ::config::ConfigError),
    #[error(transparent)]
    Io(#[from] io::Error),
}
