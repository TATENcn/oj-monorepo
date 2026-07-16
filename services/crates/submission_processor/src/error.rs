use api_server_db::{repositories::RepoError, DbErr};
use judge_core_sdk::JudgeCoreError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("AMQP error: {0}")]
    Amqp(#[from] lapin::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("DB error: {0}")]
    Db(#[from] DbErr),
    #[error("DB repo error: {0}")]
    Repo(#[from] RepoError),
    #[error("judge-core error: {0}")]
    JudgeCore(#[from] JudgeCoreError),
    #[error("config error: {0}")]
    Config(#[from] config::ConfigError),
}
