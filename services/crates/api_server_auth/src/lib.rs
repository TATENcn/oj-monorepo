pub mod config;

use tokio::io;

#[derive(Debug, thiserror::Error)]
pub enum AuthApiServerError {
    #[error(transparent)]
    Io(#[from] io::Error),
}
