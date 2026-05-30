use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub type FrameId = u64;

/// A sendable frame
#[derive(Debug, Deserialize, Serialize)]
struct Frame<T> {
    id: FrameId,
    inner: T,
}

/// Receive data
pub async fn receive<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<(FrameId, T), ProtocolError> {
    let len = stream.read_u32().await?;

    let mut buf = vec![0; len as usize];
    stream.read_exact(&mut buf).await?;

    let data: Frame<T> = postcard::from_bytes(&buf)?;
    Ok((data.id, data.inner))
}

/// Send data
pub async fn send<T: Serialize>(stream: &mut TcpStream, id: FrameId, data: T) -> Result<(), ProtocolError> {
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
}
