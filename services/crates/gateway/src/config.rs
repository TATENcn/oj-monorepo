use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationLevel {
    /// Pass if no token is provided (incorrect tokens will be rejected)
    Optional,
    /// Pass if valid token is provided
    Required,
    /// Authentication not required
    None,
    /// Strip headers and pass through
    BypassAndStrip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Prefix,
    Exact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub routes: Vec<RouteConfig>,
    pub jwks_url: String,
    pub addr: String,
    pub upstream_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub path: String,
    pub auth: AuthenticationLevel,
    pub rate_limit: RateLimitConfig,
    pub upstream: String,
    pub match_type: MatchType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub per_sec: u64,
    pub burst: u64,
}

const DEFAULT_CONFIG_PATH: &str = "./config/gateway.yaml";
const CONFIG_PATH_ENV: &str = "GATEWAY_CONFIG_PATH";
const ENV_PREFIX: &str = "GATEWAY";

impl GatewayConfig {
    pub fn load() -> Result<Self, config::ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = config::Config::builder()
            .add_source(config::File::with_name(&config_path).required(false))
            .add_source(config::Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
