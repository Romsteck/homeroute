use serde::{Deserialize, Serialize};

use crate::types::{AppStack, Environment, FrontendEndpoint};

// ── Shared Types ────────────────────────────────────────────────

/// Metrics reported by the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// RAM used in bytes.
    pub memory_bytes: u64,
    /// CPU usage percentage (0.0 - 100.0).
    pub cpu_percent: f32,
}

// ── Messages from Agent → Registry ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// Initial authentication when connecting.
    #[serde(rename = "auth")]
    Auth {
        token: String,
        service_name: String,
        version: String,
        /// Agent's IPv4 address (for local DNS A records).
        #[serde(default)]
        ipv4_address: Option<String>,
    },
    /// Periodic health report.
    #[serde(rename = "heartbeat")]
    Heartbeat {
        uptime_secs: u64,
        connections_active: u32,
    },
    /// Agent acknowledges a config push.
    #[serde(rename = "config_ack")]
    ConfigAck { config_version: u64 },
    /// Agent reports an error.
    #[serde(rename = "error")]
    Error { message: String },
    /// Agent reports system and service metrics.
    #[serde(rename = "metrics")]
    Metrics(AgentMetrics),
    /// Agent publishes its routes for reverse proxy registration.
    #[serde(rename = "publish_routes")]
    PublishRoutes {
        routes: Vec<AgentRoute>,
    },
    /// Agent reports its Dataverse schema metadata.
    #[serde(rename = "schema_metadata")]
    SchemaMetadata {
        tables: Vec<SchemaTableInfo>,
        relations: Vec<SchemaRelationInfo>,
        version: u64,
        db_size_bytes: u64,
    },
    /// Agent reports a new/changed IPv4 address (e.g. after container restart).
    #[serde(rename = "ip_update")]
    IpUpdate {
        ipv4_address: String,
    },
    /// Agent responds to a Dataverse query from the registry.
    #[serde(rename = "dataverse_query_result")]
    DataverseQueryResult {
        request_id: String,
        #[serde(default)]
        data: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
    /// Agent requests schemas of all other apps.
    #[serde(rename = "get_dataverse_schemas")]
    GetDataverseSchemas {
        request_id: String,
    },
    /// Agent reports update scan results.
    #[serde(rename = "update_scan_result")]
    UpdateScanResult {
        os_upgradable: u32,
        os_security: u32,
        #[serde(default)]
        claude_cli_installed: Option<String>,
        #[serde(default)]
        claude_cli_latest: Option<String>,
        #[serde(default)]
        code_server_installed: Option<String>,
        #[serde(default)]
        code_server_latest: Option<String>,
        #[serde(default)]
        claude_ext_installed: Option<String>,
        #[serde(default)]
        claude_ext_latest: Option<String>,
        #[serde(default)]
        scan_error: Option<String>,
    },
}

/// A route published by an agent for reverse proxy registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoute {
    pub domain: String,
    pub target_port: u16,
    pub auth_required: bool,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
}

/// Schema metadata reported by agent for Dataverse live view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaTableInfo {
    pub name: String,
    pub slug: String,
    pub columns: Vec<SchemaColumnInfo>,
    pub row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaColumnInfo {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub unique: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRelationInfo {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub relation_type: String,
}

// ── Messages from Registry → Agent ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RegistryMessage {
    /// Response to Auth.
    #[serde(rename = "auth_result")]
    AuthResult {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// The authenticated application's ID (set on success).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        app_id: Option<String>,
    },
    /// Full configuration push.
    #[serde(rename = "config")]
    Config {
        config_version: u64,
        /// Base domain for route construction (e.g., "mynetwk.biz").
        #[serde(default)]
        base_domain: String,
        /// Application slug for route construction.
        #[serde(default)]
        slug: String,
        /// Frontend endpoint configuration.
        #[serde(default)]
        frontend: Option<FrontendEndpoint>,
        /// Application environment (development or production).
        #[serde(default)]
        environment: Environment,
        /// Whether code-server is enabled.
        #[serde(default)]
        code_server_enabled: bool,
        /// Technology stack (leptos-rust or next-js).
        #[serde(default)]
        stack: AppStack,
    },
    /// Agent should self-update.
    #[serde(rename = "update_available")]
    UpdateAvailable {
        version: String,
        download_url: String,
        sha256: String,
    },
    /// Graceful shutdown request.
    #[serde(rename = "shutdown")]
    Shutdown,
    /// Certificate has been renewed; agent should re-pull certs.
    #[serde(rename = "cert_renewal")]
    CertRenewal { slug: String },
    /// Query the agent's Dataverse database (proxy from API).
    #[serde(rename = "dataverse_query")]
    DataverseQuery {
        request_id: String,
        query: DataverseQueryRequest,
    },
    /// Response with schemas of all apps (in response to GetDataverseSchemas).
    #[serde(rename = "dataverse_schemas")]
    DataverseSchemas {
        request_id: String,
        schemas: Vec<AppSchemaOverview>,
    },
    /// Push updated .claude/rules/ files to the agent.
    #[serde(rename = "update_rules")]
    UpdateRules {
        /// Vec of (filename, content) pairs, e.g. [("homeroute-deploy.md", "# Deploy...")]
        rules: Vec<(String, String)>,
    },
    /// Registry asks agent to run an update scan.
    #[serde(rename = "run_update_scan")]
    RunUpdateScan,
    /// Registry asks agent to perform a specific upgrade.
    #[serde(rename = "run_upgrade")]
    RunUpgrade {
        category: String,
    },
}

// ── Dataverse Query Types ────────────────────────────────────────

/// A query request proxied from the API to an agent's Dataverse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum DataverseQueryRequest {
    #[serde(rename = "query_rows")]
    QueryRows {
        table_name: String,
        #[serde(default)]
        filters: Vec<serde_json::Value>,
        #[serde(default = "default_query_limit")]
        limit: u64,
        #[serde(default)]
        offset: u64,
        #[serde(default)]
        order_by: Option<String>,
        #[serde(default)]
        order_desc: bool,
    },
    #[serde(rename = "insert_rows")]
    InsertRows {
        table_name: String,
        rows: Vec<serde_json::Value>,
    },
    #[serde(rename = "update_rows")]
    UpdateRows {
        table_name: String,
        updates: serde_json::Value,
        filters: Vec<serde_json::Value>,
    },
    #[serde(rename = "delete_rows")]
    DeleteRows {
        table_name: String,
        filters: Vec<serde_json::Value>,
    },
    #[serde(rename = "count_rows")]
    CountRows {
        table_name: String,
        #[serde(default)]
        filters: Vec<serde_json::Value>,
    },
    #[serde(rename = "get_migrations")]
    GetMigrations,
}

fn default_query_limit() -> u64 {
    100
}

/// Overview of another app's schema (for inter-app visibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSchemaOverview {
    pub app_id: String,
    pub slug: String,
    pub tables: Vec<SchemaTableInfo>,
    pub relations: Vec<SchemaRelationInfo>,
    pub version: u64,
}

// ── Host Agent Protocol ──────────────────────────────────────────────────

/// Messages from host-agent → registry (via WebSocket)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum HostAgentMessage {
    Auth {
        token: String,
        host_name: String,
        version: String,
        #[serde(default)]
        lan_interface: Option<String>,
        #[serde(default)]
        container_storage_path: Option<String>,
        #[serde(default)]
        role: Option<HostRole>,
    },
    Heartbeat {
        uptime_secs: u64,
        containers_running: u32,
    },
    Metrics(HostMetrics),
    ContainerList(Vec<ContainerInfo>),
    ExportReady {
        transfer_id: String,
        #[serde(default)]
        container_name: String,
        size_bytes: u64,
    },
    /// Binary chunk announcement — the actual data follows as a WebSocket Binary frame.
    TransferChunkBinary {
        transfer_id: String,
        sequence: u32,
        size: u32,
        checksum: u32, // xxhash32
    },
    WorkspaceReady {
        transfer_id: String,
        size_bytes: u64,
    },
    TransferComplete {
        transfer_id: String,
    },
    ImportComplete {
        transfer_id: String,
        container_name: String,
    },
    ExportFailed {
        transfer_id: String,
        error: String,
    },
    ImportFailed {
        transfer_id: String,
        error: String,
    },
    ExecResult {
        request_id: String,
        success: bool,
        stdout: String,
        stderr: String,
    },
    NetworkInterfaces(Vec<NetworkInterfaceInfo>),
    /// Nspawn container list reported by host-agent.
    NspawnContainerList(Vec<NspawnContainerInfo>),
    /// Terminal output data from a remote shell session.
    TerminalData {
        session_id: String,
        data: Vec<u8>,
    },
    /// Terminal session opened successfully.
    TerminalOpened {
        session_id: String,
    },
    /// Terminal session closed.
    TerminalClosed {
        session_id: String,
        exit_code: Option<i32>,
    },
    /// Host agent reports OS update scan results.
    UpdateScanResult {
        os_upgradable: u32,
        os_security: u32,
        #[serde(default)]
        scan_error: Option<String>,
    },
    /// Backup: host-agent is ready to receive data.
    BackupRepoReady {
        transfer_id: String,
    },
    /// Backup: repo backup completed on host-agent side.
    BackupRepoComplete {
        transfer_id: String,
        repo_name: String,
        success: bool,
        message: String,
        #[serde(default)]
        snapshot_name: Option<String>,
    },
}

/// Nspawn container info reported by host-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NspawnContainerInfo {
    pub name: String,
    pub status: String,
    pub storage_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub mac: String,
    pub ipv4: Option<String>,
    pub is_up: bool,
}

/// Host system metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMetrics {
    pub cpu_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub load_avg: [f32; 3],
    /// CPU temperature in °C (from thermal zones)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_temp_celsius: Option<f32>,
    /// System uptime in seconds
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<f64>,
    /// ZFS pool statuses
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zfs_pools: Option<Vec<ZfsPoolInfo>>,
    /// Disk usage per mount point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_mounts: Option<Vec<DiskUsage>>,
}

/// ZFS pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZfsPoolInfo {
    pub name: String,
    pub size: u64,
    pub allocated: u64,
    pub free: u64,
    pub health: String,
}

/// Disk usage for a mount point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskUsage {
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

/// Role assigned to a host in the infrastructure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostRole {
    #[default]
    None,
    Dev,
    Prod,
    Backup,
}

/// LXC container info reported by host-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub name: String,
    pub status: String,
    pub ipv4: Option<String>,
}

/// Messages from registry → host-agent (via WebSocket)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum HostRegistryMessage {
    AuthResult {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    CreateContainer {
        app_id: String,
        slug: String,
        config: String,
    },
    DeleteContainer {
        container_name: String,
    },
    StartContainer {
        container_name: String,
    },
    StopContainer {
        container_name: String,
    },
    PushAgentUpdate {
        version: String,
        download_url: String,
        sha256: String,
    },
    Shutdown {
        drain: bool,
    },
    /// Binary chunk announcement — the actual data follows as a WebSocket Binary frame.
    ReceiveChunkBinary {
        transfer_id: String,
        sequence: u32,
        size: u32,
        checksum: u32, // xxhash32
    },
    WorkspaceReady {
        transfer_id: String,
        size_bytes: u64,
    },
    TransferComplete {
        transfer_id: String,
    },
    ExecInContainer {
        request_id: String,
        container_name: String,
        command: Vec<String>,
    },
    ExecOnHost {
        request_id: String,
        command: Vec<String>,
    },
    PowerOff,
    Reboot,
    /// Cancel an in-flight migration transfer.
    CancelTransfer {
        transfer_id: String,
    },
    // ── Nspawn container management ──────────────────────────────
    CreateNspawnContainer {
        app_id: String,
        slug: String,
        container_name: String,
        storage_path: String,
        network_mode: String,
        agent_token: String,
        agent_config: String,
    },
    DeleteNspawnContainer {
        container_name: String,
        storage_path: String,
    },
    StartNspawnContainer {
        container_name: String,
        storage_path: String,
    },
    StopNspawnContainer {
        container_name: String,
    },
    ExecInNspawnContainer {
        request_id: String,
        container_name: String,
        command: Vec<String>,
    },
    StartNspawnExport {
        container_name: String,
        storage_path: String,
        transfer_id: String,
    },
    StartNspawnImport {
        container_name: String,
        storage_path: String,
        transfer_id: String,
        network_mode: String,
    },
    /// Open a terminal session in a container on this host.
    TerminalOpen {
        session_id: String,
        container_name: String,
    },
    /// Terminal input data from the user.
    TerminalData {
        session_id: String,
        data: Vec<u8>,
    },
    /// Close a terminal session.
    TerminalClose {
        session_id: String,
    },
    /// Registry asks host-agent to run an OS update scan.
    RunUpdateScan,
    /// Registry asks host-agent to run an APT upgrade.
    RunAptUpgrade {
        full_upgrade: bool,
    },
    /// Backup: start receiving backup data for a repo.
    StartBackupRepo {
        repo_name: String,
        transfer_id: String,
    },
    /// Backup: all data chunks sent, now sending manifest via binary chunks.
    BackupManifestStart {
        repo_name: String,
        transfer_id: String,
        manifest_size: u64,
    },
    /// Backup: manifest fully sent, finalize the repo backup.
    FinishBackupRepo {
        repo_name: String,
        transfer_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_message_serde() {
        let msg = AgentMessage::Auth {
            token: "abc".into(),
            service_name: "test".into(),
            version: "0.1.0".into(),
            ipv4_address: Some("10.0.0.100".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"auth"#));
        let parsed: AgentMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            AgentMessage::Auth { token, .. } => assert_eq!(token, "abc"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_registry_message_serde() {
        let msg = RegistryMessage::AuthResult {
            success: true,
            error: None,
            app_id: Some("test-123".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: RegistryMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            RegistryMessage::AuthResult { success, .. } => assert!(success),
            _ => panic!("wrong variant"),
        }
    }
}
