use auth::router::AppState as AuthAppState;
use config::{Config, ConfigError, Environment, File};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tokio::{fs, io};

#[config_macro::config]
#[derive(Debug, Deserialize, Serialize)]
pub struct ApiServerConfig {
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
}

#[config_macro::config]
#[derive(Debug, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[config_val(default = "postgresql://postgres:postgres@localhost:5432/taten".into())]
    pub database_url: String,
}

#[config_macro::config]
#[derive(Debug, Deserialize, Serialize)]
pub struct AuthConfig {
    /// Private key pen filepath
    pub private_key_pem_filepath: String,
    /// Public key pen filepath
    pub public_key_pem_filepath: String,
    /// Access token lifetime in seconds
    #[config_val(default = 900)]
    pub access_token_ttl_secs: u64,
    /// Refresh token lifetime in seconds
    #[config_val(default = 7 * 24 * 60 * 60)]
    pub refresh_token_ttl_secs: u64,
}

impl AuthConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.private_key_pem_filepath.is_empty() {
            return Err("private key pem filepath cannot be empty");
        }
        if self.public_key_pem_filepath.is_empty() {
            return Err("private key pem filepath cannot be empty");
        }
        if self.access_token_ttl_secs == 0 {
            return Err("access token lifetime cannot be 0");
        }
        if self.refresh_token_ttl_secs == 0 {
            return Err("refresh token lifetime cannot be 0");
        }

        Ok(())
    }
}

const DEFAULT_CONFIG_PATH: &str = "./config/api_server.toml";
const CONFIG_PATH_ENV: &str = "API_SERVER_CONFIG_PATH";
const ENV_PREFIX: &str = "API_SERVER";

impl ApiServerConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }

    pub async fn to_auth_app_state(&self, db: DatabaseConnection) -> Result<AuthAppState, io::Error> {
        let private = fs::read(&self.auth.private_key_pem_filepath).await?;
        let public = fs::read(&self.auth.public_key_pem_filepath).await?;

        Ok(AuthAppState {
            db,
            private_key_pem: private,
            public_key_pem: public,
            access_token_ttl_secs: self.auth.access_token_ttl_secs,
            refresh_token_ttl_secs: self.auth.refresh_token_ttl_secs,
        })
    }
}
