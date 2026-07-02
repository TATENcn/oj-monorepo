use std::{sync::Arc, time::Duration};

use gateway::{config::GatewayConfig, jwks::JwksManager, rate_limiter::memory::InMemoryRateLimiter, service::ProxyService};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), GatewayError> {
    tracing_subscriber::fmt::init();

    let config = GatewayConfig::load()?;
    let listener = TcpListener::bind(&config.addr).await?;
    let jwks = JwksManager::new(config.jwks_url.clone(), Duration::from_secs(60)).await?;
    jwks.start_background_refresh();

    // REVIEW: Make more choices, but in-memory now
    let rate_limiter = Arc::new(InMemoryRateLimiter::new(Duration::from_secs(300)));

    let service = Arc::new(ProxyService::new(
        config.routes,
        Duration::from_secs(config.upstream_timeout_secs),
        jwks,
        rate_limiter,
    ));

    info!(addr = %config.addr, "gateway listening");

    let mut handles = JoinSet::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, remote) = result?;
                info!(?remote, "accepted connection");
                handles.spawn(handle_connection(stream, service.clone()));
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
    while handles.join_next().await.is_some() {}

    info!("shutdown complete");
    Ok(())
}

async fn handle_connection(stream: TcpStream, service: Arc<ProxyService>) {
    if let Err(err) = AutoBuilder::new(TokioExecutor::new()).serve_connection(TokioIo::new(stream), service).await {
        error!(?err, "connection error")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error(transparent)]
    Config(#[from] ::config::ConfigError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("JWKS error: {0}")]
    Jwks(#[from] gateway::jwks::JwksError),
}
