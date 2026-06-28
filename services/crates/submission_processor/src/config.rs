use config::{Config, ConfigError, Environment, File};
use config_macro::config;
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "./config/processor.toml";
const CONFIG_PATH_ENV: &str = "SP_CONFIG_PATH";
const ENV_PREFIX: &str = "SP";

#[config]
#[derive(Debug, Deserialize)]
pub struct JudgeCoreConfig {
    #[config_val(default = "http://localhost:8000".into())]
    pub url: String,
    #[serde(default)]
    pub standalone: bool,
}

#[config]
#[derive(Debug, Deserialize)]
pub struct RabbitMqConfig {
    #[serde(default)]
    pub url: String,
    #[config_val(default = "online-judge.exchange".into())]
    pub exchange_name: String,
    #[config_val(default = "submit.queue".into())]
    pub submit_queue: String,
    #[config_val(default = "submit".into())]
    pub submit_route: String,
    #[config_val(default = "result.queue".into())]
    pub result_queue: String,
    #[config_val(default = "result".into())]
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
