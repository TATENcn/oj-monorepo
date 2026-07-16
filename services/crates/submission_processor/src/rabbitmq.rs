use lapin::{
    Connection, ConnectionProperties, ExchangeKind,
    options::{ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
};
use tracing::info;

use crate::config::RabbitMqConfig;
use crate::error::Error;

pub struct RabbitMqTopology {
    pub exchange_name: String,
    pub submit_queue: String,
    pub submit_route: String,
}

impl From<&RabbitMqConfig> for RabbitMqTopology {
    fn from(config: &RabbitMqConfig) -> Self {
        Self {
            exchange_name: config.exchange_name.clone(),
            submit_queue: config.submit_queue.clone(),
            submit_route: config.submit_route.clone(),
        }
    }
}

/// Initialize RabbitMQ topology
pub async fn init(url: &str, topology: &RabbitMqTopology) -> Result<Connection, Error> {
    let conn = Connection::connect(url, ConnectionProperties::default()).await?;
    info!("connected to RabbitMQ");

    let channel = conn.create_channel().await?;

    channel
        .exchange_declare(
            topology.exchange_name.clone().into(),
            ExchangeKind::Direct,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_declare(
            topology.submit_queue.clone().into(),
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_bind(
            topology.submit_queue.clone().into(),
            topology.exchange_name.clone().into(),
            topology.submit_route.clone().into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    channel.basic_qos(1, Default::default()).await?;
    info!("RabbitMQ topology configured");
    Ok(conn)
}
