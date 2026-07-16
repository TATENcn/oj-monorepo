use std::sync::Arc;

use api_server_db::repositories::submissions::SubmissionRepo;
use auth::extractor::{Identity, UserId};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};
use lapin::{BasicProperties, Channel, options::BasicPublishOptions};
use tracing::instrument;

use crate::config::RabbitMqConfig;
use crate::message::SubmitMessage;
use crate::models_http::{GetSubmissionQueries, GetSubmissionResponse, PostSubmissionRequest, PostSubmissionResponse};

pub struct AppState {
    pub repo: SubmissionRepo,
    pub rabbitmq_channel: Channel,
    pub rabbitmq_config: RabbitMqConfig,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v2/submissions", post(post_handler))
        .route("/api/v2/submissions", get(get_handler))
        .with_state(state)
}

#[instrument(skip(state), fields(user_id = %id.user_id, problem_id = %req.problem_id))]
async fn post_handler(
    State(state): State<Arc<AppState>>,
    UserId(id): UserId<Identity>,
    Json(req): Json<PostSubmissionRequest>,
) -> Result<Json<PostSubmissionResponse>, StatusCode> {
    let (submission_id, verdict_task) = state
        .repo
        .create_pending(req.problem_id, id.user_id, req.source_code, req.language)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to create pending submission");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let msg = SubmitMessage {
        submission_id,
        task: verdict_task,
    };
    let payload = serde_json::to_vec(&msg).map_err(|e| {
        tracing::error!(?e, "failed to serialize submit message");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    state
        .rabbitmq_channel
        .basic_publish(
            state.rabbitmq_config.exchange.as_str().into(),
            state.rabbitmq_config.submit_routing_key.as_str().into(),
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to publish submit message");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    tracing::info!(%submission_id, "submission created and queued");
    Ok(Json(PostSubmissionResponse { id: submission_id }))
}

#[instrument(skip(state), fields(user_id = %id.user_id, submission_id = %queries.id))]
async fn get_handler(
    State(state): State<Arc<AppState>>,
    Query(queries): Query<GetSubmissionQueries>,
    UserId(id): UserId<Identity>,
) -> Result<Json<GetSubmissionResponse>, StatusCode> {
    let result = state.repo.get(id.user_id, queries.id).await.map_err(|e| {
        tracing::error!(?e, "failed to get submission");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match result {
        Some(result) => Ok(Json(result)),
        None => {
            tracing::warn!("submission not found");
            Err(StatusCode::NOT_FOUND)
        }
    }
}
