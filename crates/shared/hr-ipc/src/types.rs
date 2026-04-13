use serde::{Deserialize, Serialize};

// ── IPC Request (client → hr-netcore) ───────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    ReloadConfig,
    DnsCacheStats,
    DnsStatus,
    DnsStaticRecords,
    DnsAddStaticRecord {
        name: String,
        record_type: String,
        value: String,
        ttl: u32,
    },
    DnsRemoveStaticRecordsByValue {
        value: String,
    },
    DhcpLeases,
    AdblockStats,
    AdblockWhitelistList,
    AdblockWhitelistAdd {
        domain: String,
    },
    AdblockWhitelistRemove {
        domain: String,
    },
    AdblockUpdate,
    AdblockSearch {
        query: String,
        limit: Option<usize>,
    },
    ServiceStatus,
}

// ── IPC Response (hr-netcore → client) ──────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl IpcResponse {
    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            error: None,
            data: None,
        }
    }

    pub fn ok_data(data: impl Serialize) -> Self {
        Self {
            ok: true,
            error: None,
            data: Some(serde_json::to_value(data).unwrap_or_default()),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            data: None,
        }
    }
}

// ── DTOs (Data Transfer Objects) ────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct LeaseInfo {
    pub ip: String,
    pub mac: String,
    pub hostname: Option<String>,
    pub expiry: u64,
    pub client_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DnsCacheStatsData {
    pub cache_size: usize,
    pub adblock_enabled: bool,
    pub adblock_domains: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DnsStatusData {
    pub active: bool,
    pub port: u16,
    pub upstream_servers: Vec<String>,
    pub cache_size: usize,
    pub local_domain: String,
    pub adblock_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StaticRecordDto {
    pub name: String,
    pub record_type: String,
    pub value: String,
    pub ttl: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DnsStaticRecordsData {
    pub records: Vec<StaticRecordDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdblockSourceInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdblockStatsData {
    pub domain_count: usize,
    pub enabled: bool,
    pub sources: Vec<AdblockSourceInfo>,
    pub last_update: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdblockUpdateResult {
    pub total_domains: usize,
    pub sources: Vec<AdblockSourceResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdblockSourceResult {
    pub name: String,
    pub domains: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdblockSearchResult {
    pub query: String,
    pub is_blocked: bool,
    pub results: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatusEntry {
    pub name: String,
    pub state: String,
    pub priority: String,
    pub restart_count: u32,
    pub last_state_change: u64,
    pub error: Option<String>,
}

// ── App* DTOs (parallel to hr-apps types, no crate dep) ────────

/// Application summary returned by app_list / app_get IPC calls.
/// This mirrors `hr_apps::types::Application` to avoid a dependency cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationDto {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub stack: String,
    #[serde(default)]
    pub has_db: bool,
    #[serde(default)]
    pub visibility: String,
    pub domain: String,
    pub port: u16,
    pub run_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_artefact: Option<String>,
    pub health_path: String,
    #[serde(default)]
    pub env_vars: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppListData {
    pub apps: Vec<ApplicationDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatusData {
    pub slug: String,
    pub pid: Option<u32>,
    pub state: String,
    pub port: u16,
    pub uptime_secs: u64,
    pub restart_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLogsData {
    pub slug: String,
    pub logs: Vec<AppLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDbTableColumn {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub unique: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDbRelation {
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub display_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDbTableSchema {
    pub name: String,
    pub columns: Vec<AppDbTableColumn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<AppDbRelation>,
    pub row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDbTablesData {
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDbQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
    pub total: u64,
}

