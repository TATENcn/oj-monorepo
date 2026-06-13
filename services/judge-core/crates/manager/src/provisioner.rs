use std::path::{Path, PathBuf};
use std::time::Instant;

use containerd_client::tonic::Request;
use containerd_client::{
    services::v1::{
        Container, CreateContainerRequest, CreateTaskRequest, DeleteContainerRequest, DeleteTaskRequest, GetImageRequest, KillRequest, ListContainersRequest,
        ReadContentRequest, StartRequest, WaitRequest,
        container::Runtime,
        containers_client::ContainersClient,
        content_client::ContentClient,
        images_client::ImagesClient,
        snapshots::{PrepareSnapshotRequest, RemoveSnapshotRequest, snapshots_client::SnapshotsClient},
        tasks_client::TasksClient,
    },
    tonic::transport::Channel,
    with_namespace,
};
use oci_spec::image::{ImageConfiguration, ImageIndex, ImageManifest};
use prost_types::Any;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use tokio::time::{Duration, sleep, timeout};
use tokio::{fs, io};
use tracing::{debug, info, warn};
use uuid::Uuid;

const NAMESPACE: &str = "judge-core";
const AGENT_LABEL_KEY: &str = "judge-core.agent";
const AGENT_LABEL_VALUE: &str = "true";
const SOCKET_WAIT_TIMEOUT_SECS: u64 = 30;
const SOCKET_WAIT_INTERVAL_MS: u64 = 100;

const RUNTIME: &str = "io.containerd.runc.v2";

pub struct ContainerdProvisioner {
    channel: Channel,
    image: String,
    socket_base: PathBuf,
    chain_id: OnceCell<String>,
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
            chain_id: OnceCell::new(),
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

        let parent_digest = self.resolve_chain_id().await?;
        debug!(agent_id = %id, image = %self.image, parent = %parent_digest, "resolved image chain id");

        let snapshotter = "overlayfs";
        let mut snapshots_client = SnapshotsClient::new(self.channel.clone());
        let prepare_req = with_namespace!(
            PrepareSnapshotRequest {
                snapshotter: snapshotter.to_string(),
                key: id.clone(),
                parent: parent_digest,
                labels: Default::default(),
            },
            NAMESPACE
        );
        snapshots_client.prepare(prepare_req).await?;
        info!(agent_id = %id, snapshotter = %snapshotter, "snapshot prepared");

        let mounts_req = with_namespace!(
            containerd_client::services::v1::snapshots::MountsRequest {
                snapshotter: snapshotter.to_string(),
                key: id.clone(),
            },
            NAMESPACE
        );
        let mounts_resp = snapshots_client.mounts(mounts_req).await?;
        let rootfs_mounts = mounts_resp.into_inner().mounts;
        debug!(agent_id = %id, mounts_count = rootfs_mounts.len(), "got rootfs mounts");

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
            snapshotter: snapshotter.to_string(),
            snapshot_key: id.clone(),
            labels: [(AGENT_LABEL_KEY.to_string(), AGENT_LABEL_VALUE.to_string())].into_iter().collect(),
            ..Default::default()
        };

        let req = CreateContainerRequest { container: Some(container) };
        let req = with_namespace!(req, NAMESPACE);

        containers_client.create(req).await?;
        info!(agent_id = %id, "container created");

        let mut tasks_client = TasksClient::new(self.channel.clone());

        let req = CreateTaskRequest {
            container_id: id.clone(),
            rootfs: rootfs_mounts,
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
            signal: libc::SIGINT as u32,
            ..Default::default()
        };
        let req = with_namespace!(req, NAMESPACE);

        if let Err(e) = tasks_client.kill(req).await {
            debug!(agent_id = id, error = %e, "failed to kill task (may already be stopped)");
        }

        // wait for the task to exit before deleting it
        let wait_req = WaitRequest {
            container_id: id.to_string(),
            exec_id: String::new(),
        };
        let wait_req = with_namespace!(wait_req, NAMESPACE);
        match timeout(Duration::from_secs(90), tasks_client.wait(wait_req)).await {
            Ok(Ok(_)) => debug!(agent_id = id, "task exited"),
            Ok(Err(e)) => warn!(agent_id = id, error = %e, "wait for task exit returned error"),
            Err(_) => warn!(agent_id = id, "timeout waiting for task exit, attempting delete anyway"),
        }

        let req = DeleteTaskRequest { container_id: id.to_string() };
        let req = with_namespace!(req, NAMESPACE);

        if let Err(e) = tasks_client.delete(req).await {
            warn!(agent_id = id, error = %e, "failed to delete task, container will be retained for retry");
            return Err(ProvisionError::Rpc(e));
        }

        let mut containers_client = ContainersClient::new(self.channel.clone());

        let req = DeleteContainerRequest { id: id.to_string() };
        let req = with_namespace!(req, NAMESPACE);

        containers_client.delete(req).await?;

        let mut snapshots_client = SnapshotsClient::new(self.channel.clone());
        let snapshotter = "overlayfs";
        let remove_req = with_namespace!(
            RemoveSnapshotRequest {
                snapshotter: snapshotter.to_string(),
                key: id.to_string(),
            },
            NAMESPACE
        );
        if let Err(e) = snapshots_client.remove(remove_req).await {
            debug!(agent_id = id, error = %e, "failed to remove snapshot (may already be removed)");
        }

        let socket_dir = self.socket_base.join(id);
        if let Err(e) = fs::remove_dir_all(&socket_dir).await {
            debug!(agent_id = id, dir = %socket_dir.display(), error = %e, "failed to remove socket directory");
        }

        info!(agent_id = id, "agent destroyed");
        Ok(())
    }

    /// List all agent container id
    #[tracing::instrument(skip(self))]
    pub async fn list(&self) -> Result<Vec<String>, ProvisionError> {
        let mut containers_client = ContainersClient::new(self.channel.clone());

        let req = ListContainersRequest {
            filters: vec![format!("labels.\"{}\"=={}", AGENT_LABEL_KEY, AGENT_LABEL_VALUE)],
        };
        let req = with_namespace!(req, NAMESPACE);

        let resp = containers_client.list(req).await?;
        let ids = resp.into_inner().containers.into_iter().map(|c| c.id).collect();

        Ok(ids)
    }

    /// Resolve image reference to its rootfs chain ID for snapshot parent
    #[tracing::instrument(skip(self))]
    async fn resolve_chain_id(&self) -> Result<String, ProvisionError> {
        self.chain_id.get_or_try_init(|| self.compute_chain_id_from_image()).await.cloned()
    }

    #[tracing::instrument(skip(self))]
    async fn compute_chain_id_from_image(&self) -> Result<String, ProvisionError> {
        use oci_spec::image::{Arch, Os};

        let mut images_client = ImagesClient::new(self.channel.clone());
        let get_image_req = with_namespace!(GetImageRequest { name: self.image.clone() }, NAMESPACE);
        let image_resp = images_client.get(get_image_req).await?;
        let image = image_resp
            .into_inner()
            .image
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "image not found"))?;

        let target = image
            .target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "image has no target descriptor"))?;
        let blob = self.read_content_blob(&target.digest, target.size).await?;

        let media_type = target.media_type.as_str();
        let chain_id = if media_type == "application/vnd.oci.image.index.v1+json" || media_type == "application/vnd.docker.distribution.manifest.list.v2+json" {
            let index = ImageIndex::from_reader(&*blob)?;
            let manifest_desc = index
                .manifests()
                .iter()
                .find(|m| {
                    m.platform()
                        .as_ref()
                        .map(|p| *p.os() == Os::Linux && *p.architecture() == Arch::Amd64)
                        .unwrap_or(false)
                })
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no manifest for linux/amd64 platform"))?;
            let manifest_blob = self.read_content_blob(manifest_desc.digest().as_ref(), manifest_desc.size() as i64).await?;
            let manifest = ImageManifest::from_reader(&*manifest_blob)?;
            let config_desc = manifest.config();
            let config_blob = self.read_content_blob(config_desc.digest().as_ref(), config_desc.size() as i64).await?;
            let config = ImageConfiguration::from_reader(&*config_blob)?;
            Self::compute_chain_id(config.rootfs().diff_ids())
        } else if media_type == "application/vnd.oci.image.manifest.v1+json" || media_type == "application/vnd.docker.distribution.manifest.v2+json" {
            let manifest = ImageManifest::from_reader(&*blob)?;
            let config_desc = manifest.config();
            let config_blob = self.read_content_blob(config_desc.digest().as_ref(), config_desc.size() as i64).await?;
            let config = ImageConfiguration::from_reader(&*config_blob)?;
            Self::compute_chain_id(config.rootfs().diff_ids())
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unsupported image media type: {}", media_type)).into());
        };

        info!(chain_id = %chain_id, "resolved and cached chain id");
        Ok(chain_id)
    }

    /// Read a content blob from the content store
    async fn read_content_blob(&self, digest: &str, size: i64) -> Result<Vec<u8>, ProvisionError> {
        let mut content_client = ContentClient::new(self.channel.clone());
        let req = with_namespace!(
            ReadContentRequest {
                digest: digest.to_string(),
                offset: 0,
                size,
            },
            NAMESPACE
        );
        let mut stream = content_client.read(req).await?.into_inner();
        let mut data = Vec::new();
        while let Some(chunk) = stream.message().await? {
            data.extend_from_slice(&chunk.data);
        }
        Ok(data)
    }

    /// Compute containerd chain ID from diff IDs
    fn compute_chain_id(diff_ids: &[String]) -> String {
        if diff_ids.is_empty() {
            return String::new();
        }
        let mut chain = diff_ids[0].clone();
        for diff_id in &diff_ids[1..] {
            let mut hasher = Sha256::new();
            hasher.update(format!("{} {}", chain, diff_id).as_bytes());
            let hash = hasher.finalize();
            chain = format!("sha256:{}", hash.iter().map(|b| format!("{:02x}", b)).collect::<String>());
        }
        chain
    }

    fn build_agent_spec(&self, socket_path: &Path) -> Result<Any, ProvisionError> {
        use oci_spec::runtime::{
            LinuxBuilder, LinuxDeviceBuilder, LinuxDeviceCgroupBuilder, LinuxDeviceType, LinuxIdMapping, LinuxNamespaceBuilder, LinuxNamespaceType,
            LinuxResourcesBuilder, MountBuilder, ProcessBuilder, RootBuilder, SpecBuilder,
        };

        let socket_dir = socket_path.parent().unwrap().to_str().expect("socket path is not valid utf-8");

        let spec = SpecBuilder::default()
            .root(RootBuilder::default().path("rootfs").build()?)
            .process(ProcessBuilder::default().args(vec!["/usr/local/bin/agent".to_string()]).cwd("/").build()?)
            .mounts(vec![
                MountBuilder::default().destination("/proc").source("proc").typ("proc").build()?,
                MountBuilder::default()
                    .destination("/dev")
                    .source("tmpfs")
                    .typ("tmpfs")
                    .options(vec![
                        "nosuid".to_string(),
                        "strictatime".to_string(),
                        "mode=755".to_string(),
                        "size=65536k".to_string(),
                    ])
                    .build()?,
                MountBuilder::default()
                    .destination("/dev/pts")
                    .source("devpts")
                    .typ("devpts")
                    .options(vec![
                        "nosuid".to_string(),
                        "noexec".to_string(),
                        "newinstance".to_string(),
                        "ptmxmode=0666".to_string(),
                        "mode=0620".to_string(),
                    ])
                    .build()?,
                MountBuilder::default()
                    .destination("/run/judge-core")
                    .source(socket_dir)
                    .typ("bind")
                    .options(vec!["rbind".to_string(), "rw".to_string()])
                    .build()?,
                MountBuilder::default()
                    .destination("/work")
                    .source("tmpfs")
                    .typ("tmpfs")
                    .options(vec!["nosuid".to_string(), "nodev".to_string(), "mode=755".to_string(), "size=256m".to_string()])
                    .build()?,
                MountBuilder::default()
                    .destination("/tmp")
                    .source("tmpfs")
                    .typ("tmpfs")
                    .options(vec!["nosuid".to_string(), "nodev".to_string(), "mode=1777".to_string(), "size=64m".to_string()])
                    .build()?,
                MountBuilder::default()
                    .destination("/sys/fs/cgroup")
                    .source("cgroup2")
                    .typ("cgroup2")
                    .options(vec!["nosuid".to_string(), "noexec".to_string(), "nodev".to_string(), "rw".to_string()])
                    .build()?,
            ])
            .linux(
                LinuxBuilder::default()
                    .uid_mappings(vec![serde_json::from_value::<LinuxIdMapping>(
                        serde_json::json!({"containerID": 0, "hostID": 0, "size": 65536}),
                    )?])
                    .gid_mappings(vec![serde_json::from_value::<LinuxIdMapping>(
                        serde_json::json!({"containerID": 0, "hostID": 0, "size": 65536}),
                    )?])
                    .namespaces(vec![
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Pid).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Ipc).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Uts).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Mount).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Network).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::User).build()?,
                        LinuxNamespaceBuilder::default().typ(LinuxNamespaceType::Cgroup).build()?,
                    ])
                    .devices(vec![
                        LinuxDeviceBuilder::default()
                            .path("/dev/null")
                            .typ(LinuxDeviceType::C)
                            .major(1)
                            .minor(3)
                            .file_mode(0o666_u32)
                            .build()?,
                        LinuxDeviceBuilder::default()
                            .path("/dev/zero")
                            .typ(LinuxDeviceType::C)
                            .major(1)
                            .minor(5)
                            .file_mode(0o666_u32)
                            .build()?,
                        LinuxDeviceBuilder::default()
                            .path("/dev/random")
                            .typ(LinuxDeviceType::C)
                            .major(1)
                            .minor(8)
                            .file_mode(0o666_u32)
                            .build()?,
                        LinuxDeviceBuilder::default()
                            .path("/dev/urandom")
                            .typ(LinuxDeviceType::C)
                            .major(1)
                            .minor(9)
                            .file_mode(0o666_u32)
                            .build()?,
                        LinuxDeviceBuilder::default()
                            .path("/dev/tty")
                            .typ(LinuxDeviceType::C)
                            .major(5)
                            .minor(0)
                            .file_mode(0o666_u32)
                            .build()?,
                    ])
                    .resources(
                        LinuxResourcesBuilder::default()
                            .devices(vec![
                                // deny all by default
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(false)
                                    .typ(LinuxDeviceType::A)
                                    .access("rwm".to_string())
                                    .build()?,
                                // allow null
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(true)
                                    .typ(LinuxDeviceType::C)
                                    .major(1)
                                    .minor(3)
                                    .access("rwm".to_string())
                                    .build()?,
                                // allow zero
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(true)
                                    .typ(LinuxDeviceType::C)
                                    .major(1)
                                    .minor(5)
                                    .access("rwm".to_string())
                                    .build()?,
                                // allow random
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(true)
                                    .typ(LinuxDeviceType::C)
                                    .major(1)
                                    .minor(8)
                                    .access("rwm".to_string())
                                    .build()?,
                                // allow urandom
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(true)
                                    .typ(LinuxDeviceType::C)
                                    .major(1)
                                    .minor(9)
                                    .access("rwm".to_string())
                                    .build()?,
                                // allow tty
                                LinuxDeviceCgroupBuilder::default()
                                    .allow(true)
                                    .typ(LinuxDeviceType::C)
                                    .major(5)
                                    .minor(0)
                                    .access("rwm".to_string())
                                    .build()?,
                            ])
                            .build()?,
                    )
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
