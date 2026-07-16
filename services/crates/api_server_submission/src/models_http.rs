use api_server_db::models::enums::AcceptableLanguage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSubmissionRequest {
    pub source_code: String,
    pub problem_id: Uuid,
    pub language: AcceptableLanguage,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSubmissionResponse {
    pub id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSubmissionQueries {
    pub id: Uuid,
}

pub use api_server_db::models::http::SubmissionResult as GetSubmissionResponse;
