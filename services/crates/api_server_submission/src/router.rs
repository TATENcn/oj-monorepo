use std::sync::Arc;

use api_server_db::repositories::submissions::SubmissionRepo;
use auth::extractor::{Identity, UserId};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};

use tracing::instrument;

use crate::models_http::{GetSubmissionQueries, GetSubmissionResponse, PostSubmissionRequest, PostSubmissionResponse};

pub struct AppState {
    pub repo: SubmissionRepo,
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
    let result = state
        .repo
        .create_pending(req.problem_id, id.user_id, req.source_code, req.language.into())
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to create pending submission");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    tracing::info!(submission_id = %result.0, "submission created");
    Ok(Json(PostSubmissionResponse { id: result.0 }))
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
