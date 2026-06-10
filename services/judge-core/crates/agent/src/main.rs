use agent::{AgentError, verdict::handle};
use shared::{
    models::VerdictTask,
    protocol::{receive, send},
};
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tokio::time::{self, Duration};
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    tracing_subscriber::fmt::init();

    let listener = UnixListener::bind("/run/judge-core/agent.sock")?;
    let mut tasks = JoinSet::new();

    loop {
        let (mut stream, _addr) = tokio::select! {
            result = listener.accept() => result?,
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received, draining tasks");
                break;
            }
        };

        tasks.spawn(async move {
            let result = async {
                let (id, task) = match receive::<VerdictTask, _>(&mut stream).await? {
                    Some(pair) => pair,
                    None => {
                        debug!("heartbeat received, closing connection");
                        return Ok::<(), AgentError>(());
                    }
                };

                info!(task_id = id, language = ?task.language, "starting verdict");

                let res = match task.language {
                    shared::models::Language::Cpp => handle::<agent::verdict::cpp::Cpp>(id, task).await,
                };

                info!(task_id = id, result = ?res, "verdict completed");

                send(&mut stream, id, res).await?;

                debug!(task_id = id, "response sent");
                Ok(())
            }
            .await;

            if let Err(e) = result {
                error!(error = %e, "connection handler error");
            }
        });
    }

    let drain_result = time::timeout(Duration::from_secs(60), async {
        while let Some(result) = tasks.join_next().await {
            if let Err(e) = result {
                error!(error = %e, "task panicked during drain");
            }
        }
    })
    .await;

    if drain_result.is_err() {
        let remaining = tasks.len();
        warn!(remaining, "drain timeout reached, aborting remaining tasks");
        tasks.shutdown().await;
    }

    info!("agent shut down");
    Ok(())
}
