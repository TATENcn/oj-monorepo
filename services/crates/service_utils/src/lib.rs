use std::io;

use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

/// Bind `listener` and serve `router` with graceful shutdown on `SIGINT`
pub async fn serve(listener: TcpListener, router: Router) -> io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
            info!("shutdown signal received, stopping HTTP server");
        })
        .await
}
