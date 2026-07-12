use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum FieldError {
    #[error("{field}: {message}")]
    Invalid { field: String, message: &'static str },
}

#[derive(Debug)]
pub struct ConfigValidationError(Vec<FieldError>);

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "config validation failed:")?;
        for err in &self.0 {
            writeln!(f, "  - {err}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigValidationError {}

#[derive(Debug, thiserror::Error)]
pub enum GatewayConfigError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Validation(#[from] ConfigValidationError),
}

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
    pub max_connections: usize, // Uses [`usize`] for [`Semaphore`]
    pub drain_timeout_secs: u64,
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
    pub fn load() -> Result<Self, GatewayConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = config::Config::builder()
            .add_source(config::File::with_name(&config_path).required(false))
            .add_source(config::Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        let cfg: Self = config.try_deserialize()?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), ConfigValidationError> {
        let mut errors: Vec<FieldError> = Vec::new();

        if self.addr.is_empty() {
            errors.push(FieldError::Invalid {
                field: "addr".into(),
                message: "must not be empty",
            });
        }
        if self.jwks_url.is_empty() {
            errors.push(FieldError::Invalid {
                field: "jwks_url".into(),
                message: "must not be empty",
            });
        }
        if self.upstream_timeout_secs == 0 {
            errors.push(FieldError::Invalid {
                field: "upstream_timeout_secs".into(),
                message: "must be > 0",
            });
        }
        if self.max_connections == 0 {
            errors.push(FieldError::Invalid {
                field: "max_connections".into(),
                message: "must be > 0",
            });
        }
        if self.drain_timeout_secs == 0 {
            errors.push(FieldError::Invalid {
                field: "drain_timeout_secs".into(),
                message: "must be > 0",
            });
        }

        for (i, route) in self.routes.iter().enumerate() {
            if route.path.is_empty() {
                errors.push(FieldError::Invalid {
                    field: format!("routes[{i}].path"),
                    message: "must not be empty",
                });
            }
            if route.upstream.is_empty() {
                errors.push(FieldError::Invalid {
                    field: format!("routes[{i}].upstream"),
                    message: "must not be empty",
                });
            }
            if route.rate_limit.per_sec == 0 {
                errors.push(FieldError::Invalid {
                    field: format!("routes[{i}].rate_limit.per_sec"),
                    message: "must be > 0",
                });
            }
            if route.rate_limit.burst == 0 {
                errors.push(FieldError::Invalid {
                    field: format!("routes[{i}].rate_limit.burst"),
                    message: "must be > 0",
                });
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(ConfigValidationError(errors)) }
    }
}
