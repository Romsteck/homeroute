use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub dns: DnsConfig,
    #[serde(default)]
    pub dhcp: DhcpConfig,
    #[serde(default)]
    pub ipv6: Ipv6Config,
    #[serde(default)]
    pub adblock: AdblockConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsConfig {
    #[serde(default = "default_listen_addresses")]
    pub listen_addresses: Vec<String>,
    #[serde(default = "default_dns_port")]
    pub port: u16,
    #[serde(default = "default_upstream_servers")]
    pub upstream_servers: Vec<String>,
    #[serde(default = "default_upstream_timeout")]
    pub upstream_timeout_ms: u64,
    #[serde(default = "default_cache_size")]
    pub cache_size: usize,
    #[serde(default)]
    pub local_domain: String,
    #[serde(default)]
    pub wildcard_ipv4: String,
    #[serde(default)]
    pub wildcard_ipv6: String,
    #[serde(default)]
    pub static_records: Vec<StaticRecord>,
    #[serde(default = "default_true")]
    pub expand_hosts: bool,
    #[serde(default)]
    pub query_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String,
    #[serde(default = "default_ttl")]
    pub ttl: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub interface: String,
    #[serde(default)]
    pub range_start: String,
    #[serde(default)]
    pub range_end: String,
    #[serde(default = "default_netmask")]
    pub netmask: String,
    #[serde(default)]
    pub gateway: String,
    #[serde(default)]
    pub dns_server: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_lease_time")]
    pub default_lease_time_secs: u64,
    #[serde(default)]
    pub authoritative: bool,
    #[serde(default = "default_lease_file")]
    pub lease_file: String,
    #[serde(default)]
    pub static_leases: Vec<StaticLease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticLease {
    pub mac: String,
    pub ip: String,
    #[serde(default)]
    pub hostname: String,
}

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdblockConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_block_response")]
    pub block_response: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default)]
    pub sources: Vec<AdblockSource>,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default = "default_adblock_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_auto_update_hours")]
    pub auto_update_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdblockSource {
    pub name: String,
    pub url: String,
    #[serde(default = "default_source_format")]
    pub format: String,
}

// Default functions
fn default_listen_addresses() -> Vec<String> {
    vec!["0.0.0.0".to_string()]
}
fn default_dns_port() -> u16 {
    53
}
fn default_upstream_servers() -> Vec<String> {
    vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]
}
fn default_upstream_timeout() -> u64 {
    3000
}
fn default_cache_size() -> usize {
    1000
}
fn default_ttl() -> u32 {
    300
}
fn default_true() -> bool {
    true
}
fn default_netmask() -> String {
    "255.255.255.0".to_string()
}
fn default_lease_time() -> u64 {
    86400
}
fn default_lease_file() -> String {
    "/var/lib/server-dashboard/dhcp-leases".to_string()
}
fn default_ra_lifetime() -> u32 {
    1800
}
fn default_block_response() -> String {
    "zero_ip".to_string()
}
fn default_api_port() -> u16 {
    5380
}
fn default_adblock_data_dir() -> String {
    "/var/lib/server-dashboard/adblock".to_string()
}
fn default_auto_update_hours() -> u64 {
    24
}
fn default_source_format() -> String {
    "hosts".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dns: DnsConfig::default(),
            dhcp: DhcpConfig::default(),
            ipv6: Ipv6Config::default(),
            adblock: AdblockConfig::default(),
        }
    }
}

impl Default for DnsConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Default for DhcpConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Default for Ipv6Config {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Default for AdblockConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Config {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("Failed to write config to {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, path)
            .with_context(|| format!("Failed to rename config to {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.dns.port, 53);
        assert_eq!(config.dns.cache_size, 1000);
        assert!(config.dhcp.enabled);
        assert_eq!(config.adblock.api_port, 5380);
    }

    #[test]
    fn test_roundtrip() {
        let json = r#"{
            "dns": { "port": 5353, "local_domain": "test.lab" },
            "dhcp": { "enabled": true, "range_start": "10.0.0.10" },
            "adblock": { "enabled": true }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.dns.port, 5353);
        assert_eq!(config.dns.local_domain, "test.lab");

        let serialized = serde_json::to_string(&config).unwrap();
        let config2: Config = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config2.dns.port, 5353);
    }
}
