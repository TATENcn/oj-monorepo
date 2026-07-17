use std::sync::Arc;

use api_server_auth::{AuthApiServerError, config::ApiServerConfig};
use auth::router::{AppState, jwks_router, router};
use sea_orm::Database;
use tokio::{fs, net::TcpListener};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), AuthApiServerError> {
    tracing_subscriber::fmt::init();

    let config = ApiServerConfig::load()?;
    info!(?config, "configuration loaded");
    let connection = Database::connect(&config.database.database_url).await?;

    let public_pem_file = fs::read_to_string(&config.auth.public_pem_filepath).await?;
    let private_pem_file = fs::read_to_string(&config.auth.private_pem_filepath).await?;
    info!("pem files loaded");

    let auth_state = AppState {
        db: connection.clone(),
        private_key_pem: private_pem_file.into(),
        public_key_pem: public_pem_file.into(),
        access_token_ttl_secs: config.auth.access_token_ttl_secs,
        refresh_token_ttl_secs: config.auth.refresh_token_ttl_secs,
    };
    let auth_state = Arc::new(auth_state);

    let router = router(auth_state.clone()).nest("/.well-known", jwks_router(auth_state));
    info!("router initialized");

    let listener = TcpListener::bind("localhost:9001").await?;
    info!("service available");

    service_utils::serve(listener, router).await?;

    Ok(())
}
