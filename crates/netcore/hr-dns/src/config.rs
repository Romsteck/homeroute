use serde::{Deserialize, Serialize};

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
    /// Some("hr-edge") = auto-généré depuis le reverse proxy.
    /// None = record utilisateur (édité manuellement, à préserver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
}

/// Adblock resolver config: the subset of adblock config that the DNS resolver needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdblockResolverConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_block_response")]
    pub block_response: String,
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
fn default_block_response() -> String {
    "zero_ip".to_string()
}

impl Default for DnsConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Default for AdblockResolverConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

/// Replace the records owned by `owner` in `records` with `new_records`.
/// Records owned by anyone else (including `None` = user records) are preserved.
/// The new records are stamped with `managed_by = Some(owner)`.
///
/// Pure function — extracted from `DnsState::replace_managed_records` to be
/// testable without instantiating a full DnsState.
pub fn replace_managed_in(
    records: &mut Vec<StaticRecord>,
    owner: &str,
    new_records: Vec<StaticRecord>,
) {
    records.retain(|r| r.managed_by.as_deref() != Some(owner));
    records.extend(new_records.into_iter().map(|mut r| {
        r.managed_by = Some(owner.to_string());
        r
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dns_config() {
        let config = DnsConfig::default();
        assert_eq!(config.port, 53);
        assert_eq!(config.cache_size, 1000);
        assert!(config.expand_hosts);
        assert_eq!(config.upstream_servers.len(), 2);
    }

    #[test]
    fn test_roundtrip() {
        let json = r#"{
            "port": 5353,
            "local_domain": "test.lab"
        }"#;
        let config: DnsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, 5353);
        assert_eq!(config.local_domain, "test.lab");

        let serialized = serde_json::to_string(&config).unwrap();
        let config2: DnsConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config2.port, 5353);
    }

    #[test]
    fn test_adblock_resolver_config_defaults() {
        let config = AdblockResolverConfig::default();
        assert!(config.enabled);
        assert_eq!(config.block_response, "zero_ip");
    }

    fn rec(name: &str, value: &str, managed_by: Option<&str>) -> StaticRecord {
        StaticRecord {
            name: name.to_string(),
            record_type: "A".to_string(),
            value: value.to_string(),
            ttl: 60,
            managed_by: managed_by.map(|s| s.to_string()),
        }
    }

    #[test]
    fn replace_managed_records_preserves_user_records() {
        let mut records = vec![
            rec("user1.test.lab", "10.0.0.1", None),
            rec("auto1.test.lab", "10.0.0.254", Some("hr-edge")),
        ];

        replace_managed_in(
            &mut records,
            "hr-edge",
            vec![rec("auto2.test.lab", "10.0.0.254", None)],
        );

        assert_eq!(records.len(), 2, "user record + new managed record");
        assert!(records.iter().any(|r| r.name == "user1.test.lab" && r.managed_by.is_none()));
        assert!(records.iter().any(|r| r.name == "auto2.test.lab" && r.managed_by.as_deref() == Some("hr-edge")));
        assert!(!records.iter().any(|r| r.name == "auto1.test.lab"), "old managed record removed");
    }

    #[test]
    fn replace_managed_records_idempotent() {
        let mut records = vec![rec("a.test.lab", "10.0.0.254", Some("hr-edge"))];
        let new_set = vec![
            rec("a.test.lab", "10.0.0.254", None),
            rec("b.test.lab", "10.0.0.254", None),
        ];

        replace_managed_in(&mut records, "hr-edge", new_set.clone());
        let after_first = records.clone();

        replace_managed_in(&mut records, "hr-edge", new_set);

        assert_eq!(after_first.len(), 2);
        assert_eq!(records.len(), 2, "no duplication after second call");
    }

    #[test]
    fn replace_managed_records_stamps_new_records() {
        let mut records = vec![];
        replace_managed_in(
            &mut records,
            "hr-edge",
            vec![rec("a.test.lab", "10.0.0.254", None)],
        );
        assert_eq!(records[0].managed_by.as_deref(), Some("hr-edge"));
    }

    #[test]
    fn replace_managed_records_preserves_other_owners() {
        let mut records = vec![
            rec("custom.test.lab", "10.0.0.10", Some("hr-custom")),
            rec("auto.test.lab", "10.0.0.254", Some("hr-edge")),
        ];

        replace_managed_in(&mut records, "hr-edge", vec![]);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].managed_by.as_deref(), Some("hr-custom"));
    }

    #[test]
    fn static_record_serde_back_compat_without_managed_by() {
        let json = r#"{"name":"a.test.lab","type":"A","value":"10.0.0.1","ttl":60}"#;
        let r: StaticRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.managed_by, None);

        let serialized = serde_json::to_string(&r).unwrap();
        assert!(!serialized.contains("managed_by"), "managed_by skipped when None");
    }

    #[test]
    fn static_record_serde_with_managed_by() {
        let json = r#"{"name":"a.test.lab","type":"A","value":"10.0.0.254","ttl":60,"managed_by":"hr-edge"}"#;
        let r: StaticRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.managed_by.as_deref(), Some("hr-edge"));
    }
}
