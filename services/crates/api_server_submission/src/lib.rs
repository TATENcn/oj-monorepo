pub mod models_http;
pub mod router;

use api_server_db::DbErr;
use tokio::io;

#[derive(Debug, thiserror::Error)]
pub enum ApiServerSubmissionError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Db(#[from] DbErr),
}
