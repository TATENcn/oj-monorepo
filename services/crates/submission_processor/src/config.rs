use confide::confide;
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "./config/processor.toml";
const CONFIG_PATH_ENV: &str = "SP_CONFIG_PATH";
const ENV_PREFIX: &str = "SP";

#[confide]
#[derive(Deserialize)]
pub struct JudgeCoreConfig {
    #[confide(default = "http://localhost:8000".to_string())]
    pub url: String,
    #[confide(default)]
    pub standalone: bool,
}

#[confide]
#[derive(Deserialize)]
pub struct RabbitMqConfig {
    #[confide(default)]
    pub url: String,
    #[confide(default = "online-judge.exchange".to_string())]
    pub exchange_name: String,
    #[confide(default = "submit.queue".to_string())]
    pub submit_queue: String,
    #[confide(default = "submit".to_string())]
    pub submit_route: String,
    #[confide(default = "result.queue".to_string())]
    pub result_queue: String,
    #[confide(default = "result".to_string())]
    pub result_route: String,
}

#[derive(Debug, Deserialize)]
pub struct ProcessorConfig {
    #[serde(default)]
    pub judge_core: JudgeCoreConfig,
    #[serde(default)]
    pub rabbitmq: RabbitMqConfig,
}

impl ProcessorConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
