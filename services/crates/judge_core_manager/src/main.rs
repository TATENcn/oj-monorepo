use std::{sync::Arc, time::Duration};

use containerd_client::{
    connect,
    services::v1::version_client,
    tonic::transport::{self, Channel},
};
use tracing::info;

use judge_core_manager::{
    config::ManagerConfig,
    pool::{AgentPool, PoolError},
    provisioner::{ContainerdProvisioner, ProvisionError},
    router,
};

#[tokio::main]
async fn main() -> Result<(), ManagerError> {
    tracing_subscriber::fmt::init();

    let config = ManagerConfig::load()?;
    info!(?config, "configuration loaded");

    let connection = connect(&config.server.containerd_socket).await?;

    info!(version = containerd_version(connection.clone()).await?, "containerd connected");

    let provisioner = ContainerdProvisioner::new(connection, &config.server.image, &config.provisioner.namespace, &config.provisioner.runtime);
    let pool = Arc::new(AgentPool::new(config.pool.clone(), provisioner).await);

    pool.discover_agents().await?;
    info!(metrics = ?pool.metrics().await, "agents discovered");

    // WARNING: a huge number of agents may lead to OOM or excessive CPU usage
    pool.clone().start_autoscaler(config.scaler.clone());

    let app = router::create_router(pool.clone());
    let listener = tokio::net::TcpListener::bind(&config.server.bind_address).await?;
    info!("HTTP server listening on {}", config.server.bind_address);
    info!("manager ready");

    service_utils::serve(listener, app).await?;

    info!("draining task pool");
    pool.shutdown(Duration::from_secs(60)).await?;

    info!("manager shut down");
    Ok(())
}

async fn containerd_version(connection: Channel) -> Result<String, ManagerError> {
    let mut version_client = version_client::VersionClient::new(connection);
    let version = version_client.version(()).await?.into_inner().version;

    Ok(version)
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error(transparent)]
    Connect(#[from] transport::Error),
    #[error("Rpc error: {0}")]
    Rpc(containerd_client::tonic::Code, String),
    #[error(transparent)]
    Pool(#[from] PoolError),
    #[error(transparent)]
    Provision(#[from] ProvisionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(#[from] config::ConfigError),
}

impl From<containerd_client::tonic::Status> for ManagerError {
    fn from(value: containerd_client::tonic::Status) -> Self {
        let code = value.code();
        let message = value.message();

        Self::Rpc(code, message.to_owned())
    }
}
