use std::sync::Arc;

use api_server_db::repositories::{connect_db, connect_repo};
use api_server_submission::{
    ApiServerSubmissionError,
    config::SubmissionConfig,
    router::{AppState, router},
};
use lapin::{Connection, ConnectionProperties};
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), ApiServerSubmissionError> {
    tracing_subscriber::fmt::init();

    let config = SubmissionConfig::load()?;
    info!(?config, "configuration loaded");

    let listener = TcpListener::bind("localhost:12547").await?;

    let db_connection = connect_db(&config.database.url).await?;

    let conn = Connection::connect(&config.rabbitmq.url, ConnectionProperties::default()).await?;
    info!("connected to RabbitMQ");
    let channel = conn.create_channel().await?;

    let state = AppState {
        repo: connect_repo(db_connection),
        rabbitmq_channel: channel,
        rabbitmq_config: config.rabbitmq,
    };
    let router = router(Arc::new(state));

    info!("HTTP server listening on {}", "localhost:12547");
    info!("submission api server ready");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
            info!("shutdown signal received, stopping HTTP server");
        })
        .await?;

    Ok(())
}
