pub mod config;
pub mod message;
pub mod models_http;
pub mod router;

use ::config::ConfigError;
use api_server_db::DbErr;
use tokio::io;

#[derive(Debug, thiserror::Error)]
pub enum ApiServerSubmissionError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Db(#[from] DbErr),
    #[error(transparent)]
    RabbitMq(#[from] lapin::Error),
}
