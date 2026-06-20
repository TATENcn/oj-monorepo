use agent::verdict::{cpp::Cpp, handle};
use axum::{Json, Router, extract::DefaultBodyLimit, routing::post};
use shared::models::{
    Language, VerdictTask,
    http::{SuccessResponse, VerdictResponse},
};
use std::sync::atomic::{AtomicU64, Ordering};
use tower_http::trace::TraceLayer;

static TASK_ID: AtomicU64 = AtomicU64::new(0);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/task", post(task_handler))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    tracing::info!("standalone listening on 0.0.0.0:8000");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
        })
        .await
        .unwrap();
}

async fn task_handler(Json(task): Json<VerdictTask>) -> Json<SuccessResponse<VerdictResponse>> {
    let id = TASK_ID.fetch_add(1, Ordering::Relaxed);

    tracing::info!(task_id = id, language = ?task.language, "task received");

    let result = match task.language {
        Language::Cpp => handle::<Cpp>(id, task).await,
    };

    tracing::info!(task_id = id, result = ?result, "task completed");

    Json(SuccessResponse {
        data: result.into(),
        message: "task completed".to_string(),
    })
}
