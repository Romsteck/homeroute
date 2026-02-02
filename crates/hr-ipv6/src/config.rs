use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ipv6Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ra_enabled: bool,
    #[serde(default)]
    pub ra_prefix: String,
    #[serde(default = "default_ra_lifetime")]
    pub ra_lifetime_secs: u32,
    #[serde(default)]
    pub ra_managed_flag: bool,
    #[serde(default)]
    pub ra_other_flag: bool,
    #[serde(default)]
    pub dhcpv6_enabled: bool,
    #[serde(default)]
    pub dhcpv6_dns_servers: Vec<String>,
    #[serde(default)]
    pub interface: String,

    // DHCPv6 Prefix Delegation client
    #[serde(default)]
    pub pd_enabled: bool,
    #[serde(default)]
    pub pd_wan_interface: String,
    #[serde(default = "default_pd_prefix_hint_len")]
    pub pd_prefix_hint_len: u8,
    #[serde(default)]
    pub pd_subnet_id: u16,
}

fn default_ra_lifetime() -> u32 { 1800 }
fn default_pd_prefix_hint_len() -> u8 { 56 }

impl Default for Ipv6Config {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

/// Persisted state for a DHCPv6 prefix delegation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdState {
    pub delegated_prefix: String,
    pub delegated_prefix_len: u8,
    pub selected_subnet: String,
    pub server_duid: Vec<u8>,
    pub client_duid: Vec<u8>,
    pub iaid: u32,
    pub t1: u32,
    pub t2: u32,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
    pub obtained_at: u64,
}

impl PdState {
    pub fn state_file_path() -> &'static str {
        "/var/lib/server-dashboard/dhcpv6-pd-state.json"
    }

    pub fn load() -> Option<Self> {
        let data = std::fs::read_to_string(Self::state_file_path()).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(Self::state_file_path(), data)?;
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now < self.obtained_at + self.valid_lifetime as u64
    }
}
