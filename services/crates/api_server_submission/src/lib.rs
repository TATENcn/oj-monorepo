use tokio::io;

#[derive(Debug, thiserror::Error)]
pub enum ApiServerSubmissionError {
    #[error(transparent)]
    Io(#[from] io::Error),
}
