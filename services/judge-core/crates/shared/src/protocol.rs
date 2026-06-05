use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

pub type FrameId = u64;

/// A sendable frame
#[derive(Debug, Deserialize, Serialize)]
struct Frame<T> {
    id: FrameId,
    inner: T,
}

pub const HEARTBEAT_MAGIC: u32 = 0;

pub async fn receive<T: DeserializeOwned, S: AsyncReadExt + AsyncWriteExt + Unpin>(stream: &mut S) -> Result<Option<(FrameId, T)>, ProtocolError> {
    let len = stream.read_u32().await?;

    if len == HEARTBEAT_MAGIC {
        stream.write_u32(HEARTBEAT_MAGIC).await?;
        return Ok(None);
    }

    let mut buf = vec![0; len as usize];
    stream.read_exact(&mut buf).await?;

    let data: Frame<T> = postcard::from_bytes(&buf)?;
    Ok(Some((data.id, data.inner)))
}

/// Send a heartbeat and expect a heartbeat response
pub async fn send_heartbeat<S: AsyncReadExt + AsyncWriteExt + Unpin>(stream: &mut S) -> Result<(), ProtocolError> {
    stream.write_u32(HEARTBEAT_MAGIC).await?;
    let resp = stream.read_u32().await?;
    if resp != HEARTBEAT_MAGIC {
        return Err(ProtocolError::InvalidHeartbeatResponse);
    }
    Ok(())
}

/// Send data
pub async fn send<T: Serialize, S: AsyncWriteExt + Unpin>(stream: &mut S, id: FrameId, data: T) -> Result<(), ProtocolError> {
    let frame = Frame { id, inner: data };
    let data = postcard::to_allocvec(&frame)?;
    stream.write_u32(data.len().try_into().expect("Cannot transform frame")).await?;
    stream.write_all(&data).await?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Serialization(#[from] postcard::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("invalid heartbeat response")]
    InvalidHeartbeatResponse,
    #[error("received unexpected heartbeat frame")]
    UnexpectedHeartbeat,
}
