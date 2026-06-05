use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use shared::{
    models::{VerdictTask, VerdictTaskResult},
    protocol::{self, FrameId, ProtocolError},
};
use tokio::{
    net::UnixStream,
    sync::{Mutex, RwLock, mpsc, oneshot},
    time::{sleep, timeout},
};
use tracing::{debug, error, info, warn};

use crate::provisioner::{ContainerdProvisioner, ProvisionError};

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_queue_size: usize,
    pub max_retries: u32,
    pub task_timeout: Duration,
    pub health_check_interval: Duration,
    pub health_check_failure_threshold: u32,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_retries: 3,
            task_timeout: Duration::from_secs(45),
            health_check_interval: Duration::from_secs(5),
            health_check_failure_threshold: 3,
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
}

#[derive(Debug, Clone)]
pub struct PoolMetrics {
    pub queue_size: usize,
    pub agent_count: usize,
    pub healthy_agent_count: usize,
    pub active_tasks: u32,
}

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: String,
    pub socket_path: PathBuf,
    pub active_tasks: Arc<AtomicU32>,
    pub healthy: Arc<AtomicBool>,
    pub consecutive_failures: Arc<AtomicU32>,
    pub created_at: Instant,
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
    dispatch_tx: mpsc::Sender<()>,
    pub provisioner: ContainerdProvisioner,
    next_frame_id: AtomicU32,
}

impl AgentPool {
    pub async fn new(config: PoolConfig, provisioner: ContainerdProvisioner) -> Self {
        let (dispatch_tx, dispatch_rx) = mpsc::channel::<()>(1024);

        let agents = Arc::new(RwLock::new(Vec::new()));
        let task_queue = Arc::new(Mutex::new(VecDeque::new()));

        let pool = Self {
            config: config.clone(),
            agents: agents.clone(),
            task_queue: task_queue.clone(),
            dispatch_tx: dispatch_tx.clone(),
            provisioner,
            next_frame_id: AtomicU32::new(1),
        };

        tokio::spawn(dispatch_loop(agents.clone(), task_queue.clone(), dispatch_rx, dispatch_tx.clone(), config));

        tokio::spawn(health_check_loop(
            agents.clone(),
            pool.config.health_check_interval,
            pool.config.health_check_failure_threshold,
        ));

        pool
    }

    #[tracing::instrument(skip(self, task), fields(frame_id))]
    pub async fn submit(&self, task: VerdictTask) -> Result<VerdictTaskResult, PoolError> {
        let frame_id = self.next_frame_id.fetch_add(1, Ordering::SeqCst) as u64;
        tracing::Span::current().record("frame_id", frame_id);

        let (tx, rx) = oneshot::channel();

        {
            let mut queue = self.task_queue.lock().await;
            if queue.len() >= self.config.max_queue_size {
                warn!(frame_id, queue_size = queue.len(), "task queue full, rejecting");
                return Err(PoolError::QueueFull);
            }

            queue.push_back(QueuedTask {
                frame_id,
                task,
                retries: 0,
                result_tx: tx,
            });
            debug!(frame_id, queue_size = queue.len(), "task queued");
        }

        if self.dispatch_tx.send(()).await.is_err() {
            return Err(PoolError::AgentUnavailable);
        }

        rx.await.map_err(|_| PoolError::AgentUnavailable)?
    }

    pub async fn metrics(&self) -> PoolMetrics {
        let agents = self.agents.read().await;
        let queue = self.task_queue.lock().await;

        PoolMetrics {
            queue_size: queue.len(),
            agent_count: agents.len(),
            healthy_agent_count: agents.iter().filter(|a| a.healthy.load(Ordering::Relaxed)).count(),
            active_tasks: agents.iter().map(|a| a.active_tasks.load(Ordering::Relaxed)).sum(),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn discover_agents(&self) -> Result<(), PoolError> {
        let ids = self.provisioner.list().await?;
        info!(count = ids.len(), "discovered existing agents");

        for id in ids {
            let socket_path = PathBuf::from("/run/judge-core/agents").join(&id).join("agent.sock");
            self.add_agent(id, socket_path).await;
        }

        Ok(())
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
        });

        info!(agent_id = %id, agent_count = agents.len(), "agent added to pool");
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove_agent(&self, id: &str) -> Option<AgentHandle> {
        let mut agents = self.agents.write().await;
        let pos = agents.iter().position(|a| a.id == id)?;
        let agent = agents.remove(pos);
        info!(agent_id = %id, agent_count = agents.len(), "agent removed from pool");
        Some(agent)
    }

    #[tracing::instrument(skip(self))]
    pub fn start_autoscaler(self: Arc<Self>, config: crate::scaler::ScalerConfig) {
        tokio::spawn(crate::scaler::AutoScaler::run(self, config));
    }

    pub async fn find_oldest_idle_agent(&self) -> Option<AgentHandle> {
        let agents = self.agents.read().await;
        agents
            .iter()
            .filter(|a| a.healthy.load(Ordering::Relaxed) && a.active_tasks.load(Ordering::Relaxed) == 0)
            .min_by_key(|a| a.created_at)
            .cloned()
    }
}

async fn dispatch_loop(
    agents: Arc<RwLock<Vec<AgentHandle>>>,
    queue: Arc<Mutex<VecDeque<QueuedTask>>>,
    mut rx: mpsc::Receiver<()>,
    dispatch_tx: mpsc::Sender<()>,
    config: PoolConfig,
) {
    while rx.recv().await.is_some() {
        let task = {
            let mut queue = queue.lock().await;
            queue.pop_front()
        };

        let Some(mut task) = task else {
            continue;
        };

        let agent = {
            let agents = agents.read().await;
            agents
                .iter()
                .filter(|a| a.healthy.load(Ordering::Relaxed))
                .min_by_key(|a| a.active_tasks.load(Ordering::Relaxed))
                .cloned()
        };

        let config = config.clone();
        let Some(agent) = agent else {
            warn!(frame_id = task.frame_id, "no healthy agent available");
            if task.retries >= config.max_retries {
                let _ = task.result_tx.send(Err(PoolError::AgentUnavailable));
            } else {
                task.retries += 1;
                let mut q = queue.lock().await;
                q.push_front(task);

                tokio::spawn({
                    let tx = dispatch_tx.clone();
                    async move {
                        sleep(Duration::from_secs(1)).await;
                        tx.send(()).await.ok();
                    }
                });
            }
            continue;
        };

        agent.active_tasks.fetch_add(1, Ordering::Relaxed);
        debug!(
            frame_id = task.frame_id,
            agent_id = %agent.id,
            active_tasks = agent.active_tasks.load(Ordering::Relaxed),
            "dispatching task"
        );

        let queue_clone = queue.clone();
        let dispatch_tx_clone = dispatch_tx.clone();

        tokio::spawn(async move {
            let result = execute_task(&agent, task.frame_id, &task.task, &config).await;
            agent.active_tasks.fetch_sub(1, Ordering::Relaxed);

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
                        dispatch_tx_clone.send(()).await.ok();
                    } else {
                        let _ = task.result_tx.send(Err(PoolError::MaxRetriesExceeded { retries: task.retries }));
                    }
                }
            }
        });
    }
}

#[tracing::instrument(skip(agent, task), fields(frame_id))]
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
        .ok_or(PoolError::Protocol(ProtocolError::InvalidHeartbeatResponse))?;

    let duration = start.elapsed();
    info!(frame_id, duration = ?duration, "task executed successfully");

    Ok(result)
}

async fn health_check_loop(agents: Arc<RwLock<Vec<AgentHandle>>>, interval: Duration, failure_threshold: u32) {
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;
        let agents = agents.read().await;

        for agent in agents.iter() {
            let result = timeout(Duration::from_secs(2), async {
                let mut stream = UnixStream::connect(&agent.socket_path).await?;
                protocol::send_heartbeat(&mut stream).await?;
                Ok::<(), PoolError>(())
            })
            .await;

            let healthy = match result {
                Ok(Ok(())) => true,
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

            let was_healthy = agent.healthy.load(Ordering::Relaxed);
            agent.healthy.store(healthy, Ordering::Relaxed);

            if was_healthy && !healthy {
                error!(agent_id = %agent.id, "agent marked unhealthy");
            } else if !was_healthy && healthy {
                info!(agent_id = %agent.id, "agent recovered");
                agent.consecutive_failures.store(0, Ordering::Relaxed);
            }
        }
    }
}
