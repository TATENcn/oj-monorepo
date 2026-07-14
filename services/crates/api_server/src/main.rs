use std::sync::Arc;

use api_server::{
    ApiServerError,
    config::ApiServerConfig,
    routers::{ApiServerState, problems},
};
use api_server_db::repositories::{connect_db, connect_repo};
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), ApiServerError> {
    tracing_subscriber::fmt::init();

    let config = ApiServerConfig::load()?;
    info!(?config, "configuration loaded");
    let connection = connect_db(&config.database.database_url).await?;

    let listener = TcpListener::bind("localhost:3080").await?;
    let api_state = Arc::new(ApiServerState {
        problems_repo: connect_repo(connection.clone()),
        test_cases_repo: connect_repo(connection.clone()),
        db: connection.clone(),
    });
    let router = Router::new().nest("/api/v1/problems", problems::router().with_state(api_state));

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("failed to listen for `ctrl_c`");
            info!("shutdown signal received, stopping HTTP server");
        })
        .await?;

    Ok(())
}
