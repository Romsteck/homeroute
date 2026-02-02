use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallConfig {
    #[serde(default)]
    pub enabled: bool,

    /// LAN interface where protected clients reside.
    #[serde(default)]
    pub lan_interface: String,

    /// WAN interface (inbound traffic source).
    #[serde(default)]
    pub wan_interface: String,

    /// Default policy for unsolicited inbound to LAN: "drop" or "reject".
    #[serde(default = "default_inbound_policy")]
    pub default_inbound_policy: String,

    /// User-defined allow rules for inbound traffic.
    #[serde(default)]
    pub allow_rules: Vec<FirewallRule>,
}

fn default_inbound_policy() -> String {
    "drop".into()
}

impl Default for FirewallConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub id: String,
    #[serde(default)]
    pub description: String,
    /// "tcp", "udp", "icmpv6", or "any"
    pub protocol: String,
    /// Destination port (0 = any). For tcp/udp only.
    #[serde(default)]
    pub dest_port: u16,
    /// If > 0, creates a port range dest_port-dest_port_end.
    #[serde(default)]
    pub dest_port_end: u16,
    /// Destination IPv6 address or prefix. Empty = any host on LAN.
    #[serde(default)]
    pub dest_address: String,
    /// Source IPv6 address or prefix. Empty = any.
    #[serde(default)]
    pub source_address: String,
    /// Whether the rule is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

impl FirewallConfig {
    const CONFIG_PATH: &'static str = "/var/lib/server-dashboard/firewall-config.json";

    pub fn load() -> Self {
        std::fs::read_to_string(Self::CONFIG_PATH)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(Self::CONFIG_PATH, data)?;
        Ok(())
    }
}
