use hr_adblock::AdblockEngine;
use hr_auth::AuthService;
use hr_acme::AcmeManager;
use hr_common::config::EnvConfig;
use hr_common::events::{EventBus, MigrationPhase};
use hr_common::service_registry::SharedServiceRegistry;
use hr_dns::SharedDnsState;
use hr_dhcp::SharedDhcpState;
use hr_firewall::FirewallEngine;
use hr_proxy::{ProxyState, TlsManager};
use hr_registry::AgentRegistry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory state of an active migration.
#[derive(Debug, Clone, serde::Serialize)]
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
}

/// Shared application state for all API routes.
#[derive(Clone)]
pub struct ApiState {
    pub auth: Arc<AuthService>,
    pub acme: Arc<AcmeManager>,
    pub proxy: Arc<ProxyState>,
    pub tls_manager: Arc<TlsManager>,
    pub dns: SharedDnsState,
    pub dhcp: SharedDhcpState,
    pub adblock: Arc<RwLock<AdblockEngine>>,
    pub events: Arc<EventBus>,
    pub env: Arc<EnvConfig>,
    pub service_registry: SharedServiceRegistry,
    pub firewall: Option<Arc<FirewallEngine>>,
    pub registry: Option<Arc<AgentRegistry>>,

    /// Active migrations keyed by transfer_id.
    pub migrations: Arc<RwLock<HashMap<String, MigrationState>>>,

    /// Path to dns-dhcp-config.json
    pub dns_dhcp_config_path: PathBuf,
    /// Path to rust-proxy-config.json
    pub proxy_config_path: PathBuf,
    /// Path to reverseproxy-config.json
    pub reverseproxy_config_path: PathBuf,
}
