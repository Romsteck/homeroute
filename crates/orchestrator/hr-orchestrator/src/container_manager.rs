//! Container V2 manager: lifecycle orchestration for systemd-nspawn containers.
//!
//! This is the canonical implementation. hr-api delegates to hr-orchestrator
//! via IPC for all container operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use hr_common::config::EnvConfig;
use hr_common::events::{AgentStatusEvent, EventBus, MigrationPhase, MigrationProgressEvent};
use hr_container::NspawnClient;
use hr_git::GitService;
use hr_registry::protocol::HostRegistryMessage;
use hr_registry::types::{AgentStatus, CreateApplicationRequest, Environment, UpdateApplicationRequest};
use hr_registry::AgentRegistry;

// ── Types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContainerV2Status {
    Deploying,
    Running,
    Stopped,
    Error,
    Migrating,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ContainerV2Config {
    #[serde(default = "default_storage_path")]
    pub container_storage_path: String,
    #[serde(default)]
    pub lan_interface: Option<String>,
}

fn default_storage_path() -> String {
    "/var/lib/machines".to_string()
}

#[derive(Serialize, Deserialize)]
pub struct ContainerV2State {
    #[serde(default)]
    pub config: ContainerV2Config,
    #[serde(default)]
    pub containers: Vec<ContainerV2Record>,
}

impl Default for ContainerV2State {
    fn default() -> Self {
        Self {
            config: ContainerV2Config::default(),
            containers: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StorageVolume {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub mount_point: String,
    #[serde(default)]
    pub read_only: bool,
    pub zfs_dataset: Option<String>,
    pub zfs_quota: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ContainerV2Record {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub container_name: String,
    pub host_id: String,
    #[serde(default)]
    pub environment: hr_registry::types::Environment,
    #[serde(default)]
    pub stack: hr_registry::types::AppStack,
    pub status: ContainerV2Status,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub volumes: Vec<StorageVolume>,
}

#[derive(Deserialize)]
pub struct CreateContainerRequest {
    pub name: String,
    pub slug: String,
    pub frontend: hr_registry::types::FrontendEndpoint,
    #[serde(default = "default_true")]
    pub code_server_enabled: bool,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub stack: hr_registry::types::AppStack,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct RenameContainerRequest {
    pub new_slug: String,
    #[serde(default)]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameState {
    pub rename_id: String,
    pub app_ids: Vec<String>,
    pub old_slug: String,
    pub new_slug: String,
    pub phase: RenamePhase,
    pub started_at: DateTime<Utc>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RenamePhase {
    Validating,
    RequestingCert,
    CreatingDns,
    StoppingContainers,
    RenamingFilesystem,
    UpdatingAgentConfig,
    UpdatingRegistry,
    StartingContainers,
    WaitingForAgent,
    CleaningUp,
    Complete,
    Failed,
    RollingBack,
}

#[derive(Deserialize)]
pub struct MigrateContainerRequest {
    pub target_host_id: String,
}

#[derive(Deserialize)]
pub struct UpdateContainerRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub frontend: Option<hr_registry::types::FrontendEndpoint>,
    #[serde(default)]
    pub code_server_enabled: Option<bool>,
    #[serde(default)]
    pub stack: Option<hr_registry::types::AppStack>,
}

/// In-memory state of an active migration.
#[derive(Debug, Serialize)]
pub struct MigrationState {
    pub app_id: String,
    pub transfer_id: String,
    pub source_host_id: String,
    pub target_host_id: String,
    pub phase: MigrationPhase,
    pub progress_pct: u8,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub started_at: DateTime<Utc>,
    pub error: Option<String>,
    #[serde(skip)]
    pub cancelled: Arc<AtomicBool>,
}

// ── Migration helpers (module-level) ─────────────────────────────

/// Update migration state and emit progress event.
async fn update_migration_phase(
    migrations: &Arc<RwLock<HashMap<String, MigrationState>>>,
    events: &Arc<EventBus>,
    app_id: &str,
    transfer_id: &str,
    phase: MigrationPhase,
    pct: u8,
    transferred: u64,
    total: u64,
    error: Option<String>,
) {
    {
        let mut m = migrations.write().await;
        if let Some(state) = m.get_mut(transfer_id) {
            state.phase = phase.clone();
            state.progress_pct = pct;
            state.bytes_transferred = transferred;
            state.total_bytes = total;
            state.error = error.clone();
        }
    }
    let _ = events.migration_progress.send(MigrationProgressEvent {
        app_id: app_id.to_string(),
        transfer_id: transfer_id.to_string(),
        phase,
        progress_pct: pct,
        bytes_transferred: transferred,
        total_bytes: total,
        error,
    });
}

/// Stream data from an AsyncRead source to a remote host-agent in 512KB binary chunks.
/// Returns total bytes transferred and final sequence number.
async fn stream_to_remote(
    registry: &Arc<AgentRegistry>,
    target_host_id: &str,
    transfer_id: &str,
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    total_bytes: u64,
    cancelled: &Arc<AtomicBool>,
    migrations: &Arc<RwLock<HashMap<String, MigrationState>>>,
    events: &Arc<EventBus>,
    app_id: &str,
    pct_start: u8,
    pct_end: u8,
    phase: MigrationPhase,
) -> Result<(u64, u32), String> {
    let mut buf = vec![0u8; 524288]; // 512KB
    let mut transferred: u64 = 0;
    let mut sequence: u32 = 0;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = registry.send_host_command(
                target_host_id,
                HostRegistryMessage::CancelTransfer { transfer_id: transfer_id.to_string() },
            ).await;
            return Err("Migration cancelled by user".to_string());
        }

        let n = match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("Read error: {e}")),
        };

        let chunk = &buf[..n];
        let checksum = xxhash_rust::xxh32::xxh32(chunk, 0);

        if let Err(e) = registry.send_host_command(
            target_host_id,
            HostRegistryMessage::ReceiveChunkBinary {
                transfer_id: transfer_id.to_string(),
                sequence,
                size: n as u32,
                checksum,
            },
        ).await {
            return Err(format!("Send chunk metadata failed: {e}"));
        }

        if let Err(e) = registry.send_host_binary(
            target_host_id,
            chunk.to_vec(),
        ).await {
            return Err(format!("Send binary chunk failed: {e}"));
        }

        transferred += n as u64;
        sequence += 1;
        let pct = (pct_start as u64 + (transferred * (pct_end - pct_start) as u64 / total_bytes.max(1))) as u8;

        if sequence % 4 == 0 || transferred >= total_bytes {
            update_migration_phase(migrations, events, app_id, transfer_id, phase.clone(), pct.min(pct_end), transferred, total_bytes, None).await;
        } else {
            let mut m = migrations.write().await;
            if let Some(state) = m.get_mut(transfer_id) {
                state.progress_pct = pct.min(pct_end);
                state.bytes_transferred = transferred;
            }
        }
    }

    Ok((transferred, sequence))
}

// ── ContainerManager ─────────────────────────────────────────────

pub struct ContainerManager {
    state: Arc<RwLock<ContainerV2State>>,
    state_path: PathBuf,
    pub env: Arc<EnvConfig>,
    pub events: Arc<EventBus>,
    pub registry: Arc<AgentRegistry>,
    pub git: Option<Arc<GitService>>,
}

impl ContainerManager {
    /// Load or create the container V2 state from disk.
    pub fn new(
        state_path: PathBuf,
        env: Arc<EnvConfig>,
        events: Arc<EventBus>,
        registry: Arc<AgentRegistry>,
        git: Option<Arc<GitService>>,
    ) -> Self {
        let state = match std::fs::read_to_string(&state_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!("Failed to parse containers-v2 state, starting fresh: {e}");
                ContainerV2State::default()
            }),
            Err(_) => ContainerV2State::default(),
        };

        info!(
            containers = state.containers.len(),
            "Loaded containers V2 state"
        );

        Self {
            state: Arc::new(RwLock::new(state)),
            state_path,
            env,
            events,
            registry,
            git,
        }
    }

    /// Restart local containers that were Running before a reboot.
    pub async fn restore_local_containers(&self) {
        let containers: Vec<ContainerV2Record> = {
            let state = self.state.read().await;
            state
                .containers
                .iter()
                .filter(|c| c.host_id == "local" && c.status == ContainerV2Status::Running)
                .cloned()
                .collect()
        };

        if containers.is_empty() {
            info!("No local containers to restore");
            return;
        }

        info!(count = containers.len(), "Restoring local containers after boot");
        for c in &containers {
            match NspawnClient::start_container(&c.container_name).await {
                Ok(_) => info!(container = %c.container_name, "Restored container"),
                Err(e) => error!(container = %c.container_name, "Failed to restore container: {e}"),
            }
        }
    }

    /// Restart containers on a remote host that were Running before it disconnected.
    pub async fn restore_host_containers(&self, host_id: &str) {
        let containers: Vec<ContainerV2Record> = {
            let state = self.state.read().await;
            state
                .containers
                .iter()
                .filter(|c| c.host_id == host_id && c.status == ContainerV2Status::Running)
                .cloned()
                .collect()
        };

        if containers.is_empty() {
            return;
        }

        info!(host_id, count = containers.len(), "Restoring containers on reconnected host");
        for c in &containers {
            if let Err(e) = self.registry.send_host_command(
                host_id,
                HostRegistryMessage::StartContainer {
                    container_name: c.container_name.clone(),
                },
            ).await {
                error!(container = %c.container_name, host_id, "Failed to restore container on host: {e}");
            } else {
                info!(container = %c.container_name, host_id, "Sent start command to host");
            }
        }
    }

    /// Persist state to disk (atomic write).
    async fn save_state(&self) -> Result<(), String> {
        let state = self.state.read().await;
        let json = serde_json::to_string_pretty(&*state).map_err(|e| e.to_string())?;
        let tmp = self.state_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &json)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, &self.state_path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Volume management ─────────────────────────────────────────

    /// List volumes for a container.
    pub async fn list_volumes(&self, container_id: &str) -> Result<Vec<StorageVolume>, String> {
        let state = self.state.read().await;
        match state.containers.iter().find(|c| c.id == container_id) {
            Some(c) => Ok(c.volumes.clone()),
            None => Err("Container not found".to_string()),
        }
    }

    /// Attach a new volume to a container. Creates source directory, optionally creates ZFS dataset,
    /// saves state, regenerates .nspawn unit, and restarts the container.
    pub async fn attach_volume(
        &self,
        container_id: &str,
        name: String,
        source_path: String,
        mount_point: String,
        read_only: bool,
        zfs_dataset: Option<String>,
        zfs_quota: Option<u64>,
    ) -> Result<StorageVolume, String> {
        // Verify container exists and is local
        let (container_name, host_id) = {
            let state = self.state.read().await;
            match state.containers.iter().find(|c| c.id == container_id) {
                Some(c) => (c.container_name.clone(), c.host_id.clone()),
                None => return Err("Container not found".to_string()),
            }
        };

        if host_id != "local" {
            return Err("Volume management is only supported for local containers".to_string());
        }

        // Prevent duplicate mount points
        {
            let state = self.state.read().await;
            if let Some(c) = state.containers.iter().find(|c| c.id == container_id) {
                if c.volumes.iter().any(|v| v.mount_point == mount_point) {
                    return Err(format!("A volume is already mounted at {mount_point}"));
                }
            }
        }

        // Create source directory
        tokio::fs::create_dir_all(&source_path)
            .await
            .map_err(|e| format!("Failed to create source directory {source_path}: {e}"))?;

        // Optionally create ZFS dataset (best-effort)
        if let Some(ref dataset) = zfs_dataset {
            let mut args = vec![
                "create".to_string(),
                "-o".to_string(), "compression=lz4".to_string(),
                "-o".to_string(), "atime=off".to_string(),
            ];
            if let Some(quota) = zfs_quota {
                args.push("-o".to_string());
                args.push(format!("quota={quota}"));
            }
            args.push(dataset.clone());

            let output = tokio::process::Command::new("sudo")
                .arg("zfs")
                .args(&args)
                .output()
                .await;

            match output {
                Ok(out) if out.status.success() => {
                    info!(dataset, "ZFS dataset created for volume");
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if stderr.contains("dataset already exists") {
                        info!(dataset, "ZFS dataset already exists, reusing");
                    } else {
                        warn!(dataset, stderr = %stderr, "Failed to create ZFS dataset (continuing without ZFS)");
                    }
                }
                Err(e) => {
                    warn!(dataset, error = %e, "Failed to run zfs create (continuing without ZFS)");
                }
            }
        }

        // Generate volume ID and create the volume struct
        let volume = StorageVolume {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            source_path,
            mount_point,
            read_only,
            zfs_dataset,
            zfs_quota,
        };

        // Add volume to container record
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == container_id) {
                c.volumes.push(volume.clone());
            }
        }
        self.save_state().await?;

        // Regenerate .nspawn and restart
        self.regenerate_nspawn_and_restart(container_id, &container_name).await?;

        info!(container_id, volume_id = %volume.id, "Volume attached");
        Ok(volume)
    }

    /// Update an existing volume on a container (name, source_path, mount_point, read_only).
    /// Saves state, regenerates .nspawn unit, and restarts the container.
    pub async fn update_volume(
        &self,
        container_id: &str,
        volume_id: &str,
        updates: serde_json::Value,
    ) -> Result<StorageVolume, String> {
        let container_name = {
            let mut state = self.state.write().await;
            let container = state.containers.iter_mut().find(|c| c.id == container_id)
                .ok_or_else(|| "Container not found".to_string())?;

            if container.host_id != "local" {
                return Err("Volume management is only supported for local containers".to_string());
            }

            let volume = container.volumes.iter_mut().find(|v| v.id == volume_id)
                .ok_or_else(|| "Volume not found".to_string())?;

            if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
                volume.name = name.to_string();
            }
            if let Some(source_path) = updates.get("source_path").and_then(|v| v.as_str()) {
                volume.source_path = source_path.to_string();
            }
            if let Some(mount_point) = updates.get("mount_point").and_then(|v| v.as_str()) {
                volume.mount_point = mount_point.to_string();
            }
            if let Some(read_only) = updates.get("read_only").and_then(|v| v.as_bool()) {
                volume.read_only = read_only;
            }

            container.container_name.clone()
        };

        self.save_state().await?;

        // Get updated volume to return
        let volume = {
            let state = self.state.read().await;
            let container = state.containers.iter().find(|c| c.id == container_id).unwrap();
            container.volumes.iter().find(|v| v.id == volume_id).cloned()
                .ok_or_else(|| "Volume not found after update".to_string())?
        };

        // Regenerate .nspawn and restart
        self.regenerate_nspawn_and_restart(container_id, &container_name).await?;

        info!(container_id, volume_id, "Volume updated");
        Ok(volume)
    }

    /// Detach a volume from a container. Removes from state, regenerates .nspawn, restarts.
    /// Does NOT delete the source data directory.
    pub async fn detach_volume(
        &self,
        container_id: &str,
        volume_id: &str,
    ) -> Result<(), String> {
        let container_name = {
            let mut state = self.state.write().await;
            let container = state.containers.iter_mut().find(|c| c.id == container_id)
                .ok_or_else(|| "Container not found".to_string())?;

            if container.host_id != "local" {
                return Err("Volume management is only supported for local containers".to_string());
            }

            let before_len = container.volumes.len();
            container.volumes.retain(|v| v.id != volume_id);
            if container.volumes.len() == before_len {
                return Err("Volume not found".to_string());
            }

            container.container_name.clone()
        };

        self.save_state().await?;

        // Regenerate .nspawn and restart
        self.regenerate_nspawn_and_restart(container_id, &container_name).await?;

        info!(container_id, volume_id, "Volume detached (data preserved)");
        Ok(())
    }

    /// Helper: regenerate the .nspawn unit file for a container and restart it.
    /// Reads the existing network mode from the .nspawn file instead of recomputing,
    /// so volume operations don't fail on non-bridge network configs.
    async fn regenerate_nspawn_and_restart(
        &self,
        container_id: &str,
        container_name: &str,
    ) -> Result<(), String> {
        let (host_id, volumes) = {
            let state = self.state.read().await;
            let c = state.containers.iter().find(|c| c.id == container_id)
                .ok_or_else(|| "Container not found".to_string())?;
            (c.host_id.clone(), c.volumes.clone())
        };

        let storage_path = self.resolve_storage_path(&host_id).await;
        let storage = Path::new(&storage_path);

        // Read existing network mode from the .nspawn file to avoid re-resolving
        let nspawn_path = format!("/etc/systemd/nspawn/{container_name}.nspawn");
        let network_mode = match tokio::fs::read_to_string(&nspawn_path).await {
            Ok(content) => {
                // Parse Bridge= from [Network] section
                let mut mode = None;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(val) = trimmed.strip_prefix("Bridge=") {
                        mode = Some(format!("bridge:{}", val.trim()));
                    }
                }
                mode.unwrap_or_else(|| "bridge:br0".to_string())
            }
            Err(_) => self.resolve_network_mode(&host_id).await?,
        };

        let has_workspace = storage
            .join(format!("{container_name}-workspace"))
            .exists();

        let vol_binds: Vec<(String, String, bool)> = volumes
            .iter()
            .map(|v| (v.source_path.clone(), v.mount_point.clone(), v.read_only))
            .collect();

        NspawnClient::write_nspawn_unit(container_name, storage, &network_mode, has_workspace, &vol_binds)
            .await
            .map_err(|e| format!("Failed to write nspawn unit: {e}"))?;

        // Restart container: stop then start
        let _ = NspawnClient::stop_container(container_name).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        NspawnClient::start_container(container_name)
            .await
            .map_err(|e| format!("Failed to start container after volume change: {e}"))?;

        Ok(())
    }

    // ── CRUD ─────────────────────────────────────────────────────

    /// Create a new nspawn container: register in AgentRegistry (headless), create V2 record,
    /// spawn background deploy.
    pub fn create_container(
        self: &Arc<Self>,
        req: CreateContainerRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(ContainerV2Record, String), String>> + Send + '_>> {
        Box::pin(self.create_container_inner(req))
    }

    async fn create_container_inner(
        self: &Arc<Self>,
        mut req: CreateContainerRequest,
    ) -> Result<(ContainerV2Record, String), String> {
        let host_id = req.host_id.clone().unwrap_or_else(|| "local".to_string());

        // Create application in registry (headless -- container deploy is managed separately)
        let create_req = CreateApplicationRequest {
            name: req.name.clone(),
            slug: req.slug.clone(),
            host_id: Some(host_id.clone()),
            frontend: req.frontend.clone(),
            code_server_enabled: req.code_server_enabled,
            stack: req.stack,
        };

        let (app, token) = self
            .registry
            .create_application_headless(create_req)
            .await
            .map_err(|e| format!("Failed to create application record: {e}"))?;

        // Create bare git repo for this application
        if let Some(ref git) = self.git {
            if let Err(e) = git.create_repo(&req.slug).await {
                warn!(slug = %req.slug, "Failed to create bare git repo: {e}");
            }
        }

        // Use the container_name from the registry (single source of truth)
        let container_name = app.container_name.clone();

        let record = ContainerV2Record {
            id: app.id.clone(),
            name: req.name,
            slug: req.slug.clone(),
            container_name: container_name.clone(),
            host_id: host_id.clone(),
            environment: hr_registry::types::Environment::Production,
            stack: req.stack,
            status: ContainerV2Status::Deploying,
            created_at: Utc::now(),
            volumes: Vec::new(),
        };

        // Persist the record
        {
            let mut state = self.state.write().await;
            state.containers.push(record.clone());
        }
        let _ = self.save_state().await;

        // Spawn background deploy
        let mgr = Arc::clone(self);
        let app_id = app.id.clone();
        let slug = req.slug.clone();
        let token_deploy = token.clone();
        let stack = req.stack;
        tokio::spawn(async move {
            mgr.run_nspawn_deploy_prod(&app_id, &slug, &container_name, &host_id, &token_deploy, stack)
                .await;
        });

        Ok((record, token))
    }

    /// Remove a container: stop nspawn, delete rootfs, remove from registry.
    pub async fn remove_container(&self, id: &str) -> Result<bool, String> {
        let record = {
            let state = self.state.read().await;
            state.containers.iter().find(|c| c.id == id).cloned()
        };

        let Some(record) = record else {
            return Ok(false);
        };

        let storage_path = self.resolve_storage_path(&record.host_id).await;

        if record.host_id == "local" {
            let _ = NspawnClient::stop_container(&record.container_name).await;
            let _ =
                NspawnClient::delete_container(&record.container_name, Path::new(&storage_path))
                    .await;
        } else {
            let _ = self
                .registry
                .send_host_command(
                    &record.host_id,
                    HostRegistryMessage::StopContainer {
                        container_name: record.container_name.clone(),
                    },
                )
                .await;
            // TODO: send DeleteNspawnContainer when protocol supports it
        }

        // Delete bare git repo
        if let Some(ref git) = self.git {
            if let Err(e) = git.delete_repo(&record.slug).await {
                warn!(slug = %record.slug, "Failed to delete bare git repo: {e}");
            }
        }

        // Remove from registry (also deletes ACME cert, Cloudflare DNS)
        if let Err(e) = self.registry.remove_application(id).await {
            warn!(id, error = %e, "Failed to remove application from registry");
        }

        // Remove from V2 state
        {
            let mut state = self.state.write().await;
            state.containers.retain(|c| c.id != id);
        }
        let _ = self.save_state().await;

        info!(container = record.container_name, "Container V2 removed");
        Ok(true)
    }

    /// Start a stopped container.
    pub async fn start_container(&self, id: &str) -> Result<bool, String> {
        let record = {
            let state = self.state.read().await;
            state.containers.iter().find(|c| c.id == id).cloned()
        };
        let Some(record) = record else {
            return Ok(false);
        };

        if record.host_id == "local" {
            NspawnClient::start_container(&record.container_name)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            self.registry
                .send_host_command(
                    &record.host_id,
                    HostRegistryMessage::StartContainer {
                        container_name: record.container_name.clone(),
                    },
                )
                .await?;
        }

        // Update status and re-enable in the agent registry so the health
        // watcher will monitor this container again.
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == id) {
                c.status = ContainerV2Status::Running;
            }
        }
        let _ = self.save_state().await;
        let _ = self.registry.set_application_enabled(id, true).await;
        Ok(true)
    }

    /// Stop a running container.
    pub async fn stop_container(&self, id: &str) -> Result<bool, String> {
        let record = {
            let state = self.state.read().await;
            state.containers.iter().find(|c| c.id == id).cloned()
        };
        let Some(record) = record else {
            return Ok(false);
        };

        if record.host_id == "local" {
            NspawnClient::stop_container(&record.container_name)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            self.registry
                .send_host_command(
                    &record.host_id,
                    HostRegistryMessage::StopContainer {
                        container_name: record.container_name.clone(),
                    },
                )
                .await?;
        }

        // Update status and disable in the agent registry so the health
        // watcher won't auto-restart this manually stopped container.
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == id) {
                c.status = ContainerV2Status::Stopped;
            }
        }
        let _ = self.save_state().await;
        let _ = self.registry.set_application_enabled(id, false).await;
        Ok(true)
    }

    /// Update a V2 container's configuration (endpoints, name, code-server).
    pub async fn update_container(&self, id: &str, req: UpdateContainerRequest) -> Result<bool, String> {
        // Check container exists
        let exists = {
            let state = self.state.read().await;
            state.containers.iter().any(|c| c.id == id)
        };
        if !exists {
            return Ok(false);
        }

        // Build UpdateApplicationRequest
        let update_req = UpdateApplicationRequest {
            name: req.name.clone(),
            frontend: req.frontend,
            code_server_enabled: req.code_server_enabled,
            stack: req.stack,
            ..Default::default()
        };

        self.registry
            .update_application(id, update_req)
            .await
            .map_err(|e| e.to_string())?;

        // Update V2 state (name / stack) if provided
        {
            let mut need_save = false;
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == id) {
                if let Some(ref new_name) = req.name {
                    c.name = new_name.clone();
                    need_save = true;
                }
                if let Some(new_stack) = req.stack {
                    c.stack = new_stack;
                    need_save = true;
                }
            }
            drop(state);
            if need_save {
                let _ = self.save_state().await;
            }
        }

        info!(id, "Container V2 updated");
        Ok(true)
    }

    /// List all V2 container records (raw, without enrichment).
    pub async fn list_container_records(&self) -> Vec<ContainerV2Record> {
        let state = self.state.read().await;
        state.containers.clone()
    }

    /// List all V2 containers, enriched with agent status/metrics from registry.
    pub async fn list_containers(&self) -> Vec<serde_json::Value> {
        let state = self.state.read().await;
        let apps = self.registry.list_applications().await;

        let mut result = Vec::new();
        for record in &state.containers {
            let app = apps.iter().find(|a| a.id == record.id);
            let mut entry = serde_json::to_value(record).unwrap_or_default();
            if let Some(app) = app {
                entry["agent_status"] = serde_json::to_value(&app.status).unwrap_or_default();
                entry["ipv4_address"] = serde_json::json!(app.ipv4_address.map(|ip| ip.to_string()));
                entry["agent_version"] = serde_json::json!(app.agent_version);
                entry["last_heartbeat"] = serde_json::json!(app.last_heartbeat);
                if let Some(ref metrics) = app.metrics {
                    entry["metrics"] = serde_json::to_value(metrics).unwrap_or_default();
                }
                entry["frontend"] = serde_json::to_value(&app.frontend).unwrap_or_default();
                entry["code_server_enabled"] = serde_json::json!(app.code_server_enabled);
                entry["environment"] = serde_json::to_value(&app.environment).unwrap_or_default();
            }
            result.push(entry);
        }
        result
    }

    /// Get a single container by ID, enriched with agent status.
    pub async fn get_container(&self, id: &str) -> Option<serde_json::Value> {
        let state = self.state.read().await;
        let record = state.containers.iter().find(|c| c.id == id)?;
        let mut entry = serde_json::to_value(record).unwrap_or_default();

        let app = self.registry.get_application(id).await;
        if let Some(app) = app {
            entry["agent_status"] = serde_json::to_value(&app.status).unwrap_or_default();
            entry["ipv4_address"] = serde_json::json!(app.ipv4_address.map(|ip| ip.to_string()));
            entry["agent_version"] = serde_json::json!(app.agent_version);
            entry["last_heartbeat"] = serde_json::json!(app.last_heartbeat);
            if let Some(ref metrics) = app.metrics {
                entry["metrics"] = serde_json::to_value(metrics).unwrap_or_default();
            }
            entry["frontend"] = serde_json::to_value(&app.frontend).unwrap_or_default();
            entry["code_server_enabled"] = serde_json::json!(app.code_server_enabled);
            entry["environment"] = serde_json::to_value(&app.environment).unwrap_or_default();
        }

        Some(entry)
    }

    // ── Config ───────────────────────────────────────────────────

    pub async fn get_config(&self) -> ContainerV2Config {
        let mut cfg = self.state.read().await.config.clone();
        if cfg.container_storage_path.is_empty() {
            cfg.container_storage_path = default_storage_path();
        }
        cfg
    }

    pub async fn update_config(&self, config: ContainerV2Config) -> Result<(), String> {
        {
            let mut state = self.state.write().await;
            state.config = config;
        }
        self.save_state().await
    }

    // ── Storage path resolution ──────────────────────────────────

    pub async fn resolve_storage_path(&self, host_id: &str) -> String {
        if host_id == "local" {
            let path = self.state
                .read()
                .await
                .config
                .container_storage_path
                .clone();
            if path.is_empty() {
                return default_storage_path();
            }
            return path;
        } else {
            // Try to read from hosts.json
            if let Ok(content) =
                tokio::fs::read_to_string("/data/hosts.json").await
            {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(hosts) = data.get("hosts").and_then(|h| h.as_array()) {
                        if let Some(host) = hosts
                            .iter()
                            .find(|h| h.get("id").and_then(|i| i.as_str()) == Some(host_id))
                        {
                            if let Some(path) = host
                                .get("container_storage_path")
                                .and_then(|p| p.as_str())
                            {
                                return path.to_string();
                            }
                        }
                    }
                }
            }
            default_storage_path()
        }
    }

    pub async fn resolve_network_mode(&self, host_id: &str) -> Result<String, String> {
        if host_id == "local" {
            return Ok("bridge:br0".to_string());
        }
        // Remote hosts: check hosts.json for lan_interface (bridge mode)
        if let Ok(content) = tokio::fs::read_to_string("/data/hosts.json").await {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(hosts) = data.get("hosts").and_then(|h| h.as_array()) {
                    if let Some(host) = hosts.iter().find(|h| h.get("id").and_then(|i| i.as_str()) == Some(host_id)) {
                        if let Some(iface) = host.get("lan_interface").and_then(|v| v.as_str()) {
                            if !iface.is_empty() {
                                return Ok(format!("bridge:{}", iface));
                            }
                        }
                    }
                }
            }
        }
        Err(format!("No lan_interface configured for host '{}'. Please configure a network interface in host settings.", host_id))
    }


    // ── Remote-aware helpers ─────────────────────────────────────

    /// Create an nspawn container (local or remote via host-agent).
    async fn nspawn_create_container(
        &self,
        host_id: &str,
        container_name: &str,
        storage_path: &str,
        network_mode: &str,
        with_workspace: bool,
    ) -> Result<(), String> {
        if host_id == "local" {
            NspawnClient::create_container(
                container_name,
                Path::new(storage_path),
                network_mode,
                with_workspace,
            )
            .await
            .map_err(|e| e.to_string())
        } else {
            // Send create command to remote host-agent
            self.registry
                .send_host_command(
                    host_id,
                    HostRegistryMessage::CreateNspawnContainer {
                        app_id: String::new(),
                        slug: String::new(),
                        container_name: container_name.to_string(),
                        storage_path: storage_path.to_string(),
                        network_mode: network_mode.to_string(),
                        agent_token: String::new(),
                        agent_config: String::new(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;

            // Poll until container is ready (debootstrap can take 2-5 minutes)
            for _i in 0..120 {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Ok((success, _, _)) = self
                    .registry
                    .exec_in_remote_container_with_timeout(
                        host_id,
                        container_name,
                        vec!["echo ready".to_string()],
                        10,
                    )
                    .await
                {
                    if success {
                        return Ok(());
                    }
                }
            }
            Err("Timeout waiting for remote container creation (10 min)".to_string())
        }
    }

    /// Execute a command inside a container (local or remote).
    async fn container_exec(
        &self,
        host_id: &str,
        container_name: &str,
        cmd: &str,
    ) -> Result<String, String> {
        if host_id == "local" {
            NspawnClient::exec(container_name, &[cmd])
                .await
                .map_err(|e| e.to_string())
        } else {
            let (success, stdout, stderr) = self
                .registry
                .exec_in_remote_container_with_timeout(
                    host_id,
                    container_name,
                    vec![cmd.to_string()],
                    300,
                )
                .await
                .map_err(|e| e.to_string())?;
            if success {
                Ok(stdout)
            } else {
                Err(format!("Remote exec failed: {stderr}"))
            }
        }
    }

    /// Execute a command inside a container with retries (local or remote).
    async fn container_exec_retry(
        &self,
        host_id: &str,
        container_name: &str,
        cmd: &str,
        max_retries: u32,
    ) -> Result<String, String> {
        let mut last_err = String::new();
        for attempt in 0..max_retries {
            match self.container_exec(host_id, container_name, cmd).await {
                Ok(out) => return Ok(out),
                Err(e) => {
                    last_err = e;
                    if attempt + 1 < max_retries {
                        warn!(
                            container = container_name,
                            attempt = attempt + 1,
                            max_retries,
                            "Exec failed, retrying: {last_err}"
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Push raw content bytes into a container file via exec.
    /// Uses hex encoding for reliability (no shell escaping issues).
    async fn container_push_content(
        &self,
        host_id: &str,
        container_name: &str,
        content: &[u8],
        dest: &str,
    ) -> Result<(), String> {
        // Use base64 encoding (available in coreutils, unlike xxd)
        let mut b64 = String::new();
        {
            // Manual base64 encoding (no external crate needed)
            const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i < content.len() {
                let b0 = content[i] as u32;
                let b1 = if i + 1 < content.len() { content[i + 1] as u32 } else { 0 };
                let b2 = if i + 2 < content.len() { content[i + 2] as u32 } else { 0 };
                let triple = (b0 << 16) | (b1 << 8) | b2;
                b64.push(B64[((triple >> 18) & 0x3F) as usize] as char);
                b64.push(B64[((triple >> 12) & 0x3F) as usize] as char);
                if i + 1 < content.len() {
                    b64.push(B64[((triple >> 6) & 0x3F) as usize] as char);
                } else {
                    b64.push('=');
                }
                if i + 2 < content.len() {
                    b64.push(B64[(triple & 0x3F) as usize] as char);
                } else {
                    b64.push('=');
                }
                i += 3;
            }
        }

        let dest_path = if dest.starts_with('/') {
            dest.to_string()
        } else {
            format!("/{dest}")
        };

        let dir_cmd = format!("mkdir -p $(dirname '{dest_path}')");
        let _ = self.container_exec(host_id, container_name, &dir_cmd).await;

        let chunk_size = 76_000; // base64 chars per chunk (~57KB binary)
        if b64.len() <= chunk_size {
            let cmd = format!("printf '%s' '{b64}' | base64 -d > '{dest_path}'");
            self.container_exec(host_id, container_name, &cmd).await?;
        } else {
            // First chunk: write base64 to temp file, then decode
            // (can't pipe partial base64 - must decode complete blocks)
            let first_chunk = &b64[..chunk_size];
            let cmd = format!("printf '%s' '{first_chunk}' > '{dest_path}.b64'");
            self.container_exec(host_id, container_name, &cmd).await?;
            // Remaining chunks (append to b64 file)
            let mut offset = chunk_size;
            while offset < b64.len() {
                let end = (offset + chunk_size).min(b64.len());
                let chunk = &b64[offset..end];
                let cmd = format!("printf '%s' '{chunk}' >> '{dest_path}.b64'");
                self.container_exec(host_id, container_name, &cmd).await?;
                offset = end;
            }
            // Decode the complete b64 file
            let cmd = format!("base64 -d '{dest_path}.b64' > '{dest_path}' && rm -f '{dest_path}.b64'");
            self.container_exec(host_id, container_name, &cmd).await?;
        }

        Ok(())
    }

    /// Push a text string into a container file.
    async fn container_push_text(
        &self,
        host_id: &str,
        container_name: &str,
        text: &str,
        dest: &str,
    ) -> Result<(), String> {
        self.container_push_content(host_id, container_name, text.as_bytes(), dest)
            .await
    }

    /// Create workspace directory for a container (local or remote).
    async fn container_create_workspace(
        &self,
        host_id: &str,
        container_name: &str,
        storage_path: &Path,
    ) -> Result<(), String> {
        if host_id == "local" {
            NspawnClient::create_workspace(container_name, storage_path)
                .await
                .map_err(|e| e.to_string())
        } else {
            // On remote hosts, workspace is created by CreateNspawnContainer.
            // Just ensure it exists inside the container.
            self.container_exec(
                host_id,
                container_name,
                "mkdir -p /root/workspace && chmod 755 /root/workspace",
            )
            .await?;
            Ok(())
        }
    }

    /// Wait for network connectivity in a container (local or remote).
    async fn container_wait_for_network(
        &self,
        host_id: &str,
        container_name: &str,
        timeout_secs: u32,
    ) -> Result<(), String> {
        if host_id == "local" {
            NspawnClient::wait_for_network(container_name, timeout_secs)
                .await
                .map_err(|e| e.to_string())
        } else {
            for _i in 0..timeout_secs {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Ok(out) = self
                    .container_exec(host_id, container_name, "ip addr show | grep 'inet ' | grep -v 127.0.0.1")
                    .await
                {
                    if !out.trim().is_empty() {
                        return Ok(());
                    }
                }
            }
            Err(format!("Network not ready after {timeout_secs}s"))
        }
    }

    /// Deploy agent binary to a container.
    /// Local: copies from /opt/homeroute/data/agent-binaries/hr-agent
    /// Remote: downloads from orchestrator HTTP API inside the container
    async fn container_deploy_agent_binary(
        &self,
        host_id: &str,
        container_name: &str,
        storage_path: &Path,
    ) -> Result<(), String> {
        if host_id == "local" {
            let agent_binary = PathBuf::from("/opt/homeroute/data/agent-binaries/hr-agent");
            if !agent_binary.exists() {
                return Err("Agent binary not found".to_string());
            }
            NspawnClient::push_file(
                container_name,
                &agent_binary,
                "usr/local/bin/hr-agent",
                storage_path,
            )
            .await
            .map_err(|e| e.to_string())?;
        } else {
            // Download from orchestrator HTTP API
            let api_port = self.env.api_port;
            let cmd = format!(
                "curl -4 -fsSL -o /usr/local/bin/hr-agent http://10.0.0.254:{api_port}/api/applications/agents/binary"
            );
            self.container_exec(host_id, container_name, &cmd).await?;
        }
        self.container_exec(host_id, container_name, "chmod +x /usr/local/bin/hr-agent")
            .await?;
        Ok(())
    }

    /// Write workspace files (MCP config, rules, .gitignore) for a DEV container.
    /// Local: writes directly to the workspace bind-mount on disk.
    /// Remote: writes inside the container via exec.
    async fn deploy_workspace_files(
        &self,
        host_id: &str,
        container_name: &str,
        storage_path: &Path,
        slug: &str,
        stack: hr_registry::types::AppStack,
    ) {
        let base_domain = &self.env.base_domain;
        let api_port = self.env.api_port;
        let render_rules = |template: &str| -> String {
            template
                .replace("{{slug}}", slug)
                .replace("{{domain}}", base_domain)
        };

        let mcp_config = r#"{
  "mcpServers": {
    "dataverse": {
      "command": "/usr/local/bin/hr-agent",
      "args": ["mcp"],
      "autoApprove": [
        "list_tables","describe_table","create_table","add_column","remove_column",
        "drop_table","create_relation","query_data","insert_data","update_data",
        "delete_data","count_rows","get_schema","get_db_info"
      ]
    },
    "deploy": {
      "command": "/usr/local/bin/hr-agent",
      "args": ["mcp-deploy"],
      "autoApprove": [
        "deploy_status","prod_logs"
      ]
    },
    "store": {
      "command": "/usr/local/bin/hr-agent",
      "args": ["mcp-store"],
      "autoApprove": [
        "list_store_apps","get_app_info","check_updates","publish_release"
      ]
    }
  }
}
"#;

        let dev_md_content = render_rules(include_str!("../../hr-registry/src/rules/homeroute-dev-nextjs.md"));
        let deploy_md_content = render_rules(include_str!("../../hr-registry/src/rules/homeroute-deploy-nextjs.md"));
        let dataverse_md = render_rules(include_str!("../../hr-registry/src/rules/homeroute-dataverse.md"));
        let store_md = render_rules(include_str!("../../hr-registry/src/rules/homeroute-store.md"));
        let todos_md = render_rules(include_str!("../../hr-registry/src/rules/homeroute-studio-todos.md"));
        let gitignore = "node_modules/\ntarget/\n.env\n*.db\n*.db-shm\n*.db-wal\ndist/\n.cache/\ntmp/\n";

        if host_id == "local" {
            // Write directly to workspace bind mount
            let ws_dir = storage_path.join(format!("{container_name}-workspace"));
            let _ = tokio::fs::write(ws_dir.join(".mcp.json"), mcp_config).await;
            let rules_dir = ws_dir.join(".claude/rules");
            let _ = tokio::fs::create_dir_all(&rules_dir).await;
            let _ = tokio::fs::write(rules_dir.join("homeroute-dataverse.md"), &dataverse_md).await;
            let _ = tokio::fs::write(rules_dir.join("homeroute-deploy.md"), &deploy_md_content).await;
            let _ = tokio::fs::write(rules_dir.join("homeroute-dev.md"), &dev_md_content).await;
            let _ = tokio::fs::write(rules_dir.join("homeroute-store.md"), &store_md).await;
            let _ = tokio::fs::write(rules_dir.join("homeroute-studio-todos.md"), &todos_md).await;
            let _ = tokio::fs::write(ws_dir.join(".gitignore"), gitignore).await;
        } else {
            // Write via exec inside the container
            let _ = self.container_push_text(host_id, container_name, mcp_config, "/root/workspace/.mcp.json").await;
            let _ = self.container_exec(host_id, container_name, "mkdir -p /root/workspace/.claude/rules").await;
            let _ = self.container_push_text(host_id, container_name, &dataverse_md, "/root/workspace/.claude/rules/homeroute-dataverse.md").await;
            let _ = self.container_push_text(host_id, container_name, &deploy_md_content, "/root/workspace/.claude/rules/homeroute-deploy.md").await;
            let _ = self.container_push_text(host_id, container_name, &dev_md_content, "/root/workspace/.claude/rules/homeroute-dev.md").await;
            let _ = self.container_push_text(host_id, container_name, &store_md, "/root/workspace/.claude/rules/homeroute-store.md").await;
            let _ = self.container_push_text(host_id, container_name, &todos_md, "/root/workspace/.claude/rules/homeroute-studio-todos.md").await;
            let _ = self.container_push_text(host_id, container_name, gitignore, "/root/workspace/.gitignore").await;
        }

        // Git init workspace
        let git_init_cmd = format!(
            r#"cd /root/workspace && git init && git config user.name "HomeRoute ({slug})" && git config user.email "{slug}@{base_domain}" && git add -A && git commit -m "Initial scaffold (HomeRoute)" && git remote add origin http://10.0.0.254:{api_port}/api/git/repos/{slug}.git"#,
        );
        let _ = self.container_exec(host_id, container_name, &git_init_cmd).await;
    }


    /// Deploy a production container: agent binary + config + systemd unit only.
    /// No code-server, no Claude Code CLI, no MCP config, no CLAUDE.md, no extensions.
    async fn run_nspawn_deploy_prod(
        &self,
        app_id: &str,
        slug: &str,
        container_name: &str,
        host_id: &str,
        token: &str,
        stack: hr_registry::types::AppStack,
    ) {
        let emit = |message: &str| {
            let _ = self.events.agent_status.send(AgentStatusEvent {
                app_id: app_id.to_string(),
                slug: slug.to_string(),
                status: "deploying".to_string(),
                message: Some(message.to_string()),
            });
        };

        let storage_path = self.resolve_storage_path(host_id).await;
        let storage = Path::new(&storage_path);

        let network_mode = match self.resolve_network_mode(host_id).await {
            Ok(nm) => nm,
            Err(e) => {
                error!(container = container_name, "Network mode resolution failed: {e}");
                emit(&format!("Erreur: {e}"));
                self.set_container_status(app_id, ContainerV2Status::Error)
                    .await;
                return;
            }
        };

        // Phase 1: Create the nspawn container (prod -> no workspace)
        emit("Creation du conteneur nspawn...");
        if let Err(e) = self.nspawn_create_container(host_id, container_name, &storage_path, &network_mode, false).await {
            error!(container = container_name, "Nspawn creation failed: {e}");
            emit(&format!("Erreur: {e}"));
            self.set_container_status(app_id, ContainerV2Status::Error)
                .await;
            return;
        }

        // Phase 2: Deploy agent binary
        emit("Deploiement du binaire agent...");
        if let Err(e) = self.container_deploy_agent_binary(host_id, container_name, storage).await {
            error!(container = container_name, "Agent binary deploy failed: {e}");
            emit(&format!("Erreur: {e}"));
            self.set_container_status(app_id, ContainerV2Status::Error)
                .await;
            return;
        }

        // Phase 3: Generate and push agent config
        emit("Configuration de l'agent...");
        let api_port = self.env.api_port;
        let config_content = format!(
            r#"homeroute_address = "10.0.0.254"
homeroute_port = {api_port}
token = "{token}"
service_name = "{slug}"
interface = "host0"
"#
        );
        let _ = self.container_push_text(host_id, container_name, &config_content, "/etc/hr-agent.toml").await;

        // Phase 4: Push systemd unit
        let unit_content = r#"[Unit]
Description=HomeRoute Agent
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/hr-agent
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#;
        let _ = self.container_push_text(host_id, container_name, unit_content, "/etc/systemd/system/hr-agent.service").await;

        // Phase 5: Enable and start agent
        emit("Demarrage de l'agent...");
        let _ = self.container_exec(host_id, container_name, "systemctl daemon-reload").await;
        let _ = self.container_exec(host_id, container_name, "systemctl enable --now hr-agent").await;

        // Phase 6: Wait for network
        emit("Attente de la connectivite reseau...");
        if let Err(e) = self.container_wait_for_network(host_id, container_name, 30).await {
            warn!(container = container_name, "Network wait failed: {e}");
        }

        // Phase 7: Install dependencies
        emit("Installation des dependances...");
        let _ = self.container_exec_retry(host_id, container_name, "apt-get update -qq && apt-get install -y -qq curl ca-certificates", 3).await;

        // Phase 7b: Install Node.js for NextJs stack
        if stack == hr_registry::types::AppStack::NextJs {
            emit("Installation Node.js 22 (PROD)...");
            let _ = self.container_exec_retry(host_id, container_name, "curl -4 -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y -qq nodejs", 3).await;
        }

        // Update status (no workspace for prod containers)
        self.set_container_status(app_id, ContainerV2Status::Running)
            .await;

        let _ = self.events.agent_status.send(AgentStatusEvent {
            app_id: app_id.to_string(),
            slug: slug.to_string(),
            status: "pending".to_string(),
            message: Some("Deploiement termine".to_string()),
        });


        info!(container = container_name, "Container V2 prod deploy complete");
    }

    async fn set_container_status(&self, id: &str, status: ContainerV2Status) {
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == id) {
                c.status = status.clone();
            }
        }
        let _ = self.save_state().await;

        // Also update registry application status
        let agent_status = match status {
            ContainerV2Status::Deploying => AgentStatus::Deploying,
            ContainerV2Status::Error => AgentStatus::Error,
            _ => return,
        };
        let _ = self
            .registry
            .update_application(
                id,
                UpdateApplicationRequest {
                    ..Default::default()
                },
            )
            .await;
        // Set status through the registry's internal mechanism if needed
        let _ = agent_status; // used for matching only
    }

    /// Set container status (public version for IPC handler).
    pub async fn set_container_status_pub(&self, app_id: &str, status: ContainerV2Status) {
        self.set_container_status(app_id, status).await;
    }

    // ── Inter-host migration ─────────────────────────────────────

    /// Start migration of a V2 container to another host.
    pub async fn migrate_container(
        self: &Arc<Self>,
        container_id: &str,
        target_host_id: &str,
        migrations: &Arc<RwLock<HashMap<String, MigrationState>>>,
    ) -> Result<String, String> {
        let record = {
            let state = self.state.read().await;
            state
                .containers
                .iter()
                .find(|c| c.id == container_id)
                .cloned()
        };

        let record = record.ok_or("Container not found")?;
        let source_host_id = record.host_id.clone();

        if source_host_id == target_host_id {
            return Err("Container is already on target host".to_string());
        }

        if target_host_id != "local" && !self.registry.is_host_connected(target_host_id).await {
            return Err("Target host is not connected".to_string());
        }

        let transfer_id = uuid::Uuid::new_v4().to_string();
        let cancelled = Arc::new(AtomicBool::new(false));

        let migration_state = MigrationState {
            app_id: container_id.to_string(),
            transfer_id: transfer_id.clone(),
            source_host_id: source_host_id.clone(),
            target_host_id: target_host_id.to_string(),
            phase: MigrationPhase::Stopping,
            progress_pct: 0,
            bytes_transferred: 0,
            total_bytes: 0,
            started_at: Utc::now(),
            error: None,
            cancelled: cancelled.clone(),
        };

        {
            let mut m = migrations.write().await;
            if m.values().any(|ms| {
                ms.app_id == container_id
                    && ms.error.is_none()
                    && !matches!(
                        ms.phase,
                        MigrationPhase::Complete | MigrationPhase::Failed
                    )
            }) {
                return Err("Migration already in progress".to_string());
            }
            m.insert(transfer_id.clone(), migration_state);
        }

        // Update container status
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == container_id) {
                c.status = ContainerV2Status::Migrating;
            }
        }
        let _ = self.save_state().await;

        let mgr = Arc::clone(self);
        let migrations = migrations.clone();
        let events = self.events.clone();
        let registry = self.registry.clone();
        let tid = transfer_id.clone();
        let cid = container_id.to_string();
        let thid = target_host_id.to_string();
        let slug = record.slug.clone();
        let container_name = record.container_name.clone();

        tokio::spawn(async move {
            mgr.run_nspawn_migration(
                &registry,
                &migrations,
                &events,
                &cid,
                &slug,
                &tid,
                &source_host_id,
                &thid,
                &container_name,
                &cancelled,
            )
            .await;
        });

        Ok(transfer_id)
    }

    async fn run_nspawn_migration(
        &self,
        registry: &Arc<AgentRegistry>,
        migrations: &Arc<RwLock<HashMap<String, MigrationState>>>,
        events: &Arc<EventBus>,
        app_id: &str,
        _slug: &str,
        transfer_id: &str,
        source_host_id: &str,
        target_host_id: &str,
        container_name: &str,
        cancelled: &Arc<AtomicBool>,
    ) {
        let source_stopped = AtomicBool::new(false);

        let result = self
            .run_nspawn_migration_inner(
                registry,
                migrations,
                events,
                app_id,
                transfer_id,
                source_host_id,
                target_host_id,
                container_name,
                &source_stopped,
                cancelled,
            )
            .await;

        if let Err(error_msg) = result {
            // Rollback: restart source container if stopped
            if source_stopped.load(Ordering::SeqCst) {
                warn!(
                    app_id = %app_id,
                    container = %container_name,
                    "Nspawn migration failed after source stop, restarting source"
                );
                if source_host_id == "local" {
                    let _ = NspawnClient::start_container(container_name).await;
                } else {
                    let _ = registry
                        .send_host_command(
                            source_host_id,
                            HostRegistryMessage::StartContainer {
                                container_name: container_name.to_string(),
                            },
                        )
                        .await;
                }
            }

            // Restore container status
            {
                let mut state = self.state.write().await;
                if let Some(c) = state.containers.iter_mut().find(|c| c.id == app_id) {
                    c.status = ContainerV2Status::Running;
                }
            }
            let _ = self.save_state().await;

            update_migration_phase(
                migrations,
                events,
                app_id,
                transfer_id,
                MigrationPhase::Failed,
                0,
                0,
                0,
                Some(error_msg),
            )
            .await;
        }
    }

    async fn run_nspawn_migration_inner(
        &self,
        registry: &Arc<AgentRegistry>,
        migrations: &Arc<RwLock<HashMap<String, MigrationState>>>,
        events: &Arc<EventBus>,
        app_id: &str,
        transfer_id: &str,
        source_host_id: &str,
        target_host_id: &str,
        container_name: &str,
        source_stopped: &AtomicBool,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        let source_is_local = source_host_id == "local";
        let target_is_local = target_host_id == "local";

        let source_storage = self.resolve_storage_path(source_host_id).await;
        let target_storage = self.resolve_storage_path(target_host_id).await;

        // Phase 1: Stopping
        update_migration_phase(
            migrations,
            events,
            app_id,
            transfer_id,
            MigrationPhase::Stopping,
            0,
            0,
            0,
            None,
        )
        .await;

        // Phase 2: Exporting
        update_migration_phase(
            migrations,
            events,
            app_id,
            transfer_id,
            MigrationPhase::Exporting,
            10,
            0,
            0,
            None,
        )
        .await;

        if source_is_local {
            // Stop the container
            let _ = NspawnClient::stop_container(container_name).await;
            source_stopped.store(true, Ordering::SeqCst);

            let rootfs_path = Path::new(&source_storage).join(container_name);

            // Estimate size
            let size_output = tokio::process::Command::new("du")
                .args(["-sb", &rootfs_path.to_string_lossy()])
                .output()
                .await
                .map_err(|e| format!("du failed: {e}"))?;
            let total_bytes: u64 = String::from_utf8_lossy(&size_output.stdout)
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            update_migration_phase(
                migrations,
                events,
                app_id,
                transfer_id,
                MigrationPhase::Transferring,
                20,
                0,
                total_bytes,
                None,
            )
            .await;

            if !target_is_local {
                // Local -> Remote: stream rootfs tar to target
                let import_rx = registry.register_migration_signal(transfer_id).await;

                let target_network_mode = self.resolve_network_mode(target_host_id).await?;
                let _ = registry
                    .send_host_command(
                        target_host_id,
                        HostRegistryMessage::StartNspawnImport {
                            container_name: container_name.to_string(),
                            storage_path: target_storage.clone(),
                            transfer_id: transfer_id.to_string(),
                            network_mode: target_network_mode,
                        },
                    )
                    .await
                    .map_err(|e| format!("Failed to notify target: {e}"))?;

                // Spawn tar
                let mut tar_child = tokio::process::Command::new("tar")
                    .args(["cf", "-", "-C", &rootfs_path.to_string_lossy(), "."])
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn tar: {e}"))?;

                let mut tar_stdout = tar_child.stdout.take().unwrap();

                let (_transferred, _seq) = stream_to_remote(
                    registry,
                    target_host_id,
                    transfer_id,
                    &mut tar_stdout,
                    total_bytes,
                    cancelled,
                    migrations,
                    events,
                    app_id,
                    20,
                    80,
                    MigrationPhase::Transferring,
                )
                .await?;

                let _ = tar_child.wait().await;

                // Stream workspace if exists
                let ws_path = Path::new(&source_storage).join(format!("{}-workspace", container_name));
                if tokio::fs::metadata(&ws_path).await.is_ok() {
                    let ws_size_output = tokio::process::Command::new("du")
                        .args(["-sb", &ws_path.to_string_lossy()])
                        .output()
                        .await;
                    let ws_size: u64 = ws_size_output
                        .ok()
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .split_whitespace()
                                .next()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);

                    let _ = registry
                        .send_host_command(
                            target_host_id,
                            HostRegistryMessage::WorkspaceReady {
                                transfer_id: transfer_id.to_string(),
                                size_bytes: ws_size,
                            },
                        )
                        .await;

                    if let Ok(mut ws_child) = tokio::process::Command::new("tar")
                        .args(["cf", "-", "-C", &ws_path.to_string_lossy(), "."])
                        .stdout(std::process::Stdio::piped())
                        .spawn()
                    {
                        if let Some(mut ws_stdout) = ws_child.stdout.take() {
                            let _ = stream_to_remote(
                                registry,
                                target_host_id,
                                transfer_id,
                                &mut ws_stdout,
                                ws_size,
                                cancelled,
                                migrations,
                                events,
                                app_id,
                                82,
                                84,
                                MigrationPhase::TransferringWorkspace,
                            )
                            .await;
                        }
                        let _ = ws_child.wait().await;
                    }
                }

                let _ = registry
                    .send_host_command(
                        target_host_id,
                        HostRegistryMessage::TransferComplete {
                            transfer_id: transfer_id.to_string(),
                        },
                    )
                    .await;

                update_migration_phase(
                    migrations,
                    events,
                    app_id,
                    transfer_id,
                    MigrationPhase::Importing,
                    85,
                    0,
                    0,
                    None,
                )
                .await;

                match tokio::time::timeout(Duration::from_secs(120), import_rx).await {
                    Ok(Ok(hr_registry::MigrationResult::ImportComplete { .. })) => {
                        info!(transfer_id, "Nspawn import confirmed by target host");
                    }
                    Ok(Ok(hr_registry::MigrationResult::ImportFailed { error })) => {
                        return Err(format!("Migration failed on target: {error}"));
                    }
                    Ok(Ok(hr_registry::MigrationResult::ExportFailed { error })) => {
                        return Err(format!("Migration failed: {error}"));
                    }
                    Ok(Err(_)) => return Err("Migration signal lost".to_string()),
                    Err(_) => return Err("Import timed out after 120s".to_string()),
                }
            } else {
                // Local -> Local: unlikely but handle gracefully
                return Err("Local-to-local nspawn migration not supported".to_string());
            }
        } else {
            // Source is remote
            let import_rx = registry.register_migration_signal(transfer_id).await;

            if target_is_local {
                registry
                    .set_transfer_container_name(transfer_id, container_name)
                    .await;
            } else {
                registry
                    .set_transfer_relay_target(transfer_id, target_host_id, container_name)
                    .await;

                let target_network_mode = self.resolve_network_mode(target_host_id).await?;
                let _ = registry
                    .send_host_command(
                        target_host_id,
                        HostRegistryMessage::StartNspawnImport {
                            container_name: container_name.to_string(),
                            storage_path: target_storage.clone(),
                            transfer_id: transfer_id.to_string(),
                            network_mode: target_network_mode,
                        },
                    )
                    .await
                    .map_err(|e| format!("Failed to notify target: {e}"))?;
            }

            let _ = registry
                .send_host_command(
                    source_host_id,
                    HostRegistryMessage::StartNspawnExport {
                        container_name: container_name.to_string(),
                        storage_path: source_storage.clone(),
                        transfer_id: transfer_id.to_string(),
                    },
                )
                .await
                .map_err(|e| format!("Failed to start export: {e}"))?;

            source_stopped.store(true, Ordering::SeqCst);

            update_migration_phase(
                migrations,
                events,
                app_id,
                transfer_id,
                MigrationPhase::Exporting,
                10,
                0,
                0,
                None,
            )
            .await;

            match tokio::time::timeout(Duration::from_secs(600), import_rx).await {
                Ok(Ok(hr_registry::MigrationResult::ExportFailed { error })) => {
                    return Err(format!("Export failed on source: {error}"));
                }
                Ok(Ok(hr_registry::MigrationResult::ImportFailed { error })) => {
                    return Err(format!("Import failed: {error}"));
                }
                Ok(Ok(hr_registry::MigrationResult::ImportComplete { .. })) => {
                    info!(transfer_id, "Remote nspawn migration confirmed");
                }
                Ok(Err(_)) => return Err("Migration signal lost".to_string()),
                Err(_) => return Err("Remote migration timed out after 600s".to_string()),
            }
        }

        // Phase 5: Starting -- update host_id
        update_migration_phase(
            migrations,
            events,
            app_id,
            transfer_id,
            MigrationPhase::Starting,
            90,
            0,
            0,
            None,
        )
        .await;

        let update_req = UpdateApplicationRequest {
            host_id: Some(target_host_id.to_string()),
            ..Default::default()
        };
        let mut host_updated = false;
        for attempt in 0..3u32 {
            match registry.update_application(app_id, update_req.clone()).await {
                Ok(_) => {
                    host_updated = true;
                    break;
                }
                Err(e) => {
                    warn!(attempt, "Failed to update host_id: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        if !host_updated {
            return Err("Failed to update application host_id after 3 attempts".to_string());
        }

        // Update V2 record
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == app_id) {
                c.host_id = target_host_id.to_string();
            }
        }
        let _ = self.save_state().await;

        // Phase 6: Verifying
        update_migration_phase(
            migrations,
            events,
            app_id,
            transfer_id,
            MigrationPhase::Verifying,
            93,
            0,
            0,
            None,
        )
        .await;

        let mut agent_reconnected = false;
        for _ in 0..30 {
            if registry.is_agent_connected(app_id).await {
                agent_reconnected = true;
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        if !agent_reconnected {
            error!(app_id, "Agent did not reconnect within 60s after nspawn migration");

            // Rollback: delete target, revert host_id, restart source
            if target_is_local {
                let _ = NspawnClient::delete_container(
                    container_name,
                    Path::new(&target_storage),
                )
                .await;
            } else {
                let _ = registry
                    .send_host_command(
                        target_host_id,
                        HostRegistryMessage::DeleteContainer {
                            container_name: container_name.to_string(),
                        },
                    )
                    .await;
            }

            let revert_req = UpdateApplicationRequest {
                host_id: Some(source_host_id.to_string()),
                ..Default::default()
            };
            let _ = registry.update_application(app_id, revert_req).await;

            {
                let mut state = self.state.write().await;
                if let Some(c) = state.containers.iter_mut().find(|c| c.id == app_id) {
                    c.host_id = source_host_id.to_string();
                }
            }
            let _ = self.save_state().await;

            return Err("Agent did not reconnect after migration".to_string());
        }

        // Phase 7: Cleanup source
        if source_is_local {
            let _ = NspawnClient::delete_container(
                container_name,
                Path::new(&source_storage),
            )
            .await;
        } else {
            // Retry cleanup with reconnection wait -- the source host may have
            // temporarily disconnected during the migration (heartbeat timeout).
            let mut cleanup_ok = false;
            for attempt in 1..=5 {
                match registry
                    .send_host_command(
                        source_host_id,
                        HostRegistryMessage::DeleteNspawnContainer {
                            container_name: container_name.to_string(),
                            storage_path: source_storage.clone(),
                        },
                    )
                    .await
                {
                    Ok(()) => {
                        info!(transfer_id, attempt, "Source cleanup command sent to {}", source_host_id);
                        cleanup_ok = true;
                        break;
                    }
                    Err(e) => {
                        warn!(transfer_id, attempt, "Source cleanup failed: {e} -- retrying in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                }
            }
            if !cleanup_ok {
                error!(transfer_id, "Failed to send cleanup to source host {} after 5 attempts", source_host_id);
            }
        }

        // Update container status
        {
            let mut state = self.state.write().await;
            if let Some(c) = state.containers.iter_mut().find(|c| c.id == app_id) {
                c.status = ContainerV2Status::Running;
            }
        }
        let _ = self.save_state().await;

        // Phase 8: Complete
        update_migration_phase(
            migrations,
            events,
            app_id,
            transfer_id,
            MigrationPhase::Complete,
            100,
            0,
            0,
            None,
        )
        .await;

        info!(
            app_id,
            transfer_id, "Nspawn migration complete: {} -> {}", source_host_id, target_host_id
        );
        Ok(())
    }

    // ── Slug rename ─────────────────────────────────────────────

    /// Rename a container's slug (and all its linked containers).
    /// Returns a rename_id for tracking progress. The actual rename runs in a background task.
    pub async fn rename_container(
        self: &Arc<Self>,
        id: &str,
        req: RenameContainerRequest,
        renames: &Arc<RwLock<HashMap<String, RenameState>>>,
    ) -> Result<String, String> {
        let new_slug = req.new_slug.trim().to_lowercase();

        // -- Phase 0: Validation --
        // Slug format: lowercase alphanumeric + hyphens, 3-32 chars, no leading/trailing hyphen
        let valid_slug = new_slug.len() >= 3
            && new_slug.len() <= 32
            && new_slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !new_slug.starts_with('-')
            && !new_slug.ends_with('-');
        if !valid_slug {
            return Err("Invalid slug: must be 3-32 chars, lowercase alphanumeric and hyphens, cannot start/end with hyphen".to_string());
        }

        // Get the primary container record
        let record = {
            let state = self.state.read().await;
            state.containers.iter().find(|c| c.id == id).cloned()
        };
        let record = record.ok_or("Container not found")?;
        let old_slug = record.slug.clone();

        if old_slug == new_slug {
            return Err("New slug is the same as the current slug".to_string());
        }

        // Must be local
        if record.host_id != "local" {
            return Err("Rename is only supported for local containers".to_string());
        }

        // Check no duplicate slug in registry
        let apps = self.registry.list_applications().await;
        if apps.iter().any(|a| a.slug == new_slug) {
            return Err(format!("Slug '{}' is already in use", new_slug));
        }

        // Find all app IDs with this slug (dev + prod pair)
        let app_ids: Vec<String> = apps
            .iter()
            .filter(|a| a.slug == old_slug)
            .map(|a| a.id.clone())
            .collect();
        if app_ids.is_empty() {
            return Err("No applications found with this slug".to_string());
        }

        // Check no active migration for any of these apps
        {
            let migrations = renames.read().await;
            for rs in migrations.values() {
                if rs.app_ids.iter().any(|aid| app_ids.contains(aid))
                    && rs.phase != RenamePhase::Complete
                    && rs.phase != RenamePhase::Failed
                {
                    return Err("A rename is already in progress for this application".to_string());
                }
            }
        }

        let rename_id = uuid::Uuid::new_v4().to_string();
        let rename_state = RenameState {
            rename_id: rename_id.clone(),
            app_ids: app_ids.clone(),
            old_slug: old_slug.clone(),
            new_slug: new_slug.clone(),
            phase: RenamePhase::Validating,
            started_at: Utc::now(),
            error: None,
        };

        {
            let mut renames_map = renames.write().await;
            renames_map.insert(rename_id.clone(), rename_state);
        }

        // Spawn background task
        let mgr = Arc::clone(self);
        let renames = renames.clone();
        let rid = rename_id.clone();
        let new_name = req.new_name;

        tokio::spawn(async move {
            mgr.run_rename(&rid, &old_slug, &new_slug, &app_ids, new_name, &renames)
                .await;
        });

        Ok(rename_id)
    }

    /// Background rename execution with rollback journal.
    async fn run_rename(
        &self,
        rename_id: &str,
        old_slug: &str,
        new_slug: &str,
        app_ids: &[String],
        new_name: Option<String>,
        renames: &Arc<RwLock<HashMap<String, RenameState>>>,
    ) {
        let result = self
            .run_rename_inner(rename_id, old_slug, new_slug, app_ids, new_name, renames)
            .await;

        if let Err(error_msg) = result {
            error!(
                rename_id,
                old_slug,
                new_slug,
                error = %error_msg,
                "Rename failed"
            );
            Self::set_rename_phase(renames, rename_id, RenamePhase::Failed, Some(error_msg))
                .await;
        }
    }

    async fn run_rename_inner(
        &self,
        rename_id: &str,
        old_slug: &str,
        new_slug: &str,
        app_ids: &[String],
        new_name: Option<String>,
        renames: &Arc<RwLock<HashMap<String, RenameState>>>,
    ) -> Result<(), String> {
        let storage_path = self.resolve_storage_path("local").await;
        let storage = Path::new(&storage_path);

        // Gather per-app info: (app_id, old_container_name, new_container_name)
        let apps = self.registry.list_applications().await;
        let mut app_infos: Vec<(String, String, String)> = Vec::new();
        for aid in app_ids {
            let app = apps.iter().find(|a| a.id == *aid).ok_or_else(|| {
                format!("Application {} not found in registry", aid)
            })?;
            let new_container_name = format!("hr-v2-{}-prod", new_slug);
            app_infos.push((
                aid.clone(),
                app.container_name.clone(),
                new_container_name,
            ));
        }

        // Determine the network mode for .nspawn unit rewrite
        let network_mode = self.resolve_network_mode("local").await
            .map_err(|e| format!("Cannot resolve network mode: {e}"))?;

        // -- Phase 2: Create new DNS records --
        Self::set_rename_phase(renames, rename_id, RenamePhase::CreatingDns, None).await;
        let dns_created = if let (Some(token), Some(zone_id)) =
            (&self.env.cf_api_token, &self.env.cf_zone_id)
        {
            // Get public IPv6 for the AAAA record
            let ipv6 = Self::get_public_ipv6(&self.env.cf_interface)?;
            if let Err(e) = hr_registry::cloudflare::upsert_app_wildcard_dns(
                token,
                zone_id,
                new_slug,
                &self.env.base_domain,
                &ipv6,
                true,
                "AAAA",
            )
            .await
            {
                // Rollback: delete new cert
                self.rollback_cert(new_slug).await;
                return Err(format!("Failed to create DNS for new slug: {e}"));
            }
            true
        } else {
            false
        };

        // -- Phase 3: Stop containers --
        Self::set_rename_phase(renames, rename_id, RenamePhase::StoppingContainers, None).await;
        let mut stopped_containers: Vec<String> = Vec::new();
        for (_, old_name, _) in &app_infos {
            if let Err(e) = NspawnClient::stop_container(old_name).await {
                warn!(container = old_name.as_str(), "Stop failed (may already be stopped): {e}");
            }
            stopped_containers.push(old_name.clone());
        }
        // Wait for containers to fully stop
        tokio::time::sleep(Duration::from_secs(3)).await;

        // -- Phase 4: Rename filesystem --
        Self::set_rename_phase(renames, rename_id, RenamePhase::RenamingFilesystem, None).await;
        let mut renamed_rootfs: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut renamed_workspaces: Vec<(PathBuf, PathBuf)> = Vec::new();

        for (_, old_name, new_name) in &app_infos {
            // Rename rootfs
            let old_rootfs = storage.join(old_name);
            let new_rootfs = storage.join(new_name);
            if old_rootfs.exists() {
                if let Err(e) = tokio::fs::rename(&old_rootfs, &new_rootfs).await {
                    // Rollback already-renamed rootfs
                    self.rollback_renames(&renamed_rootfs).await;
                    self.rollback_renames(&renamed_workspaces).await;
                    self.rollback_start(&stopped_containers).await;
                    if dns_created {
                        self.rollback_dns(new_slug).await;
                    }
                    self.rollback_cert(new_slug).await;
                    return Err(format!(
                        "Failed to rename rootfs {} -> {}: {e}",
                        old_rootfs.display(),
                        new_rootfs.display()
                    ));
                }
                renamed_rootfs.push((new_rootfs, old_rootfs));
            }

            // Rename workspace
            let old_ws = storage.join(format!("{}-workspace", old_name));
            let new_ws = storage.join(format!("{}-workspace", new_name));
            if old_ws.exists() {
                if let Err(e) = tokio::fs::rename(&old_ws, &new_ws).await {
                    // Rollback workspace and rootfs renames
                    self.rollback_renames(&renamed_workspaces).await;
                    self.rollback_renames(&renamed_rootfs).await;
                    self.rollback_start(&stopped_containers).await;
                    if dns_created {
                        self.rollback_dns(new_slug).await;
                    }
                    self.rollback_cert(new_slug).await;
                    return Err(format!(
                        "Failed to rename workspace {} -> {}: {e}",
                        old_ws.display(),
                        new_ws.display()
                    ));
                }
                renamed_workspaces.push((new_ws, old_ws));
            }

            // Delete old .nspawn unit, write new one
            let old_unit = format!("/etc/systemd/nspawn/{}.nspawn", old_name);
            let _ = tokio::fs::remove_file(&old_unit).await;

            // Determine if workspace exists for .nspawn unit
            let has_workspace = storage
                .join(format!("{}-workspace", new_name))
                .exists();

            // Remove old symlink from /var/lib/machines/ if exists
            let old_machine_link = Path::new("/var/lib/machines").join(old_name);
            if old_machine_link.is_symlink() {
                let _ = tokio::fs::remove_file(&old_machine_link).await;
            }

            // Collect volume binds for the renamed container
            let vol_binds: Vec<(String, String, bool)> = {
                let st = self.state.read().await;
                st.containers.iter()
                    .find(|c| c.container_name == *new_name)
                    .map(|c| c.volumes.iter().map(|v| (v.source_path.clone(), v.mount_point.clone(), v.read_only)).collect())
                    .unwrap_or_default()
            };

            if let Err(e) =
                NspawnClient::write_nspawn_unit(new_name, storage, &network_mode, has_workspace, &vol_binds)
                    .await
            {
                warn!(
                    old = old_name.as_str(),
                    new = new_name.as_str(),
                    "Failed to write new .nspawn unit: {e}"
                );
            }
        }

        // -- Phase 5: Update agent config --
        Self::set_rename_phase(renames, rename_id, RenamePhase::UpdatingAgentConfig, None).await;
        for (_, _, new_name) in &app_infos {
            let config_path = storage.join(new_name).join("etc/hr-agent.toml");
            if config_path.exists() {
                match tokio::fs::read_to_string(&config_path).await {
                    Ok(content) => {
                        let updated = content.replace(
                            &format!("service_name = \"{}\"", old_slug),
                            &format!("service_name = \"{}\"", new_slug),
                        );
                        if let Err(e) = tokio::fs::write(&config_path, &updated).await {
                            warn!(
                                config = %config_path.display(),
                                "Failed to update agent config: {e}"
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            config = %config_path.display(),
                            "Failed to read agent config: {e}"
                        );
                    }
                }
            }
        }

        // -- Phase 6: Update registry + V2 state --
        Self::set_rename_phase(renames, rename_id, RenamePhase::UpdatingRegistry, None).await;
        for (aid, _, new_name) in &app_infos {
            if let Err(e) = self
                .registry
                .rename_application(aid, new_slug, new_name)
                .await
            {
                error!(app_id = aid.as_str(), "Failed to update registry: {e}");
            }
        }

        // Update V2 state
        {
            let mut state = self.state.write().await;
            for (aid, _, new_container_name) in &app_infos {
                if let Some(c) = state.containers.iter_mut().find(|c| c.id == *aid) {
                    c.slug = new_slug.to_string();
                    c.container_name = new_container_name.clone();
                    if let Some(ref display_name) = new_name {
                        c.name = display_name.clone();
                    }
                }
            }
        }
        let _ = self.save_state().await;

        // -- Phase 7: Start containers --
        Self::set_rename_phase(renames, rename_id, RenamePhase::StartingContainers, None).await;
        for (_, _, new_name) in &app_infos {
            if let Err(e) = NspawnClient::start_container(new_name).await {
                error!(container = new_name.as_str(), "Failed to start renamed container: {e}");
            }
        }

        // Wait for agent reconnection
        Self::set_rename_phase(renames, rename_id, RenamePhase::WaitingForAgent, None).await;
        for (aid, _, _) in &app_infos {
            let mut reconnected = false;
            for _ in 0..30 {
                if self.registry.is_agent_connected(aid).await {
                    reconnected = true;
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            if !reconnected {
                warn!(app_id = aid.as_str(), "Agent did not reconnect within 60s after rename");
            }
        }

        // -- Phase 8: Cleanup old resources --
        Self::set_rename_phase(renames, rename_id, RenamePhase::CleaningUp, None).await;

        // Delete old certificate (best-effort)
        {
            let acme_guard = self.registry.acme.read().await;
            if let Some(ref acme) = *acme_guard {
                if let Err(e) = acme.delete_app_certificate(old_slug) {
                    warn!(slug = old_slug, error = %e, "Failed to delete old certificate");
                }
            }
        }

        // Delete old DNS records (best-effort)
        if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
            if let Err(e) = hr_registry::cloudflare::delete_app_wildcard_dns(
                token,
                zone_id,
                old_slug,
                &self.env.base_domain,
            )
            .await
            {
                warn!(slug = old_slug, error = %e, "Failed to delete old DNS records");
            }
        }

        // -- Phase 9: Complete --
        Self::set_rename_phase(renames, rename_id, RenamePhase::Complete, None).await;

        info!(
            rename_id,
            old_slug,
            new_slug,
            app_count = app_ids.len(),
            "Slug rename complete"
        );
        Ok(())
    }

    // ── Rename helpers ──────────────────────────────────────────

    async fn set_rename_phase(
        renames: &Arc<RwLock<HashMap<String, RenameState>>>,
        rename_id: &str,
        phase: RenamePhase,
        error: Option<String>,
    ) {
        let mut map = renames.write().await;
        if let Some(state) = map.get_mut(rename_id) {
            state.phase = phase;
            state.error = error;
        }
    }

    /// Get the public IPv6 address of a network interface.
    fn get_public_ipv6(interface: &str) -> Result<String, String> {
        let output = std::process::Command::new("ip")
            .args(["-6", "addr", "show", interface, "scope", "global"])
            .output()
            .map_err(|e| format!("Failed to get IPv6: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let line = line.trim();
            if line.starts_with("inet6") && !line.contains("deprecated") {
                if let Some(addr) = line.split_whitespace().nth(1) {
                    if let Some(ip) = addr.split('/').next() {
                        return Ok(ip.to_string());
                    }
                }
            }
        }
        Err(format!("No public IPv6 found on interface {interface}"))
    }

    /// Rollback: reverse filesystem renames. Each tuple is (current_path, original_path).
    async fn rollback_renames(&self, renames: &[(PathBuf, PathBuf)]) {
        for (current, original) in renames.iter().rev() {
            if current.exists() {
                if let Err(e) = tokio::fs::rename(current, original).await {
                    error!(
                        from = %current.display(),
                        to = %original.display(),
                        "Rollback rename failed: {e}"
                    );
                }
            }
        }
    }

    /// Rollback: restart stopped containers.
    async fn rollback_start(&self, containers: &[String]) {
        for name in containers {
            if let Err(e) = NspawnClient::start_container(name).await {
                error!(container = name.as_str(), "Rollback start failed: {e}");
            }
        }
    }

    /// Rollback: delete certificate for a slug.
    async fn rollback_cert(&self, slug: &str) {
        let acme_guard = self.registry.acme.read().await;
        if let Some(ref acme) = *acme_guard {
            if let Err(e) = acme.delete_app_certificate(slug) {
                warn!(slug, error = %e, "Rollback: failed to delete certificate");
            }
        }
    }

    /// Rollback: delete DNS records for a slug.
    async fn rollback_dns(&self, slug: &str) {
        if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
            if let Err(e) = hr_registry::cloudflare::delete_app_wildcard_dns(
                token,
                zone_id,
                slug,
                &self.env.base_domain,
            )
            .await
            {
                warn!(slug, error = %e, "Rollback: failed to delete DNS");
            }
        }
    }
}
