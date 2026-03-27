use serde::{Deserialize, Serialize};
use crate::types::IpcResponse;

// ── OrchestratorRequest (client -> hr-orchestrator) ──────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    // ── Applications ─────────────────────────────────────────
    ListApplications,
    GetApplication { id: String },
    IsAgentConnected { app_id: String },

    // ── Applications extended ─────────────────────────────────
    UpdateApplication { id: String, request: serde_json::Value },
    DeleteApplication { id: String },
    ExecInContainer { app_id: String, commands: Vec<String> },
    ExecRemoteContainer { host_id: String, container_name: String, commands: Vec<String> },
    SendToAgent { app_id: String, message: serde_json::Value },
    TriggerAgentUpdate { agent_ids: Option<Vec<String>> },
    GetAgentUpdateStatus,
    FixAgentUpdate { app_id: String },
    UpdateAgentRules { app_ids: Option<Vec<String>> },
    // ── Container V2 (nspawn) ────────────────────────────────
    ListContainers,
    GetContainer { id: String },
    CreateContainer { request: serde_json::Value },
    StartContainer { id: String },
    StopContainer { id: String },
    DeleteContainer { id: String },
    UpdateContainer { id: String, request: serde_json::Value },

    // ── Container volumes ─────────────────────────────────────
    ListVolumes { container_id: String },
    AttachVolume { container_id: String, volume: serde_json::Value },
    UpdateVolume { container_id: String, volume_id: String, updates: serde_json::Value },
    DetachVolume { container_id: String, volume_id: String },

    // ── Container extended ────────────────────────────────────
    MigrateContainer { id: String, target_host_id: String },
    GetMigrationStatus { app_id: String },
    CancelMigration { app_id: String },
    RenameContainer { id: String, request: serde_json::Value },
    GetRenameStatus { app_id: String },
    GetContainerConfig,
    UpdateContainerConfig { config: serde_json::Value },

    // ── Git ──────────────────────────────────────────────────
    ListRepos,
    GetRepo { slug: String },
    CreateRepo { slug: String },
    DeleteRepo { slug: String },

    // ── Git extended ──────────────────────────────────────────
    GetCommits { slug: String, limit: usize },
    GetBranches { slug: String },
    TriggerSync { slug: String },
    SyncAll,
    GetSshKey,
    GenerateSshKey,
    GetGitConfig,
    UpdateGitConfig { config: serde_json::Value },

    // ── Dataverse ────────────────────────────────────────────
    DataverseQuery { app_id: String, query: serde_json::Value },
    DataverseGetSchema { app_id: String },

    // ── Dataverse extended ───────────────────────────────────
    DataverseOverview,

    // ── Host operations ──────────────────────────────────────
    ListHostConnections,
    IsHostConnected { host_id: String },
    GetHostPowerState { host_id: String },
    SendHostCommand { host_id: String, command: serde_json::Value },
    WakeHost { host_id: String },
    HostPowerAction { host_id: String, action: String },

    // ── Updates scan ─────────────────────────────────────────
    ScanUpdates,
    GetScanResults,
    StoreScanResult { target: serde_json::Value },

    // ── Backup pipeline ─────────────────────────────────────
    /// Trigger the incremental SSH backup pipeline (4 repos: homeroute, pixel, containers, git).
    TriggerBackup,
    /// Get the current backup pipeline status and last run result.
    GetBackupStatus,
    /// Get per-repo backup status (last backup time, success, snapshot ID, etc.).
    GetBackupRepos,
    /// Get backup job history (last 20 jobs, most recent first).
    GetBackupJobs,
    /// Get live backup progress for the currently running repo/phase.
    GetBackupProgress,
    /// Cancel the currently running backup pipeline.
    CancelBackup,
    /// Get all backup data in a single call (status + repos + jobs + progress).
    GetBackupLive,

    // ── Agent metrics ────────────────────────────────────────
    /// Get all current agent metrics (lightweight, for polling by homeroute).
    GetAgentMetrics,

    // ── Agent auth (for hr-api cert distribution) ────────────
    /// Authenticate an agent by its bearer token.
    /// Returns {app_id, slug} on success.
    AuthenticateAgentToken { token: String },

    // ── Environments ─────────────────────────────────────────
    ListEnvironments,
    GetEnvironment { id: String },
    CreateEnvironment { request: serde_json::Value },
    DeleteEnvironment { id: String },
    StartEnvironment { id: String },
    StopEnvironment { id: String },

    // ── Environment sub-resources ──────────────────────────
    /// List apps deployed in an environment.
    GetEnvironmentApps { env_slug: String },
    /// Get monitoring data for an environment (CPU, memory, disk, app health).
    GetEnvironmentMonitoring { env_slug: String },
    /// Control an app inside an environment (start, stop, restart).
    ControlEnvironmentApp { env_slug: String, app_slug: String, action: String },
    /// Get recent logs for an app in an environment.
    GetEnvironmentAppLogs { env_slug: String, app_slug: String, lines: Option<u64> },
    /// List database tables in an environment.
    GetEnvironmentDbTables { env_slug: String },
    /// Query data from an environment database.
    QueryEnvironmentDb { env_slug: String, query: serde_json::Value },
    /// Get a summary of all environments' monitoring data.
    GetEnvironmentsMonitoringSummary,

    // ── Multi-host environments (7.6) ─────────────────────
    /// List environments running on a specific host.
    ListEnvironmentsByHost { host_id: String },
    /// Get capacity info for a host (env count, running count, etc.).
    GetHostCapacity { host_id: String },
}

// ── OrchestratorClient ───────────────────────────────────────

use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::Result;

/// IPC client for communicating with hr-orchestrator via Unix socket.
#[derive(Clone)]
pub struct OrchestratorClient {
    socket_path: PathBuf,
}

impl OrchestratorClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a request with the default timeout (30s -- orchestrator operations can be slow).
    pub async fn request(&self, req: &OrchestratorRequest) -> Result<IpcResponse> {
        crate::transport::request(&self.socket_path, req, Duration::from_secs(30)).await
    }

    /// Send a request with 120s timeout (for long operations like deploy, migrate, sync).
    pub async fn request_long(&self, req: &OrchestratorRequest) -> Result<IpcResponse> {
        crate::transport::request(&self.socket_path, req, Duration::from_secs(120)).await
    }

    /// Send a request with a custom timeout.
    pub async fn request_with_timeout(
        &self,
        req: &OrchestratorRequest,
        timeout: Duration,
    ) -> Result<IpcResponse> {
        crate::transport::request(&self.socket_path, req, timeout).await
    }
}
