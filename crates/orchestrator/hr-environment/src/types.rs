use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Type of environment — determines permissions and behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvType {
    /// Full access: code editing, DB writes, build, run, promote.
    Development,
    /// Pre-prod: no code editing, DB writes via tests only, promote to prod.
    Acceptance,
    /// Locked: read-only code, DB writes via pipeline only, rollback only.
    Production,
}

impl std::fmt::Display for EnvType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Development => write!(f, "dev"),
            Self::Acceptance => write!(f, "acc"),
            Self::Production => write!(f, "prod"),
        }
    }
}

/// An environment managed by HomeRoute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentRecord {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name (e.g., "Development", "Production").
    pub name: String,
    /// URL-safe slug (e.g., "dev", "prod", "acc").
    pub slug: String,
    /// Environment type — determines permissions.
    pub env_type: EnvType,
    /// Host this environment runs on (e.g., "medion", "local").
    pub host_id: String,
    /// Container name for nspawn (e.g., "env-dev", "env-prod").
    pub container_name: String,
    /// IPv4 address of the container.
    pub ipv4_address: Option<Ipv4Addr>,
    /// Current status.
    pub status: EnvStatus,
    /// Whether the env-agent is connected via WebSocket.
    pub agent_connected: bool,
    /// env-agent version.
    pub agent_version: Option<String>,
    /// Last heartbeat from the env-agent.
    pub last_heartbeat: Option<DateTime<Utc>>,
    /// Apps deployed in this environment.
    pub apps: Vec<EnvApp>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// CPU usage percentage (from env-agent metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f32>,
    /// Memory used in bytes (from env-agent metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_used_bytes: Option<u64>,
    /// Memory total in bytes (from env-agent host metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_total_bytes: Option<u64>,
    /// Disk used in bytes (from env-agent metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_used_bytes: Option<u64>,
    /// Disk total in bytes (from env-agent metrics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_total_bytes: Option<u64>,
}

/// Status of an environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvStatus {
    /// Container created, env-agent not yet connected.
    Pending,
    /// Provisioning in progress.
    Provisioning,
    /// env-agent connected and healthy.
    Running,
    /// env-agent disconnected or heartbeat stale.
    Disconnected,
    /// Stopped intentionally.
    Stopped,
    /// Unrecoverable error.
    Error,
}

/// An application within an environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvApp {
    /// Application slug (e.g., "trader", "wallet").
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Technology stack.
    pub stack: AppStackType,
    /// Port the app listens on inside the env container.
    pub port: u16,
    /// Current version deployed in this env.
    pub version: Option<String>,
    /// Whether the app process is running.
    pub running: bool,
    /// Whether this app has a Dataverse database.
    pub has_db: bool,
}

/// Technology stack for an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppStackType {
    /// Next.js with custom server + WebSocket support — public/SEO apps.
    #[default]
    NextJs,
    /// Rust Axum backend + Vite/React frontend — authenticated/perf-sensitive apps.
    AxumVite,
    /// Pure Rust Axum backend — API-only services, no frontend.
    Axum,
}

impl AppStackType {
    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::NextJs => "Next.js",
            Self::AxumVite => "Axum + Vite/React",
            Self::Axum => "Axum",
        }
    }

    /// Default build command for this stack.
    pub fn default_build_command(&self) -> &'static str {
        match self {
            Self::NextJs => "pnpm build",
            Self::AxumVite => "cd server && cargo build --release && cd ../client && pnpm build",
            Self::Axum => "cargo build --release",
        }
    }

    /// Default run command (serves the built app — same in all env types).
    pub fn default_run_command(&self) -> &'static str {
        match self {
            Self::NextJs => "node server.js",
            Self::AxumVite => "./server/target/release/{slug}",
            Self::Axum => "./target/release/{slug}",
        }
    }

    /// Default watch command for dev (rebuild on file changes).
    pub fn default_watch_command(&self) -> &'static str {
        match self {
            Self::NextJs => "pnpm dev",
            Self::AxumVite => "cd client && pnpm dev & cd server && cargo watch -x run",
            Self::Axum => "cargo watch -x run",
        }
    }

    /// Default health check path.
    pub fn default_health_path(&self) -> &'static str {
        match self {
            Self::NextJs | Self::AxumVite | Self::Axum => "/api/health",
        }
    }

    /// Brief description of the project structure for this stack.
    pub fn project_structure(&self) -> &'static str {
        match self {
            Self::NextJs => "app/ (pages+API), components/, lib/, server.ts",
            Self::AxumVite => "server/ (Rust Axum) + client/ (Vite/React/TypeScript)",
            Self::Axum => "src/ (Rust Axum), Cargo.toml",
        }
    }
}

/// Permissions derived from the environment type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvPermissions {
    /// Can modify source code.
    pub code_edit: bool,
    /// Can build and run locally.
    pub build_run: bool,
    /// Can modify DB schema.
    pub db_schema_write: bool,
    /// Can insert/update/delete data.
    pub db_data_write: bool,
    /// Can read data (SELECT).
    pub db_data_read: bool,
    /// Can view logs.
    pub logs_read: bool,
    /// Can trigger pipeline promotions.
    pub pipeline_promote: bool,
    /// Can rollback.
    pub pipeline_rollback: bool,
    /// Can modify env vars.
    pub env_vars_write: bool,
}

impl EnvPermissions {
    pub fn for_type(env_type: EnvType) -> Self {
        match env_type {
            EnvType::Development => Self {
                code_edit: true,
                build_run: true,
                db_schema_write: true,
                db_data_write: true,
                db_data_read: true,
                logs_read: true,
                pipeline_promote: true,
                pipeline_rollback: false,
                env_vars_write: true,
            },
            EnvType::Acceptance => Self {
                code_edit: false,
                build_run: false,
                db_schema_write: false,
                db_data_write: true, // via tests
                db_data_read: true,
                logs_read: true,
                pipeline_promote: true,
                pipeline_rollback: true,
                env_vars_write: true,
            },
            EnvType::Production => Self {
                code_edit: false,
                build_run: false,
                db_schema_write: false,
                db_data_write: false, // pipeline only
                db_data_read: true,
                logs_read: true,
                pipeline_promote: false,
                pipeline_rollback: true,
                env_vars_write: false, // pipeline only
            },
        }
    }
}

/// Host capability for multi-host env distribution.
/// Used by the orchestrator to select the best host when creating new environments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCapability {
    /// Unique host identifier (e.g., "medion", "cloudmaster").
    pub host_id: String,
    /// Human-readable hostname.
    pub hostname: String,
    /// Maximum number of environments this host can support.
    pub max_environments: u32,
    /// Current number of environments running on this host.
    pub current_environments: u32,
    /// Available RAM in megabytes.
    pub available_memory_mb: u64,
    /// Available disk space in gigabytes.
    pub available_disk_gb: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_type_display() {
        assert_eq!(EnvType::Development.to_string(), "dev");
        assert_eq!(EnvType::Acceptance.to_string(), "acc");
        assert_eq!(EnvType::Production.to_string(), "prod");
    }

    #[test]
    fn test_permissions_dev() {
        let perms = EnvPermissions::for_type(EnvType::Development);
        assert!(perms.code_edit);
        assert!(perms.build_run);
        assert!(perms.db_schema_write);
        assert!(perms.db_data_write);
    }

    #[test]
    fn test_permissions_prod_locked() {
        let perms = EnvPermissions::for_type(EnvType::Production);
        assert!(!perms.code_edit);
        assert!(!perms.build_run);
        assert!(!perms.db_schema_write);
        assert!(!perms.db_data_write);
        assert!(perms.db_data_read);
        assert!(perms.logs_read);
        assert!(perms.pipeline_rollback);
    }

    #[test]
    fn test_env_record_serde() {
        let env = EnvironmentRecord {
            id: "env-001".into(),
            name: "Development".into(),
            slug: "dev".into(),
            env_type: EnvType::Development,
            host_id: "medion".into(),
            container_name: "env-dev".into(),
            ipv4_address: Some(Ipv4Addr::new(10, 0, 0, 200)),
            status: EnvStatus::Running,
            agent_connected: true,
            agent_version: Some("0.1.0".into()),
            last_heartbeat: Some(chrono::Utc::now()),
            apps: vec![],
            created_at: chrono::Utc::now(),
            cpu_percent: None,
            memory_used_bytes: None,
            memory_total_bytes: None,
            disk_used_bytes: None,
            disk_total_bytes: None,
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: EnvironmentRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.slug, "dev");
        assert_eq!(parsed.env_type, EnvType::Development);
    }
}
