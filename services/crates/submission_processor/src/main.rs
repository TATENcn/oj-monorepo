mod config;
mod error;
mod message;
mod rabbitmq;

use api_server_db::repositories::{connect_db, connect_repo, submissions::SubmissionRepo};
use config::ProcessorConfig;
use error::Error;
use futures_util::StreamExt;
use judge_core_sdk::JudgeCoreClient;
use judge_core_shared::models::http::VerdictResponse;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions},
    types::FieldTable,
};
use message::SubmitMessage;
use rabbitmq::RabbitMqTopology;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let config = ProcessorConfig::load()?;
    info!(?config, "configuration loaded");

    let topology = RabbitMqTopology::from(&config.rabbitmq);

    let conn = rabbitmq::init(&config.rabbitmq.url, &topology).await?;
    let channel = conn.create_channel().await?;
    let client = JudgeCoreClient::new(&config.judge_core.url, config.judge_core.standalone);

    let db = connect_db(&config.database.url).await?;
    let repo = connect_repo::<SubmissionRepo>(db);

    if !config.judge_core.standalone {
        client.acceptable().await?;
    }
    info!("judge-core available");

    let mut consumer = channel
        .basic_consume(
            topology.submit_queue.clone().into(),
            "submission-processor".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    info!("consuming from {}", topology.submit_queue);

    loop {
        tokio::select! {
            delivery = consumer.next() => {
                match delivery {
                    Some(Ok(delivery)) => {
                        let submission_id = process_message(
                            &client,
                            &repo,
                            &delivery.data,
                            &delivery.acker,
                        )
                        .await;

                        if let Some(id) = submission_id {
                            info!(submission_id = %id, "finished task");
                        }
                    }
                    Some(Err(e)) => {
                        error!(error = %e, "consumer error");
                    }
                    None => {
                        info!("consumer stream ended, channel or connection closed");
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    info!("shutting down");
    conn.close(200, "bye".into()).await?;
    Ok(())
}

/// Process a single message
///
/// # Returns
/// Some(submission_id) on success
/// None on failure (message not acked, it will be redelivered)
async fn process_message(client: &JudgeCoreClient, repo: &SubmissionRepo, body: &[u8], acker: &lapin::Acker) -> Option<String> {
    let submit_msg: SubmitMessage = match serde_json::from_slice(body) {
        Ok(m) => m,
        Err(e) => {
            error!(error = %e, "failed to deserialize message, discarding");
            acker.ack(BasicAckOptions::default()).await.ok();
            return None;
        }
    };

    let submission_id = submit_msg.submission_id;

    match client.task_submit(&submit_msg.task).await {
        Ok(task_result) => {
            let verdict = VerdictResponse::from(task_result);

            let Ok(submission_uuid) = uuid::Uuid::parse_str(&submission_id) else {
                error!(%submission_id, "invalid submission id UUID");
                acker.ack(BasicAckOptions::default()).await.ok();
                return None;
            };

            if let Err(e) = repo.mark_completed(submission_uuid, &verdict).await {
                error!(%submission_id, ?e, "failed to update submission in DB");
                return None;
            }

            acker.ack(BasicAckOptions::default()).await.ok();
            Some(submission_id)
        }
        Err(e) => {
            error!(submission_id = %submission_id, error = %e, "failed to process task");
            None
        }
    }
}
