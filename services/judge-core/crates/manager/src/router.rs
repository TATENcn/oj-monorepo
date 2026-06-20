use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;
use tracing::error;

use crate::pool::{AgentPool, PoolError};
use shared::models::http::{
    AcceptablezResponse, ERR_AGENT_BUSY, ERR_AGENT_UNAVAILABLE, ERR_CONNECTION_FAILED, ERR_MAX_RETRIES_EXCEEDED, ERR_PROTOCOL_ERROR, ERR_PROVISION_ERROR,
    ERR_QUEUE_FULL, ERR_SHUTTING_DOWN, ERR_TASK_TIMEOUT, ErrorBody, ErrorResponse, PoolMetrics, SuccessResponse, VerdictResponse,
};
use shared::models::{
    VerdictTask,
    http::{ACCEPTABLE_URL, METRICS_URL, TASK_URL},
};

pub fn create_router(pool: Arc<AgentPool>) -> Router {
    Router::new()
        .route(METRICS_URL, get(metricsz_handler))
        .route(ACCEPTABLE_URL, get(acceptablez_handler))
        .route(TASK_URL, post(task_handler))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(pool)
}

async fn metricsz_handler(State(pool): State<Arc<AgentPool>>) -> Result<Json<SuccessResponse<PoolMetrics>>, AppError> {
    let metrics = pool.metrics().await;
    Ok(Json(SuccessResponse {
        data: metrics,
        message: "metrics retrieved".to_string(),
    }))
}

async fn acceptablez_handler(State(pool): State<Arc<AgentPool>>) -> Result<Json<SuccessResponse<AcceptablezResponse>>, AppError> {
    let metrics = pool.metrics().await;
    let acceptable = metrics.queue_size < 1000 && metrics.healthy_agent_count > 0;
    Ok(Json(SuccessResponse {
        data: AcceptablezResponse { acceptable, metrics },
        message: "acceptable status retrieved".to_string(),
    }))
}

async fn task_handler(State(pool): State<Arc<AgentPool>>, Json(task): Json<VerdictTask>) -> Result<Json<SuccessResponse<VerdictResponse>>, AppError> {
    let result = pool.submit(task).await?;
    Ok(Json(SuccessResponse {
        data: result.into(),
        message: "task completed".to_string(),
    }))
}

struct AppError(PoolError);

impl From<PoolError> for AppError {
    fn from(err: PoolError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self.0 {
            PoolError::QueueFull => (StatusCode::SERVICE_UNAVAILABLE, ERR_QUEUE_FULL),
            PoolError::MaxRetriesExceeded { .. } => (StatusCode::SERVICE_UNAVAILABLE, ERR_MAX_RETRIES_EXCEEDED),
            PoolError::AgentUnavailable => (StatusCode::SERVICE_UNAVAILABLE, ERR_AGENT_UNAVAILABLE),
            PoolError::ShuttingDown => (StatusCode::SERVICE_UNAVAILABLE, ERR_SHUTTING_DOWN),
            PoolError::TaskTimeout(_) => (StatusCode::GATEWAY_TIMEOUT, ERR_TASK_TIMEOUT),
            PoolError::ConnectionFailed(_) => (StatusCode::INTERNAL_SERVER_ERROR, ERR_CONNECTION_FAILED),
            PoolError::Protocol(_) => (StatusCode::INTERNAL_SERVER_ERROR, ERR_PROTOCOL_ERROR),
            PoolError::Provision(_) => (StatusCode::INTERNAL_SERVER_ERROR, ERR_PROVISION_ERROR),
            PoolError::AgentBusy { .. } => (StatusCode::INTERNAL_SERVER_ERROR, ERR_AGENT_BUSY),
        };
        let message = self.0.to_string();

        error!(error = %self.0, status = %status, "request failed");

        let body = Json(ErrorResponse {
            error: ErrorBody { code: code.to_string() },
            message,
        });

        (status, body).into_response()
    }
}
