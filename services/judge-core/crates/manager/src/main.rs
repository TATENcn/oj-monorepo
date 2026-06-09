use std::{sync::Arc, time::Duration};

use containerd_client::{
    connect,
    services::v1::version_client,
    tonic::transport::{self, Channel},
};
use tracing::{error, info};

use manager::{
    pool::{AgentPool, PoolConfig, PoolError},
    provisioner::{ContainerdProvisioner, ProvisionError},
    router,
    scaler::ScalerConfig,
};

#[tokio::main]
async fn main() -> Result<(), ManagerError> {
    tracing_subscriber::fmt::init();

    let connection = connect("/run/containerd/containerd.sock").await?;

    info!(version = containerd_version(connection.clone()).await?, "containerd connected");

    let provisioner = ContainerdProvisioner::new(connection, "docker.io/library/judge-core:latest");

    let pool = Arc::new(
        AgentPool::new(
            PoolConfig {
                max_queue_size: 1000,
                max_retries: 3,
                task_timeout: Duration::from_secs(45),
                health_check_interval: Duration::from_secs(5),
                health_check_failure_threshold: 3,
                max_concurrent_per_agent: 5,
            },
            provisioner,
        )
        .await,
    );

    pool.discover_agents().await?;
    info!(metrics = ?pool.metrics().await, "agents discovered");

    // WARNING: a huge number of agents may lead to OOM or excessive CPU usage
    pool.clone().start_autoscaler(ScalerConfig {
        min_agents: 2,
        max_agents: 5,
        scale_down_utilization_pct: 0.3,
        scale_up_cooldown_secs: 5,
        scale_down_cooldown_secs: 300,
        check_interval_secs: 10,
        provision_time_secs: 30,
        max_scale_up_batch: 3,
        scale_down_confirm_ticks: 3,
        ema_alpha: 0.3,
        max_concurrent_per_agent: 5,
    });

    let app = router::create_router(pool.clone());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    info!("HTTP server listening on 0.0.0.0:8000");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "HTTP server error");
        }
    });

    info!("manager ready");

    tokio::signal::ctrl_c().await?;
    info!("shutting down");

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
}

impl From<containerd_client::tonic::Status> for ManagerError {
    fn from(value: containerd_client::tonic::Status) -> Self {
        let code = value.code();
        let message = value.message();

        Self::Rpc(code, message.to_owned())
    }
}
