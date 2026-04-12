use hr_acme::AcmeManager;
use hr_auth::AuthService;
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_common::service_registry::SharedServiceRegistry;
use hr_common::task_store::TaskStore;
use hr_git::GitService;
use hr_ipc::orchestrator::OrchestratorClient;
use hr_ipc::{EdgeClient, NetcoreClient};

use hr_common::logging::LogStore;
use hr_registry::AgentRegistry;
use std::path::PathBuf;
use std::sync::Arc;

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

    /// Git repository service.
    pub git: Option<Arc<GitService>>,

    /// Update audit log.
    pub update_log: Arc<crate::routes::updates::UpdateAuditLog>,

    /// Task queue store (SQLite).
    pub task_store: Arc<TaskStore>,

    /// Centralized log store.
    pub log_store: Arc<LogStore>,

    /// Path to dns-dhcp-config.json
    pub dns_dhcp_config_path: PathBuf,
    /// Path to rust-proxy-config.json
    pub proxy_config_path: PathBuf,
    /// Path to reverseproxy-config.json
    pub reverseproxy_config_path: PathBuf,
}
