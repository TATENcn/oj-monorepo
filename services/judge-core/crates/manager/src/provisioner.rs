use std::path::{Path, PathBuf};
use std::time::Instant;

use containerd_client::tonic::Request;
use containerd_client::{
    services::v1::{
        Container, CreateContainerRequest, CreateTaskRequest, DeleteContainerRequest, DeleteTaskRequest, KillRequest, ListContainersRequest, StartRequest,
        container::Runtime, containers_client::ContainersClient, tasks_client::TasksClient,
    },
    tonic::transport::Channel,
    with_namespace,
};
use prost_types::Any;
use tokio::fs;
use tokio::time::{Duration, sleep};
use tracing::{debug, info, warn};
use uuid::Uuid;

const NAMESPACE: &str = "judge-core";
const SOCKET_WAIT_TIMEOUT_SECS: u64 = 30;
const SOCKET_WAIT_INTERVAL_MS: u64 = 100;

const RUNTIME: &str = "io.containerd.runc.v2";

pub struct ContainerdProvisioner {
    channel: Channel,
    image: String,
    socket_base: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("containerd rpc error: {0}")]
    Rpc(#[from] containerd_client::tonic::Status),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("socket not ready after {0}s")]
    SocketTimeout(u64),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("oci spec error: {0}")]
    OciSpec(#[from] oci_spec::OciSpecError),
}

impl ContainerdProvisioner {
    pub fn new(channel: Channel, image: impl Into<String>) -> Self {
        Self {
            channel,
            image: image.into(),
            socket_base: PathBuf::from("/run/judge-core/agents"),
        }
    }

    /// Create a new agent container
    #[tracing::instrument(skip(self))]
    pub async fn create(&self) -> Result<(String, PathBuf), ProvisionError> {
        let id = Uuid::new_v4().to_string();
        let socket_dir = self.socket_base.join(&id);
        let socket_path = socket_dir.join("agent.sock");

        fs::create_dir_all(&socket_dir).await?;
        debug!(agent_id = %id, socket_dir = %socket_dir.display(), "created socket directory");

        let mut containers_client = ContainersClient::new(self.channel.clone());

        let spec = self.build_agent_spec(&socket_path)?;
        let container = Container {
            id: id.clone(),
            image: self.image.clone(),
            runtime: Some(Runtime {
                name: RUNTIME.to_owned(),
                options: None,
            }),
            spec: Some(spec),
            labels: [("judge-core.agent".to_string(), "true".to_string())].into_iter().collect(),
            ..Default::default()
        };

        let req = CreateContainerRequest { container: Some(container) };
        let req = with_namespace!(req, NAMESPACE);

        containers_client.create(req).await?;
        info!(agent_id = %id, "container created");

        let mut tasks_client = TasksClient::new(self.channel.clone());

        let req = CreateTaskRequest {
            container_id: id.clone(),
            ..Default::default()
        };
        let req = with_namespace!(req, NAMESPACE);

        tasks_client.create(req).await?;
        info!(agent_id = %id, "task created");

        let req = StartRequest {
            container_id: id.clone(),
            ..Default::default()
        };
        let req = with_namespace!(req, NAMESPACE);

        tasks_client.start(req).await?;
        info!(agent_id = %id, "task started");

        self.wait_for_socket(&socket_path).await?;
        info!(agent_id = %id, socket = %socket_path.display(), "agent ready");

        Ok((id, socket_path))
    }

    /// Destroy an agent container and clean up its resources
    #[tracing::instrument(skip(self))]
    pub async fn destroy(&self, id: &str) -> Result<(), ProvisionError> {
        let mut tasks_client = TasksClient::new(self.channel.clone());

        let req = KillRequest {
            container_id: id.to_string(),
            signal: 9,
            ..Default::default()
        };
        let req = with_namespace!(req, NAMESPACE);

        if let Err(e) = tasks_client.kill(req).await {
            warn!(agent_id = id, error = %e, "failed to kill task (may already be stopped)");
        }

        let req = DeleteTaskRequest { container_id: id.to_string() };
        let req = with_namespace!(req, NAMESPACE);

        if let Err(e) = tasks_client.delete(req).await {
            warn!(agent_id = id, error = %e, "failed to delete task (may already be deleted)");
        }

        let mut containers_client = ContainersClient::new(self.channel.clone());

        let req = DeleteContainerRequest { id: id.to_string() };
        let req = with_namespace!(req, NAMESPACE);

        containers_client.delete(req).await?;

        let socket_dir = self.socket_base.join(id);
        if let Err(e) = fs::remove_dir_all(&socket_dir).await {
            warn!(agent_id = id, dir = %socket_dir.display(), error = %e, "failed to remove socket directory");
        }

        info!(agent_id = id, "agent destroyed");
        Ok(())
    }

    /// List all agent container id
    #[tracing::instrument(skip(self))]
    pub async fn list(&self) -> Result<Vec<String>, ProvisionError> {
        let mut containers_client = ContainersClient::new(self.channel.clone());

        let req = ListContainersRequest {
            filters: vec![format!("labels.\"judge-core.agent\",true")],
        };
        let req = with_namespace!(req, NAMESPACE);

        let resp = containers_client.list(req).await?;
        let ids = resp.into_inner().containers.into_iter().map(|c| c.id).collect();

        Ok(ids)
    }

    fn build_agent_spec(&self, socket_path: &Path) -> Result<Any, ProvisionError> {
        use oci_spec::runtime::{LinuxBuilder, LinuxNamespaceBuilder, LinuxNamespaceType, MountBuilder, ProcessBuilder, RootBuilder, SpecBuilder};

        let socket_dir = socket_path.parent().unwrap().to_str().expect("socket path is not valid utf-8");

        let spec = SpecBuilder::default()
            .root(RootBuilder::default().path("rootfs").build()?)
            .process(ProcessBuilder::default().args(vec!["/usr/local/bin/agent".to_string()]).cwd("/").build()?)
            .mounts(vec![
                MountBuilder::default()
                    .destination("/run/judge-core")
                    .source(socket_dir)
                    .typ("bind")
                    .options(vec!["rbind".to_string(), "rw".to_string()])
                    .build()?,
            ])
            .linux(
                LinuxBuilder::default()
                    .namespaces(vec![
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Pid).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Ipc).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Uts).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Mount).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Network).build()?,
                    ])
                    .build()?,
            )
            .build()?;

        let json = serde_json::to_vec(&spec)?;
        Ok(Any {
            type_url: "types.containerd.io/opencontainers/runtime-spec/1/Spec".to_string(),
            value: json,
        })
    }

    /// Wait for mounted agent socket
    async fn wait_for_socket(&self, socket_path: &Path) -> Result<(), ProvisionError> {
        let start = Instant::now();
        let timeout = Duration::from_secs(SOCKET_WAIT_TIMEOUT_SECS);

        while start.elapsed() < timeout {
            if fs::try_exists(socket_path).await? {
                return Ok(());
            }
            sleep(Duration::from_millis(SOCKET_WAIT_INTERVAL_MS)).await;
        }

        Err(ProvisionError::SocketTimeout(SOCKET_WAIT_TIMEOUT_SECS))
    }
}
