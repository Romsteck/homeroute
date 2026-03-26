use hr_registry::protocol::HostRole;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub homeroute_url: String,
    pub token: String,
    pub host_name: String,
    #[serde(default = "default_reconnect")]
    pub reconnect_interval_secs: u64,
    /// Physical LAN interface for macvlan (e.g., "enp7s0f0"). Required for container migrations.
    #[serde(default)]
    pub lan_interface: Option<String>,
    /// Storage path for nspawn containers (default: /var/lib/machines).
    #[serde(default)]
    pub container_storage_path: Option<String>,
    /// Role of this host in the infrastructure (dev, prod, backup).
    #[serde(default)]
    pub role: Option<HostRole>,

    // ── MCP Project Manager ──
    #[serde(default)]
    pub mcp_port: Option<u16>,
    #[serde(default)]
    pub mcp_token: Option<String>,
    #[serde(default)]
    pub prod_server: Option<String>,
    #[serde(default)]
    pub prod_user: Option<String>,
    #[serde(default)]
    pub projects_registry_path: Option<String>,
}

fn default_reconnect() -> u64 {
    5
}

impl Config {
    pub fn load(path: &PathBuf) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config {}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))
    }

    pub fn ws_url(&self) -> String {
        format!("ws://{}/api/hosts/agent/ws", self.homeroute_url)
    }

    pub fn mcp_enabled(&self) -> bool {
        self.mcp_port.is_some() && self.mcp_token.is_some()
    }

    pub fn registry_path(&self) -> &str {
        self.projects_registry_path
            .as_deref()
            .unwrap_or("/etc/hr-host-agent/projects.json")
    }
}
