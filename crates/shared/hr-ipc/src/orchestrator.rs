use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── OrchestratorRequest (client -> hr-orchestrator) ──────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    // ── Applications ─────────────────────────────────────────
    ListApplications,
    GetApplication {
        id: String,
    },
    IsAgentConnected {
        app_id: String,
    },

    // ── Applications extended ─────────────────────────────────
    UpdateApplication {
        id: String,
        request: serde_json::Value,
    },
    DeleteApplication {
        id: String,
    },
    ExecInContainer {
        app_id: String,
        commands: Vec<String>,
    },
    ExecRemoteContainer {
        host_id: String,
        container_name: String,
        commands: Vec<String>,
    },
    SendToAgent {
        app_id: String,
        message: serde_json::Value,
    },
    TriggerAgentUpdate {
        agent_ids: Option<Vec<String>>,
    },
    GetAgentUpdateStatus,
    FixAgentUpdate {
        app_id: String,
    },
    UpdateAgentRules {
        app_ids: Option<Vec<String>>,
    },

    // ── Git ──────────────────────────────────────────────────
    ListRepos,
    GetRepo {
        slug: String,
    },
    CreateRepo {
        slug: String,
    },
    DeleteRepo {
        slug: String,
    },

    // ── Git extended ──────────────────────────────────────────
    GetCommits {
        slug: String,
        limit: usize,
    },
    GetBranches {
        slug: String,
    },
    TriggerSync {
        slug: String,
    },
    SyncAll,
    GetSshKey,
    GenerateSshKey,
    GetGitConfig,
    UpdateGitConfig {
        config: serde_json::Value,
    },

    // ── Host operations ──────────────────────────────────────
    ListHostConnections,
    IsHostConnected {
        host_id: String,
    },
    GetHostPowerState {
        host_id: String,
    },
    SendHostCommand {
        host_id: String,
        command: serde_json::Value,
    },
    WakeHost {
        host_id: String,
    },
    HostPowerAction {
        host_id: String,
        action: String,
    },

    // ── Updates scan ─────────────────────────────────────────
    ScanUpdates,
    GetScanResults,
    StoreScanResult {
        target: serde_json::Value,
    },

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
    AuthenticateAgentToken {
        token: String,
    },

    // ── Apps (V3 — direct app supervision via hr-apps) ────
    /// List all applications managed by the AppSupervisor.
    AppList,
    /// Get a single application by slug.
    AppGet {
        slug: String,
    },
    /// Create a new application.
    AppCreate {
        slug: String,
        name: String,
        stack: String,
        has_db: bool,
        visibility: String,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        #[serde(default)]
        build_artefact: Option<String>,
    },
    /// Update an existing application's metadata.
    AppUpdate {
        slug: String,
        name: Option<String>,
        visibility: Option<String>,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        env_vars: Option<BTreeMap<String, String>>,
        #[serde(default)]
        has_db: Option<bool>,
        #[serde(default)]
        build_artefact: Option<String>,
    },
    /// Delete an application. If `keep_data` is true, the DB and source dirs are preserved.
    AppDelete {
        slug: String,
        keep_data: bool,
    },
    /// Control an app process: "start" | "stop" | "restart".
    AppControl {
        slug: String,
        action: String,
    },
    /// Build an app remotely on CloudMaster (rsync source up → SSH build → rsync artefacts back).
    /// Bounded by `timeout_secs` (default 1800, clamped to [60, 7200]).
    AppBuild {
        slug: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Ship pre-built artefacts from CloudMaster to Medion (skip compile).
    /// Used when the agent has already run the build locally on CloudMaster
    /// and just wants to push the artefacts + restart the supervised process.
    AppShip {
        slug: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Broadcast a custom AppBuildEvent (used by local builds via the
    /// app-build skill to notify the Studio's per-app live panel).
    AppEmitBuildEvent {
        slug: String,
        status: String,
        #[serde(default)]
        phase: Option<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        step: Option<u32>,
        #[serde(default)]
        total_steps: Option<u32>,
    },
    /// Get runtime status (pid, state, port, uptime).
    AppStatus {
        slug: String,
    },
    /// Get recent logs for an app.
    AppLogs {
        slug: String,
        limit: Option<usize>,
        level: Option<String>,
    },
    /// Execute a shell command in the context of an app.
    AppExec {
        slug: String,
        command: String,
        timeout_secs: Option<u64>,
    },
    /// Regenerate the Claude context file for an app.
    AppRegenerateContext {
        slug: String,
    },

    // ── App-managed DB (per-app SQLite via DbManager) ─────
    /// List user-defined tables in the app database.
    AppDbListTables {
        slug: String,
    },
    /// Describe a single table (columns, row count).
    AppDbDescribeTable {
        slug: String,
        table: String,
    },
    /// Run a SQL query against the app database.
    AppDbQuery {
        slug: String,
        sql: String,
        params: Vec<serde_json::Value>,
    },
    /// Execute a mutation (INSERT/UPDATE/DELETE) against the app database.
    AppDbExecute {
        slug: String,
        sql: String,
        params: Vec<serde_json::Value>,
    },
    /// Structured query: SELECT rows with filters, pagination, and optional Lookup expansion.
    AppDbQueryRows {
        slug: String,
        table: String,
        #[serde(default)]
        filters: Vec<serde_json::Value>,
        #[serde(default)]
        limit: Option<u64>,
        #[serde(default)]
        offset: Option<u64>,
        #[serde(default)]
        order_by: Option<String>,
        #[serde(default)]
        order_desc: Option<bool>,
        #[serde(default)]
        expand: Vec<String>,
    },
    /// Sync SQLite tables into Dataverse metadata.
    AppDbSyncSchema {
        slug: String,
    },
    /// Get full database schema (all tables + relations + version).
    AppDbGetSchema {
        slug: String,
    },
    /// Create a new table.
    AppDbCreateTable {
        slug: String,
        definition: serde_json::Value,
    },
    /// Drop a table.
    AppDbDropTable {
        slug: String,
        table: String,
    },
    /// Add a column to a table.
    AppDbAddColumn {
        slug: String,
        table: String,
        column: serde_json::Value,
    },
    /// Remove a column from a table.
    AppDbRemoveColumn {
        slug: String,
        table: String,
        column: String,
    },
    /// Create a relation between tables.
    AppDbCreateRelation {
        slug: String,
        relation: serde_json::Value,
    },

    // ── Per-app todos (JSON-backed, live via app_todos event) ─────
    AppTodosList {
        slug: String,
        #[serde(default)]
        status: Option<String>,
    },
    AppTodosCreate {
        slug: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
    },
    AppTodosUpdate {
        slug: String,
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        status: Option<String>,
    },
    AppTodosDelete {
        slug: String,
        id: String,
    },
}

// ── OrchestratorClient ───────────────────────────────────────

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

    // ── App* typed helpers ────────────────────────────────────

    pub async fn app_list(&self) -> Result<AppListData> {
        let resp = self.request(&OrchestratorRequest::AppList).await?;
        extract_data(resp)
    }

    pub async fn app_get(&self, slug: &str) -> Result<ApplicationDto> {
        let resp = self
            .request(&OrchestratorRequest::AppGet {
                slug: slug.to_string(),
            })
            .await?;
        extract_data(resp)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn app_create(
        &self,
        slug: String,
        name: String,
        stack: String,
        has_db: bool,
        visibility: String,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        build_artefact: Option<String>,
    ) -> Result<ApplicationDto> {
        let resp = self
            .request_long(&OrchestratorRequest::AppCreate {
                slug,
                name,
                stack,
                has_db,
                visibility,
                run_command,
                build_command,
                health_path,
                build_artefact,
            })
            .await?;
        extract_data(resp)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn app_update(
        &self,
        slug: String,
        name: Option<String>,
        visibility: Option<String>,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        env_vars: Option<BTreeMap<String, String>>,
        has_db: Option<bool>,
        build_artefact: Option<String>,
    ) -> Result<ApplicationDto> {
        let resp = self
            .request(&OrchestratorRequest::AppUpdate {
                slug,
                name,
                visibility,
                run_command,
                build_command,
                health_path,
                env_vars,
                has_db,
                build_artefact,
            })
            .await?;
        extract_data(resp)
    }

    pub async fn app_delete(&self, slug: &str, keep_data: bool) -> Result<IpcResponse> {
        self.request(&OrchestratorRequest::AppDelete {
            slug: slug.to_string(),
            keep_data,
        })
        .await
    }

    pub async fn app_control(&self, slug: &str, action: &str) -> Result<IpcResponse> {
        self.request_long(&OrchestratorRequest::AppControl {
            slug: slug.to_string(),
            action: action.to_string(),
        })
        .await
    }

    pub async fn app_status(&self, slug: &str) -> Result<AppStatusData> {
        let resp = self
            .request(&OrchestratorRequest::AppStatus {
                slug: slug.to_string(),
            })
            .await?;
        extract_data(resp)
    }

    pub async fn app_logs(
        &self,
        slug: &str,
        limit: Option<usize>,
        level: Option<String>,
    ) -> Result<AppLogsData> {
        let resp = self
            .request(&OrchestratorRequest::AppLogs {
                slug: slug.to_string(),
                limit,
                level,
            })
            .await?;
        extract_data(resp)
    }

    pub async fn app_exec(
        &self,
        slug: &str,
        command: String,
        timeout_secs: Option<u64>,
    ) -> Result<AppExecResult> {
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(60).max(1) + 5);
        let resp = self
            .request_with_timeout(
                &OrchestratorRequest::AppExec {
                    slug: slug.to_string(),
                    command,
                    timeout_secs,
                },
                timeout,
            )
            .await?;
        extract_data(resp)
    }

    pub async fn app_regenerate_context(&self, slug: &str) -> Result<IpcResponse> {
        self.request(&OrchestratorRequest::AppRegenerateContext {
            slug: slug.to_string(),
        })
        .await
    }

    // ── App DB helpers ────────────────────────────────────────

    pub async fn app_db_list_tables(&self, slug: &str) -> Result<AppDbTablesData> {
        let resp = self
            .request(&OrchestratorRequest::AppDbListTables {
                slug: slug.to_string(),
            })
            .await?;
        extract_data(resp)
    }

    pub async fn app_db_describe_table(&self, slug: &str, table: &str) -> Result<AppDbTableSchema> {
        let resp = self
            .request(&OrchestratorRequest::AppDbDescribeTable {
                slug: slug.to_string(),
                table: table.to_string(),
            })
            .await?;
        extract_data(resp)
    }

    pub async fn app_db_query(
        &self,
        slug: &str,
        sql: String,
        params: Vec<serde_json::Value>,
    ) -> Result<AppDbQueryResult> {
        let resp = self
            .request(&OrchestratorRequest::AppDbQuery {
                slug: slug.to_string(),
                sql,
                params,
            })
            .await?;
        extract_data(resp)
    }

}

/// Extract typed data from IpcResponse, returning an error if the response indicates failure.
fn extract_data<T: serde::de::DeserializeOwned>(resp: IpcResponse) -> Result<T> {
    use anyhow::Context;
    if !resp.ok {
        anyhow::bail!(
            "hr-orchestrator error: {}",
            resp.error.unwrap_or_else(|| "unknown error".into())
        );
    }
    let data = resp.data.context("hr-orchestrator returned no data")?;
    Ok(serde_json::from_value(data)?)
}
