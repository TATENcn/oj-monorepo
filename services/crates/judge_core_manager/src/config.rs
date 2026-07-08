use std::{path::Path, time::Duration};

use confide::confide;
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "./config/manager.toml";
const CONFIG_PATH_ENV: &str = "JC_MANAGER_CONFIG_PATH";
const ENV_PREFIX: &str = "JC_MANAGER";

#[confide]
#[derive(Clone, Deserialize)]
pub struct PoolConfig {
    #[confide(default = 1000)]
    pub max_queue_size: usize,
    #[confide(default = 3)]
    pub max_retries: u32,
    #[confide(default_duration = "45s")]
    pub task_timeout: Duration,
    #[confide(default_duration = "5s")]
    pub health_check_interval: Duration,
    #[confide(default = 3)]
    pub health_check_failure_threshold: u32,
    #[confide(default = 5)]
    pub max_concurrent_per_agent: u32,
    #[confide(default = 5)]
    pub drain_check_interval_secs: u64,
}

#[confide]
#[derive(Clone, Deserialize)]
pub struct ScalerConfig {
    #[confide(default = 2)]
    pub min_agents: usize,
    #[confide(default = 10)]
    pub max_agents: usize,
    #[confide(default = 0.3)]
    pub scale_down_utilization_pct: f64,
    #[confide(default = 5)]
    pub scale_up_cooldown_secs: u64,
    #[confide(default = 300)]
    pub scale_down_cooldown_secs: u64,
    #[confide(default = 10)]
    pub check_interval_secs: u64,
    #[confide(default = 30)]
    pub provision_time_secs: u64,
    #[confide(default = 3)]
    pub max_scale_up_batch: usize,
    #[confide(default = 3)]
    pub scale_down_confirm_ticks: u32,
    #[confide(default = 0.3)]
    pub ema_alpha: f64,
    #[confide(default = 5)]
    pub max_concurrent_per_agent: u32,
}

#[confide]
#[derive(Clone, Deserialize)]
pub struct ProvisionerConfig {
    #[confide(default = "judge-core".to_string())]
    pub namespace: String,
    #[confide(default = "io.containerd.runc.v2".to_string())]
    pub runtime: String,
}

#[confide]
#[derive(Deserialize)]
pub struct ServerConfig {
    #[confide(default = "0.0.0.0:8000".to_string())]
    pub bind_address: String,
    #[confide(default = "/run/containerd/containerd.sock".to_string())]
    pub containerd_socket: String,
    #[confide(default = "docker.io/library/judge-core:latest".to_string())]
    pub image: String,
}

#[derive(Debug, Deserialize)]
pub struct ManagerConfig {
    #[serde(default)]
    pub pool: PoolConfig,
    #[serde(default)]
    pub scaler: ScalerConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub provisioner: ProvisionerConfig,
}

impl ManagerConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.into());

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }

    #[allow(dead_code)]
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::with_name(path.as_ref().to_str().unwrap()).required(false))
            .add_source(Environment::with_prefix(ENV_PREFIX).separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
