use serde::{Deserialize, Serialize};

use crate::types::{AppStackType, EnvApp, EnvPermissions, EnvType};

// ── Messages from env-agent → orchestrator (via WebSocket) ──────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EnvAgentMessage {
    /// Initial authentication when connecting.
    #[serde(rename = "auth")]
    Auth {
        token: String,
        env_slug: String,
        version: String,
        #[serde(default)]
        ipv4_address: Option<String>,
    },

    /// Periodic health report.
    #[serde(rename = "heartbeat")]
    Heartbeat {
        uptime_secs: u64,
        apps_running: u32,
        apps_total: u32,
    },

    /// Report current app states (sent after auth and on changes).
    #[serde(rename = "app_discovery")]
    AppDiscovery { apps: Vec<EnvApp> },

    /// Agent reports system metrics.
    #[serde(rename = "metrics")]
    Metrics {
        #[serde(flatten)]
        data: EnvAgentMetrics,
    },

    /// Agent reports host-level metrics (for multi-host tracking).
    #[serde(rename = "host_metrics")]
    HostMetrics {
        hostname: String,
        total_memory_mb: u64,
        available_memory_mb: u64,
        cpu_usage_percent: f32,
    },

    /// Agent reports an error.
    #[serde(rename = "error")]
    Error { message: String },

    /// Agent reports update scan results.
    #[serde(rename = "update_scan_result")]
    UpdateScanResult {
        os_upgradable: u32,
        os_security: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        claude_cli_installed: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code_server_installed: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        claude_ext_installed: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scan_error: Option<String>,
    },

    /// Agent reports upgrade result.
    #[serde(rename = "upgrade_result")]
    UpgradeResult {
        category: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// App process status changed.
    #[serde(rename = "app_status")]
    AppStatus {
        app_slug: String,
        running: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// Pipeline step completed (for progress tracking).
    #[serde(rename = "pipeline_progress")]
    PipelineProgress {
        pipeline_id: String,
        step: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// DB migration result.
    #[serde(rename = "migration_result")]
    MigrationResult {
        pipeline_id: String,
        app_slug: String,
        success: bool,
        migrations_applied: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// Metrics from the env-agent (environment-level).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvAgentMetrics {
    pub memory_bytes: u64,
    #[serde(default)]
    pub memory_total_bytes: u64,
    pub cpu_percent: f32,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_memory_bytes: u64,
}

// ── Messages from orchestrator → env-agent (via WebSocket) ──────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EnvOrchestratorMessage {
    /// Response to Auth.
    #[serde(rename = "auth_result")]
    AuthResult {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        env_id: Option<String>,
    },

    /// Push environment configuration.
    #[serde(rename = "config")]
    Config {
        env_slug: String,
        env_type: EnvType,
        base_domain: String,
        permissions: EnvPermissions,
        apps: Vec<EnvAppConfig>,
    },

    /// Deploy an app (from pipeline).
    #[serde(rename = "deploy_app")]
    DeployApp {
        pipeline_id: String,
        app_slug: String,
        version: String,
        /// URL to download the binary/assets bundle.
        artifact_url: String,
        /// SHA256 hash for verification.
        sha256: String,
    },

    /// Apply DB migrations (from pipeline).
    #[serde(rename = "migrate_db")]
    MigrateDb {
        pipeline_id: String,
        app_slug: String,
        /// SQL migration statements to apply.
        migrations: Vec<String>,
    },

    /// Rollback an app to previous version.
    #[serde(rename = "rollback_app")]
    RollbackApp {
        pipeline_id: String,
        app_slug: String,
    },

    /// Start/stop/restart a specific app process.
    #[serde(rename = "app_control")]
    AppControl {
        app_slug: String,
        action: AppControlAction,
    },

    /// Run an update scan (APT, claude-cli, code-server, etc.).
    #[serde(rename = "run_update_scan")]
    RunUpdateScan,

    /// Run an upgrade for a specific category (apt, claude_cli, code_server, claude_ext).
    #[serde(rename = "run_upgrade")]
    RunUpgrade { category: String },

    /// Graceful shutdown of the entire environment.
    #[serde(rename = "shutdown")]
    Shutdown,

    /// Update env-agent binary.
    #[serde(rename = "update_available")]
    UpdateAvailable {
        version: String,
        download_url: String,
        sha256: String,
    },

    /// Snapshot DB for backup before migration.
    #[serde(rename = "snapshot_db")]
    SnapshotDb {
        pipeline_id: String,
        app_slug: String,
    },

    /// Regenerate Claude Code context files for an app.
    #[serde(rename = "refresh_context")]
    RefreshContext { app_slug: String },

    /// Push host info to the env-agent (for multi-host awareness).
    #[serde(rename = "host_info")]
    HostInfo {
        host_id: String,
        hostname: String,
        total_memory_mb: u64,
        available_memory_mb: u64,
    },
}

/// App control actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppControlAction {
    Start,
    Stop,
    Restart,
}

/// Per-app configuration pushed from orchestrator to env-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvAppConfig {
    pub slug: String,
    pub name: String,
    pub stack: AppStackType,
    pub port: u16,
    /// Git repo URL (from hr-git).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_repo: Option<String>,
    /// Whether this app has a Dataverse database.
    pub has_db: bool,
    /// Build command (used in dev env).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    /// Run command (e.g., "./bin/trader" or "node .next/standalone/server.js").
    pub run_command: String,
    /// Health check path (e.g., "/api/health").
    #[serde(default = "default_health_path")]
    pub health_path: String,
}

fn default_health_path() -> String {
    "/api/health".to_string()
}

// ── MCP Tool Categories ─────────────────────────────────────────
//
// The env-agent exposes MCP tools over HTTP (port 4010).
// Tools are organized by category:
//
// db.*        — Database operations (proxied from hr-db)
//   db.overview, db.list_tables, db.describe_table, db.get_schema,
//   db.get_db_info, db.count_rows, db.create_table, db.add_column,
//   db.remove_column, db.drop_table, db.create_relation,
//   db.query_data, db.insert_data, db.update_data, db.delete_data
//
// app.*       — App lifecycle management
//   app.list, app.status, app.start, app.stop, app.restart,
//   app.logs, app.health, app.env_vars
//
// env.*       — Environment introspection
//   env.info, env.permissions, env.metrics
//
// pipeline.*  — Pipeline operations (delegated from orchestrator)
//   pipeline.promote, pipeline.rollback, pipeline.status, pipeline.history
//
// studio.*    — Studio/Claude Code context management
//   studio.switch_project, studio.refresh_context, studio.get_context

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_agent_message_serde() {
        let msg = EnvAgentMessage::Auth {
            token: "abc123".into(),
            env_slug: "dev".into(),
            version: "0.1.0".into(),
            ipv4_address: Some("10.0.0.200".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"auth"#));
        let parsed: EnvAgentMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            EnvAgentMessage::Auth { env_slug, .. } => assert_eq!(env_slug, "dev"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_orchestrator_message_serde() {
        let msg = EnvOrchestratorMessage::AppControl {
            app_slug: "trader".into(),
            action: AppControlAction::Restart,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: EnvOrchestratorMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            EnvOrchestratorMessage::AppControl { app_slug, action } => {
                assert_eq!(app_slug, "trader");
                assert_eq!(action, AppControlAction::Restart);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_deploy_app_message() {
        let msg = EnvOrchestratorMessage::DeployApp {
            pipeline_id: "pipe-001".into(),
            app_slug: "trader".into(),
            version: "2.3.1".into(),
            artifact_url: "http://10.0.0.254:4001/artifacts/trader-2.3.1.tar.gz".into(),
            sha256: "abc123def456".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("deploy_app"));
        assert!(json.contains("trader"));
    }
}
