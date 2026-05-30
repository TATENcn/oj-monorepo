use agent::AgentError;
use shared::{
    models::VerdictTask,
    protocol::{FrameId, receive, send},
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let listener = TcpListener::bind("127.0.0.1:3000").await?;

    loop {
        let (mut stream, _addr) = tokio::select! {
            result = listener.accept() => result?,
            _ = tokio::signal::ctrl_c() => break,
        };

        let (id, task): (FrameId, VerdictTask) = receive(&mut stream).await?;

        todo!("complete agent functionality");

        send(&mut stream, id, todo!("task result")).await?;
    }

    Ok(())
}
