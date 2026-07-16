use ::config::{Config, ConfigError, Environment, File};
use confide::confide;
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "./config/submission.toml";
const CONFIG_PATH_ENV: &str = "AS_CONFIG_PATH";
const ENV_PREFIX: &str = "AS";

#[confide]
#[derive(Deserialize)]
pub struct DatabaseConfig {
    #[confide(default)]
    pub url: String,
}

#[confide]
#[derive(Deserialize)]
pub struct RabbitMqConfig {
    #[confide(default)]
    pub url: String,
    #[confide(default = "online-judge.exchange".to_string())]
    pub exchange: String,
    #[confide(default = "submit".to_string())]
    pub submit_routing_key: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmissionConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub rabbitmq: RabbitMqConfig,
}

impl SubmissionConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
