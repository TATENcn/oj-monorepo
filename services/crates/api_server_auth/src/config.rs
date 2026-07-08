use confide::confide;
use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};

#[confide]
#[derive(Deserialize, Serialize)]
pub struct ApiServerConfig {
    #[confide(default)]
    pub database: DatabaseConfig,
    #[confide(default)]
    pub auth: AuthConfig,
}

#[confide]
#[derive(Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[confide(default = "postgresql://postgres:postgres@localhost:5432/taten".to_string())]
    pub database_url: String,
}

#[confide]
#[derive(Deserialize, Serialize)]
pub struct AuthConfig {
    #[confide(default)]
    pub public_pem_filepath: String,
    #[confide(default)]
    pub private_pem_filepath: String,
    #[confide(default = 60 * 60 * 24)]
    pub access_token_ttl_secs: u64,
    #[confide(default = 60 * 60 * 24 * 7)]
    pub refresh_token_ttl_secs: u64,
}

const DEFAULT_CONFIG_PATH: &str = "./config/api_server_auth.toml";
const CONFIG_PATH_ENV: &str = "API_SERVER_AUTH_CONFIG_PATH";
const ENV_PREFIX: &str = "API_SERVER_AUTH";

impl ApiServerConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
