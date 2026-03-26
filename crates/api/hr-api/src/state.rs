use hr_auth::AuthService;
use hr_acme::AcmeManager;
use hr_common::config::EnvConfig;
use hr_common::events::{EventBus, MigrationPhase};
use hr_common::task_store::TaskStore;
use hr_common::service_registry::SharedServiceRegistry;
use hr_ipc::{NetcoreClient, EdgeClient};
use hr_ipc::orchestrator::OrchestratorClient;
use hr_git::GitService;

use hr_registry::AgentRegistry;
use hr_registry::types::Environment;
use crate::container_manager::{ContainerManager, RenameState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::RwLock;

/// In-memory state of an active migration.
#[derive(Debug, serde::Serialize)]
pub struct MigrationState {
    pub app_id: String,
    pub transfer_id: String,
    pub source_host_id: String,
    pub target_host_id: String,
    pub phase: MigrationPhase,
    pub progress_pct: u8,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub error: Option<String>,
    /// Cancel flag: set by the cancel endpoint, checked by the migration task.
    #[serde(skip)]
    pub cancelled: Arc<AtomicBool>,
}

/// Cached Dataverse schema metadata for an application.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedDataverseSchema {
    pub app_id: String,
    pub slug: String,
    pub environment: Environment,
    pub tables: Vec<CachedTableInfo>,
    pub relations: Vec<CachedRelationInfo>,
    pub version: u64,
    pub db_size_bytes: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedTableInfo {
    pub name: String,
    pub slug: String,
    pub columns: Vec<CachedColumnInfo>,
    pub row_count: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedColumnInfo {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub unique: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedRelationInfo {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub relation_type: String,
}

/// Shared application state for all API routes.
#[derive(Clone)]
pub struct ApiState {
    pub auth: Arc<AuthService>,
    pub acme: Arc<AcmeManager>,
    pub edge: Arc<EdgeClient>,
    pub netcore: Arc<NetcoreClient>,
    pub orchestrator: Arc<OrchestratorClient>,
    pub events: Arc<EventBus>,
    pub env: Arc<EnvConfig>,
    pub service_registry: SharedServiceRegistry,

    pub registry: Option<Arc<AgentRegistry>>,

    /// Container V2 manager (nspawn).
    pub container_manager: Option<Arc<ContainerManager>>,

    /// Git repository service.
    pub git: Option<Arc<GitService>>,

    /// Active migrations keyed by transfer_id.
    pub migrations: Arc<RwLock<HashMap<String, MigrationState>>>,

    /// Active slug renames keyed by rename_id.
    pub renames: Arc<RwLock<HashMap<String, RenameState>>>,

    /// Cached Dataverse schemas keyed by app_id.
    pub dataverse_schemas: Arc<RwLock<HashMap<String, CachedDataverseSchema>>>,

    /// Update audit log.
    pub update_log: Arc<crate::routes::updates::UpdateAuditLog>,

    /// Task queue store (SQLite).
    pub task_store: Arc<TaskStore>,

    /// Path to dns-dhcp-config.json
    pub dns_dhcp_config_path: PathBuf,
    /// Path to rust-proxy-config.json
    pub proxy_config_path: PathBuf,
    /// Path to reverseproxy-config.json
    pub reverseproxy_config_path: PathBuf,
}
