use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Bus d'événements pour la communication inter-services
pub struct EventBus {
    /// Changements de statut hôtes (monitoring → websocket)
    pub host_status: broadcast::Sender<HostStatusEvent>,
    /// Legacy alias (monitoring still emits both during transition)
    pub server_status: broadcast::Sender<ServerStatusEvent>,
    /// Notifications de changement de config (API → services pour reload)
    pub config_changed: broadcast::Sender<ConfigChangeEvent>,
    /// System update events (updates → websocket)
    pub updates: broadcast::Sender<UpdateEvent>,
    /// Agent status change events (registry → websocket)
    pub agent_status: broadcast::Sender<AgentStatusEvent>,
    /// Agent metrics events (registry → websocket)
    pub agent_metrics: broadcast::Sender<AgentMetricsEvent>,
    /// Service command completion events (registry → websocket)
    pub service_command: broadcast::Sender<ServiceCommandEvent>,
    /// Agent update events (registry → websocket)
    pub agent_update: broadcast::Sender<AgentUpdateEvent>,
    /// Migration progress events (API → websocket)
    pub migration_progress: broadcast::Sender<MigrationProgressEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            host_status: broadcast::channel(64).0,
            server_status: broadcast::channel(64).0,
            config_changed: broadcast::channel(16).0,
            updates: broadcast::channel(256).0,
            agent_status: broadcast::channel(64).0,
            agent_metrics: broadcast::channel(64).0,
            service_command: broadcast::channel(64).0,
            agent_update: broadcast::channel(64).0,
            migration_progress: broadcast::channel(64).0,
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostStatusEvent {
    pub host_id: String,
    pub status: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatusEvent {
    pub server_id: String,
    pub status: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigChangeEvent {
    ProxyRoutes,
    DnsDhcp,
    Adblock,
    Users,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusEvent {
    pub app_id: String,
    pub slug: String,
    pub status: String,
    /// Optional step description for deployment progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum UpdateEvent {
    Started,
    Phase { phase: String, message: String },
    Output { line: String },
    AptComplete { packages: Vec<serde_json::Value>, security_count: usize },
    SnapComplete { snaps: Vec<serde_json::Value> },
    NeedrestartComplete(serde_json::Value),
    Complete { success: bool, summary: serde_json::Value, duration: u64 },
    Cancelled,
    Error { error: String },
    UpgradeStarted { upgrade_type: String },
    UpgradeOutput { line: String },
    UpgradeComplete { upgrade_type: String, success: bool, duration: u64, error: Option<String> },
    UpgradeCancelled,
}

/// Agent metrics event (registry → websocket for frontend display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetricsEvent {
    pub app_id: String,
    pub code_server_status: String,
    pub app_status: String,
    pub db_status: String,
    pub memory_bytes: u64,
    pub cpu_percent: f32,
    pub code_server_idle_secs: u64,
    pub app_idle_secs: u64,
}

/// Service command completion event (registry → websocket).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceCommandEvent {
    pub app_id: String,
    pub service_type: String,
    pub action: String,
    pub success: bool,
}

/// Agent update status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentUpdateStatus {
    /// Update message sent to agent.
    Notified,
    /// Agent reconnected after update.
    Reconnected,
    /// Agent version verified as expected.
    VersionVerified,
    /// Update failed (agent did not reconnect or wrong version).
    Failed,
}

/// Agent update event (registry → websocket for update progress).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUpdateEvent {
    pub app_id: String,
    pub slug: String,
    pub status: AgentUpdateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Migration progress event (API → websocket for frontend display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationProgressEvent {
    pub app_id: String,
    pub transfer_id: String,
    pub phase: MigrationPhase,
    pub progress_pct: u8,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Phase of an LXC container migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Stopping,
    Exporting,
    Transferring,
    Importing,
    Starting,
    Complete,
    Failed,
}
