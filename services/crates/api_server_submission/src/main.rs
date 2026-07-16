use std::{env, sync::Arc};

use api_server_db::repositories::{connect_db, connect_repo};
use api_server_submission::{
    ApiServerSubmissionError,
    router::{AppState, router},
};
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), ApiServerSubmissionError> {
    tracing_subscriber::fmt::init();

    let listener = TcpListener::bind("localhost:12547").await?;

    let db_connection = connect_db(env::var("DATABASE_URL").expect("database connection url")).await?;
    let state = AppState {
        repo: connect_repo(db_connection),
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
