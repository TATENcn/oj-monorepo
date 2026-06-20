use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use shared::{
    models::{VerdictTask, VerdictTaskResult, http::PoolMetrics},
    protocol::{self, FrameId, ProtocolError},
};
use tokio::{
    net::UnixStream,
    sync::{Mutex, Notify, RwLock, mpsc, oneshot},
    task::JoinSet,
    time::{self, interval, timeout},
};
use tracing::{debug, error, info, warn};

use crate::provisioner::{ContainerdProvisioner, ProvisionError};

const DRAIN_CHECK_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_queue_size: usize,
    pub max_retries: u32,
    pub task_timeout: Duration,
    pub health_check_interval: Duration,
    pub health_check_failure_threshold: u32,
    pub max_concurrent_per_agent: u32,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_retries: 3,
            task_timeout: Duration::from_secs(45),
            health_check_interval: Duration::from_secs(5),
            health_check_failure_threshold: 3,
            max_concurrent_per_agent: 5,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("task queue is full")]
    QueueFull,
    #[error("max retries ({retries}) exceeded")]
    MaxRetriesExceeded { retries: u32 },
    #[error("no healthy agent available")]
    AgentUnavailable,
    #[error("connection failed: {0}")]
    ConnectionFailed(#[from] std::io::Error),
    #[error("task timed out after {0}s")]
    TaskTimeout(u64),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("provision error: {0}")]
    Provision(#[from] ProvisionError),
    #[error("agent {agent_id} is busy with active tasks")]
    AgentBusy { agent_id: String },
    #[error("pool is shutting down")]
    ShuttingDown,
}

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: String,
    pub socket_path: PathBuf,
    pub active_tasks: Arc<AtomicU32>,
    pub healthy: Arc<AtomicBool>,
    pub consecutive_failures: Arc<AtomicU32>,
    pub created_at: Instant,
    pub shutting_down: Arc<AtomicBool>,
}

#[derive(Debug)]
struct QueuedTask {
    frame_id: FrameId,
    task: VerdictTask,
    retries: u32,
    result_tx: oneshot::Sender<Result<VerdictTaskResult, PoolError>>,
}

pub struct AgentPool {
    config: PoolConfig,
    agents: Arc<RwLock<Vec<AgentHandle>>>,
    task_queue: Arc<Mutex<VecDeque<QueuedTask>>>,
    provisioner: Arc<ContainerdProvisioner>,
    next_frame_id: AtomicU64,
    task_signal_tx: mpsc::UnboundedSender<()>,
    agent_available: Arc<Notify>,
    shutting_down: AtomicBool,
}

impl AgentPool {
    pub async fn new(config: PoolConfig, provisioner: ContainerdProvisioner) -> Self {
        let agents = Arc::new(RwLock::new(Vec::new()));
        let task_queue = Arc::new(Mutex::new(VecDeque::new()));
        let (task_signal_tx, task_signal_rx) = mpsc::unbounded_channel();
        let agent_available = Arc::new(Notify::new());
        let provisioner = Arc::new(provisioner);

        let pool = Self {
            config: config.clone(),
            agents: agents.clone(),
            task_queue: task_queue.clone(),
            provisioner: provisioner.clone(),
            next_frame_id: AtomicU64::new(1),
            task_signal_tx: task_signal_tx.clone(),
            agent_available: agent_available.clone(),
            shutting_down: AtomicBool::new(false),
        };

        tokio::spawn(dispatch_loop(
            agents.clone(),
            task_queue.clone(),
            task_signal_tx,
            task_signal_rx,
            agent_available.clone(),
            config,
        ));

        tokio::spawn(health_check_loop(
            agents.clone(),
            agent_available.clone(),
            pool.config.health_check_interval,
            pool.config.health_check_failure_threshold,
        ));

        tokio::spawn(drain_loop(agents.clone(), provisioner));

        pool
    }

    #[tracing::instrument(skip(self, task), fields(frame_id))]
    pub async fn submit(&self, task: VerdictTask) -> Result<VerdictTaskResult, PoolError> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(PoolError::ShuttingDown);
        }

        let frame_id = self.next_frame_id.fetch_add(1, Ordering::SeqCst);
        tracing::Span::current().record("frame_id", frame_id);

        let (tx, rx) = oneshot::channel();

        {
            let mut queue = self.task_queue.lock().await;
            if queue.len() >= self.config.max_queue_size {
                warn!(frame_id, queue_size = queue.len(), "task queue full, rejecting");
                return Err(PoolError::QueueFull);
            }

            let was_empty = queue.is_empty();

            queue.push_back(QueuedTask {
                frame_id,
                task,
                retries: 0,
                result_tx: tx,
            });
            debug!(frame_id, queue_size = queue.len(), "task queued");

            if was_empty {
                let _ = self.task_signal_tx.send(());
            }
        }

        rx.await.map_err(|_| PoolError::AgentUnavailable)?
    }

    pub async fn metrics(&self) -> PoolMetrics {
        let queue = self.task_queue.lock().await;
        let agents = self.agents.read().await;

        let total_active: u32 = agents.iter().map(|a| a.active_tasks.load(Ordering::Relaxed)).sum();
        let healthy_count = agents.iter().filter(|a| a.healthy.load(Ordering::Relaxed)).count();
        let draining_count = agents.iter().filter(|a| a.shutting_down.load(Ordering::Relaxed)).count();
        let unhealthy_count = agents.iter().filter(|a| !a.healthy.load(Ordering::Relaxed)).count();

        PoolMetrics {
            queue_size: queue.len(),
            agent_count: agents.len(),
            healthy_agent_count: healthy_count,
            active_tasks: total_active,
            draining_agent_count: draining_count,
            unhealthy_agent_count: unhealthy_count,
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn discover_agents(&self) -> Result<(), PoolError> {
        let ids = self.provisioner.list().await?;
        info!(count = ids.len(), "discovered existing agents");

        for id in ids {
            let socket_path = PathBuf::from("/run/judge-core/agents").join(&id).join("agent.sock");

            let is_alive = timeout(Duration::from_secs(1), async {
                let mut stream = UnixStream::connect(&socket_path).await?;
                protocol::send_heartbeat(&mut stream).await?;
                Ok::<(), PoolError>(())
            })
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);

            if is_alive {
                self.add_agent(id, socket_path).await;
            } else {
                info!(agent_id = %id, "discovered agent is unreachable, destroying");
                let _ = self.provisioner.destroy(&id).await;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn create_agent(&self) -> Result<(), PoolError> {
        match self.provisioner.create().await {
            Ok((id, socket_path)) => {
                self.add_agent(id, socket_path).await;
                Ok(())
            }
            Err(e) => {
                error!(error = %e, "failed to create agent");
                Err(PoolError::Provision(e))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn add_agent(&self, id: String, socket_path: PathBuf) {
        let mut agents = self.agents.write().await;
        if agents.iter().any(|a| a.id == id) {
            warn!(agent_id = %id, "agent already exists in pool");
            return;
        }

        agents.push(AgentHandle {
            id: id.clone(),
            socket_path,
            active_tasks: Arc::new(AtomicU32::new(0)),
            healthy: Arc::new(AtomicBool::new(true)),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            created_at: Instant::now(),
            shutting_down: Arc::new(AtomicBool::new(false)),
        });

        info!(agent_id = %id, agent_count = agents.len(), "agent added to pool");
        self.agent_available.notify_one();
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove_agent(&self, id: &str) -> Result<AgentHandle, PoolError> {
        let mut agents = self.agents.write().await;
        let pos = agents.iter().position(|a| a.id == id).ok_or(PoolError::AgentUnavailable)?;
        let agent = agents.remove(pos);
        info!(agent_id = %id, agent_count = agents.len(), "agent removed from pool");
        Ok(agent)
    }

    #[tracing::instrument(skip(self))]
    pub async fn shutdown_agent(&self, id: &str) -> Result<(), PoolError> {
        let agents = self.agents.read().await;
        let agent = agents.iter().find(|a| a.id == id).ok_or(PoolError::AgentUnavailable)?;
        agent.shutting_down.store(true, Ordering::Relaxed);
        info!(agent_id = %id, "agent marked for shutdown");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn start_autoscaler(self: Arc<Self>, config: crate::scaler::ScalerConfig) {
        tokio::spawn(crate::scaler::AutoScaler::run(self, config));
    }

    pub async fn find_least_loaded_agent(&self) -> Option<AgentHandle> {
        let agents = self.agents.read().await;
        agents
            .iter()
            .filter(|a| a.healthy.load(Ordering::Relaxed) && !a.shutting_down.load(Ordering::Relaxed))
            .min_by_key(|a| a.active_tasks.load(Ordering::Relaxed))
            .cloned()
    }

    #[tracing::instrument(skip(self))]
    pub async fn shutdown(&self, drain_timeout: Duration) -> Result<(), PoolError> {
        info!("initiating pool shutdown");

        self.shutting_down.store(true, Ordering::SeqCst);

        let mut queue = self.task_queue.lock().await;
        let rejected = queue.len();
        while let Some(task) = queue.pop_front() {
            let _ = task.result_tx.send(Err(PoolError::ShuttingDown));
        }
        drop(queue);
        if rejected > 0 {
            info!(rejected, "rejected queued tasks during shutdown");
        }

        let agent_ids: Vec<String> = {
            let agents = self.agents.read().await;
            for a in agents.iter() {
                a.shutting_down.store(true, Ordering::Relaxed);
            }
            agents.iter().map(|a| a.id.clone()).collect()
        };
        info!(agent_count = agent_ids.len(), "marked agents for shutdown");

        let start = Instant::now();
        loop {
            let metrics = self.metrics().await;
            if metrics.active_tasks == 0 {
                break;
            }
            if start.elapsed() >= drain_timeout {
                warn!(
                    active_tasks = metrics.active_tasks,
                    timeout_secs = drain_timeout.as_secs(),
                    "drain timeout reached, forcing shutdown"
                );

                break;
            }
            time::sleep(Duration::from_millis(100)).await;
        }

        info!("destroying agents");
        {
            let mut guard = self.agents.write().await;
            guard.clear();
        }

        destroy_agents_parallel(self.provisioner.clone(), agent_ids, "shutdown".into()).await;

        info!("pool shutdown complete");
        Ok(())
    }
}

async fn destroy_agents_parallel(provisioner: Arc<ContainerdProvisioner>, agent_ids: Vec<String>, context: String) {
    if agent_ids.is_empty() {
        return;
    }
    let mut join_set = JoinSet::new();
    for id in agent_ids {
        let provisioner = provisioner.clone();
        let context = context.clone();
        join_set.spawn(async move {
            match provisioner.destroy(&id).await {
                Err(e) => error!(agent_id = %id, error = %e, "failed to destroy agent ({context})"),
                Ok(()) => info!(agent_id = %id, "agent destroyed ({context})"),
            }
        });
    }
    while let Some(result) = join_set.join_next().await {
        if let Err(e) = result {
            error!(error = %e, "destroy task panicked ({context})");
        }
    }
}

async fn dispatch_loop(
    agents: Arc<RwLock<Vec<AgentHandle>>>,
    queue: Arc<Mutex<VecDeque<QueuedTask>>>,
    task_signal_tx: mpsc::UnboundedSender<()>,
    mut task_signal_rx: mpsc::UnboundedReceiver<()>,
    agent_available: Arc<Notify>,
    config: PoolConfig,
) {
    loop {
        let mut task;
        let agent;

        {
            let mut q = queue.lock().await;
            if q.is_empty() {
                drop(q);
                task_signal_rx.recv().await.expect("task_signal sender should not be dropped");
                continue;
            }

            let agents_guard = agents.read().await;
            if let Some(a) = agents_guard
                .iter()
                .filter(|a| {
                    a.healthy.load(Ordering::Relaxed)
                        && !a.shutting_down.load(Ordering::Relaxed)
                        && a.active_tasks.load(Ordering::Relaxed) < config.max_concurrent_per_agent
                })
                .min_by_key(|a| a.active_tasks.load(Ordering::Relaxed))
                .cloned()
            {
                agent = a;
                task = q.pop_front().unwrap();
            } else {
                drop(agents_guard);
                drop(q);
                tokio::select! {
                    _ = agent_available.notified() => {},
                    _ = task_signal_rx.recv() => {},
                }
                continue;
            }
        }

        agent.active_tasks.fetch_add(1, Ordering::Relaxed);
        debug!(
            frame_id = task.frame_id,
            agent_id = %agent.id,
            active_tasks = agent.active_tasks.load(Ordering::Relaxed),
            "dispatching task"
        );

        let queue_clone = queue.clone();
        let agent_available_clone = agent_available.clone();
        let task_signal_tx_clone = task_signal_tx.clone();
        let config = config.clone();

        tokio::spawn(async move {
            let result = execute_task(&agent, task.frame_id, &task.task, &config).await;

            let prev = agent.active_tasks.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                agent_available_clone.notify_one();
            }

            match result {
                Ok(res) => {
                    debug!(frame_id = task.frame_id, "task completed");
                    let _ = task.result_tx.send(Ok(res));
                }
                Err(e) => {
                    warn!(frame_id = task.frame_id, error = %e, retries = task.retries, "task failed");

                    if task.retries < config.max_retries {
                        task.retries += 1;
                        let mut q = queue_clone.lock().await;
                        q.push_back(task);
                        let _ = task_signal_tx_clone.send(());
                    } else {
                        let _ = task.result_tx.send(Err(PoolError::MaxRetriesExceeded { retries: task.retries }));
                    }
                }
            }
        });
    }
}

#[tracing::instrument(skip(agent, task, config), fields(frame_id))]
async fn execute_task(agent: &AgentHandle, frame_id: FrameId, task: &VerdictTask, config: &PoolConfig) -> Result<VerdictTaskResult, PoolError> {
    let start = Instant::now();

    debug!(socket = %agent.socket_path.display(), "connecting to agent");
    let mut stream = timeout(Duration::from_secs(5), UnixStream::connect(&agent.socket_path))
        .await
        .map_err(|_| PoolError::TaskTimeout(5))??;

    debug!("sending task");
    protocol::send(&mut stream, frame_id, task).await?;

    debug!("waiting for response");
    let (_, result): (FrameId, VerdictTaskResult) = timeout(config.task_timeout, protocol::receive::<VerdictTaskResult, _>(&mut stream))
        .await
        .map_err(|_| PoolError::TaskTimeout(config.task_timeout.as_secs()))?
        .map_err(PoolError::Protocol)?
        .ok_or(PoolError::Protocol(ProtocolError::UnexpectedHeartbeat))?;

    let duration = start.elapsed();
    info!(frame_id, duration = ?duration, "task executed successfully");

    Ok(result)
}

async fn health_check_loop(agents: Arc<RwLock<Vec<AgentHandle>>>, agent_available: Arc<Notify>, interval: Duration, failure_threshold: u32) {
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;

        let snapshot: Vec<AgentHandle> = {
            let guard = agents.read().await;
            guard.clone()
        };

        let mut join_set = JoinSet::new();
        for agent in snapshot {
            join_set.spawn(async move {
                let result = timeout(Duration::from_secs(2), async {
                    let mut stream = UnixStream::connect(&agent.socket_path).await?;
                    protocol::send_heartbeat(&mut stream).await?;
                    Ok::<(), PoolError>(())
                })
                .await;

                let healthy = match result {
                    Ok(Ok(())) => {
                        agent.consecutive_failures.store(0, Ordering::Relaxed);
                        true
                    }
                    Ok(Err(e)) => {
                        let failures = agent.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                        warn!(agent_id = %agent.id, failures, error = %e, "health check failed");
                        failures < failure_threshold
                    }
                    Err(_) => {
                        let failures = agent.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                        warn!(agent_id = %agent.id, failures, "health check timed out");
                        failures < failure_threshold
                    }
                };

                (agent, healthy)
            });
        }

        while let Some(result) = join_set.join_next().await {
            let (agent, healthy) = result.expect("health check task panicked");
            let was_healthy = agent.healthy.load(Ordering::Relaxed);
            agent.healthy.store(healthy, Ordering::Relaxed);

            if was_healthy && !healthy {
                error!(agent_id = %agent.id, "agent marked unhealthy");
            } else if !was_healthy && healthy {
                info!(agent_id = %agent.id, "agent recovered");
                agent.consecutive_failures.store(0, Ordering::Relaxed);
                agent_available.notify_one();
            }
        }
    }
}

async fn drain_loop(agents: Arc<RwLock<Vec<AgentHandle>>>, provisioner: Arc<ContainerdProvisioner>) {
    let mut ticker = interval(Duration::from_secs(DRAIN_CHECK_INTERVAL_SECS));

    loop {
        ticker.tick().await;

        let snapshot = {
            let guard = agents.read().await;
            guard.clone()
        };

        let candidates: Vec<AgentHandle> = snapshot
            .into_iter()
            .filter(|agent| {
                !agent.healthy.load(Ordering::Relaxed) || (agent.shutting_down.load(Ordering::Relaxed) && agent.active_tasks.load(Ordering::Relaxed) == 0)
            })
            .collect();

        if candidates.is_empty() {
            continue;
        }

        {
            let mut guard = agents.write().await;
            guard.retain(|a| !candidates.iter().any(|c| c.id == a.id));
        }

        let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        destroy_agents_parallel(provisioner.clone(), ids, "drain".into()).await;
    }
}
