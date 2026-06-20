use std::{
    sync::Arc,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use shared::models::http::PoolMetrics;
use tokio::time::interval;
use tracing::{debug, error, info};

use crate::pool::AgentPool;

#[derive(Debug, Clone)]
pub struct ScalerConfig {
    pub min_agents: usize,
    pub max_agents: usize,
    pub scale_down_utilization_pct: f64,
    pub scale_up_cooldown_secs: u64,
    pub scale_down_cooldown_secs: u64,
    pub check_interval_secs: u64,
    pub provision_time_secs: u64,
    pub max_scale_up_batch: usize,
    pub scale_down_confirm_ticks: u32,
    pub ema_alpha: f64,
    pub max_concurrent_per_agent: u32,
}

impl Default for ScalerConfig {
    fn default() -> Self {
        Self {
            min_agents: 2,
            max_agents: 10,
            scale_down_utilization_pct: 0.3,
            scale_up_cooldown_secs: 5,
            scale_down_cooldown_secs: 300,
            check_interval_secs: 10,
            provision_time_secs: 30,
            max_scale_up_batch: 3,
            scale_down_confirm_ticks: 3,
            ema_alpha: 0.3,
            max_concurrent_per_agent: 5,
        }
    }
}

struct QueueVelocityTracker {
    prev_queue_size: usize,
    velocity: f64,
    alpha: f64,
    check_interval_secs: u64,
}

impl QueueVelocityTracker {
    fn new(alpha: f64, check_interval_secs: u64) -> Self {
        Self {
            prev_queue_size: 0,
            velocity: 0.0,
            alpha,
            check_interval_secs,
        }
    }

    /// Returns velocity in tasks per second
    fn update(&mut self, queue_size: usize) -> f64 {
        let delta = queue_size as f64 - self.prev_queue_size as f64;
        let raw_velocity = self.alpha * delta + (1.0 - self.alpha) * self.velocity;
        self.velocity = raw_velocity;
        self.prev_queue_size = queue_size;
        if self.check_interval_secs > 0 {
            raw_velocity / self.check_interval_secs as f64
        } else {
            0.0
        }
    }
}

pub struct AutoScaler;

impl AutoScaler {
    fn scale_up_decision(metrics: &PoolMetrics, velocity: f64, config: &ScalerConfig) -> usize {
        let capacity_per_agent = config.max_concurrent_per_agent as usize;
        if capacity_per_agent == 0 {
            return 0;
        }

        let total_load = metrics.active_tasks as usize + metrics.queue_size;

        let provision_buffer = if velocity > 0.0 {
            (velocity * config.provision_time_secs as f64).ceil() as usize
        } else {
            0
        };

        let needed_capacity = total_load + provision_buffer;
        let needed_agents = needed_capacity.div_ceil(capacity_per_agent);
        let need = needed_agents.saturating_sub(metrics.agent_count);

        need.min(config.max_scale_up_batch).min(config.max_agents.saturating_sub(metrics.agent_count))
    }

    fn scale_down_decision(metrics: &PoolMetrics, config: &ScalerConfig, confirm_counter: u32) -> (bool, u32) {
        let effective_agents = metrics.agent_count.saturating_sub(metrics.draining_agent_count).max(1);
        let max_capacity = effective_agents as f64 * config.max_concurrent_per_agent as f64;
        let utilization = if max_capacity > 0.0 {
            metrics.active_tasks as f64 / max_capacity
        } else {
            0.0
        };

        let below_threshold = utilization < config.scale_down_utilization_pct;
        let above_min = metrics.agent_count > config.min_agents;

        if below_threshold && above_min {
            let new_counter = confirm_counter + 1;
            let should_scale = new_counter >= config.scale_down_confirm_ticks;
            (should_scale, if should_scale { 0 } else { new_counter })
        } else {
            (false, 0)
        }
    }
    #[tracing::instrument(skip(pool, config))]
    pub async fn run(pool: Arc<AgentPool>, config: ScalerConfig) {
        let mut ticker = interval(Duration::from_secs(config.check_interval_secs));
        let mut last_scale_up = Instant::now() - Duration::from_secs(config.scale_up_cooldown_secs);
        let mut last_scale_down = Instant::now() - Duration::from_secs(config.scale_down_cooldown_secs);
        let mut velocity_tracker = QueueVelocityTracker::new(config.ema_alpha, config.check_interval_secs);
        let mut scale_down_confirm_counter: u32 = 0;

        loop {
            ticker.tick().await;

            let metrics = pool.metrics().await;
            let velocity = velocity_tracker.update(metrics.queue_size);

            debug!(
                queue_size = metrics.queue_size,
                agent_count = metrics.agent_count,
                healthy_agents = metrics.healthy_agent_count,
                active_tasks = metrics.active_tasks,
                draining_agents = metrics.draining_agent_count,
                unhealthy_agents = metrics.unhealthy_agent_count,
                velocity,
                "scaler evaluation"
            );

            let now = Instant::now();

            let needs_scale_up = Self::scale_up_decision(&metrics, velocity, &config);

            if needs_scale_up > 0 {
                if now.duration_since(last_scale_up).as_secs() < config.scale_up_cooldown_secs {
                    debug!("scale-up cooldown active, skipping");
                } else {
                    info!(
                        queue_size = metrics.queue_size,
                        agent_count = metrics.agent_count,
                        need = needs_scale_up,
                        "scaling up: creating agents"
                    );
                    last_scale_up = now;

                    for _ in 0..needs_scale_up {
                        let pool_clone = pool.clone();
                        tokio::spawn(async move {
                            if let Err(e) = pool_clone.create_agent().await {
                                error!(error = %e, "failed to create agent during scale-up");
                            }
                        });
                    }
                }
            } else {
                let (should_scale_down, new_counter) = Self::scale_down_decision(&metrics, &config, scale_down_confirm_counter);
                scale_down_confirm_counter = new_counter;

                if should_scale_down {
                    if now.duration_since(last_scale_down).as_secs() < config.scale_down_cooldown_secs {
                        debug!("scale-down cooldown active, skipping");
                    } else {
                        if let Some(agent) = pool.find_least_loaded_agent().await {
                            let id = agent.id.clone();
                            info!(
                                agent_id = %id,
                                active_tasks = agent.active_tasks.load(Ordering::Relaxed),
                                "scaling down: shutting down agent"
                            );
                            last_scale_down = now;
                            scale_down_confirm_counter = 0;

                            if let Err(e) = pool.shutdown_agent(&id).await {
                                error!(agent_id = %id, error = %e, "failed to shutdown agent during scale-down");
                            }
                        } else {
                            debug!("no suitable agent found for scale-down");
                        }
                    }
                }
            }
        }
    }
}
