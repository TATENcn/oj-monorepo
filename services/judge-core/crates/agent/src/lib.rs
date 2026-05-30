use shared::protocol::ProtocolError;
use tokio::io;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
