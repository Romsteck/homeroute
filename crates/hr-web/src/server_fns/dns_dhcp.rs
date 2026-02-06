use leptos::prelude::*;

use crate::types::{DnsDhcpData, LeaseInfo};
#[cfg(feature = "ssr")]
use crate::types::{DhcpConfigInfo, DnsConfigInfo, DnsRecord, Ipv6ConfigInfo};

#[server]
pub async fn get_dns_dhcp_data() -> Result<DnsDhcpData, ServerFnError> {
    use std::path::PathBuf;

    use hr_dhcp::SharedDhcpState;
    use hr_dns::SharedDnsState;

    let config_path: PathBuf = expect_context();
    let dhcp: SharedDhcpState = expect_context();
    let dns: SharedDnsState = expect_context();

    // Read config file
    let config_str = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Config read error: {e}")))?;
    let config: serde_json::Value =
        serde_json::from_str(&config_str).map_err(|e| ServerFnError::new(format!("{e}")))?;

    // DNS config
    let dns_state = dns.read().await;
    let cache_size = dns_state.dns_cache.len().await;
    let adblock_enabled = dns_state.adblock_enabled;
    drop(dns_state);

    let dns_cfg = config.get("dns");
    let dns_info = DnsConfigInfo {
        upstream_servers: dns_cfg
            .and_then(|d| d.get("upstream"))
            .and_then(|u| u.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        cache_size,
        wildcard_domain: dns_cfg
            .and_then(|d| d.get("wildcard_domain"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        wildcard_ipv4: dns_cfg
            .and_then(|d| d.get("wildcard_ipv4"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        wildcard_ipv6: dns_cfg
            .and_then(|d| d.get("wildcard_ipv6"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        adblock_enabled,
    };

    // Static records
    let static_records = dns_cfg
        .and_then(|d| d.get("static_records"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some(DnsRecord {
                        name: r.get("name")?.as_str()?.to_string(),
                        record_type: r.get("type")?.as_str()?.to_string(),
                        value: r.get("value")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // DHCP config
    let dhcp_cfg = config.get("dhcp");
    let dhcp_info = DhcpConfigInfo {
        enabled: dhcp_cfg
            .and_then(|d| d.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        interface: dhcp_cfg
            .and_then(|d| d.get("interface"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        range_start: dhcp_cfg
            .and_then(|d| d.get("range_start"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        range_end: dhcp_cfg
            .and_then(|d| d.get("range_end"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        lease_time_secs: dhcp_cfg
            .and_then(|d| d.get("lease_time"))
            .and_then(|v| v.as_u64())
            .unwrap_or(86400),
        gateway: dhcp_cfg
            .and_then(|d| d.get("gateway"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        dns_server: dhcp_cfg
            .and_then(|d| d.get("dns_server"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        domain: dhcp_cfg
            .and_then(|d| d.get("domain"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };

    // IPv6 config
    let ipv6_cfg = config.get("ipv6");
    let ipv6_info = Ipv6ConfigInfo {
        ra_enabled: ipv6_cfg
            .and_then(|d| d.get("ra_enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        dhcpv6_range: ipv6_cfg
            .and_then(|d| d.get("dhcpv6_range"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        dns_servers: ipv6_cfg
            .and_then(|d| d.get("dns_servers"))
            .and_then(|u| u.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
    };

    // DHCP leases
    let mut dhcp_state = dhcp.write().await;
    dhcp_state.lease_store.purge_expired();
    let leases: Vec<LeaseInfo> = dhcp_state
        .lease_store
        .all_leases()
        .into_iter()
        .map(|l| LeaseInfo {
            hostname: l.hostname.clone(),
            ip: l.ip.to_string(),
            mac: l.mac.clone(),
            expiry: l.expiry,
        })
        .collect();
    drop(dhcp_state);

    Ok(DnsDhcpData {
        dns: dns_info,
        dhcp: dhcp_info,
        ipv6: ipv6_info,
        leases,
        static_records,
    })
}

#[server]
pub async fn reload_dns_config() -> Result<(), ServerFnError> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use hr_adblock::AdblockEngine;
    use hr_dhcp::SharedDhcpState;
    use hr_dns::SharedDnsState;
    use tokio::sync::RwLock;

    let config_path: PathBuf = expect_context();
    let dhcp: SharedDhcpState = expect_context();
    let dns: SharedDnsState = expect_context();
    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();

    let config_str = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Lecture config: {e}")))?;
    let config: serde_json::Value =
        serde_json::from_str(&config_str).map_err(|e| ServerFnError::new(format!("{e}")))?;

    // Update DNS
    if let Some(dns_cfg) = config.get("dns") {
        if let Ok(dns_config) = serde_json::from_value::<hr_dns::DnsConfig>(dns_cfg.clone()) {
            let mut state = dns.write().await;
            state.config = dns_config;
        }
    }

    // Update DHCP
    if let Some(dhcp_cfg) = config.get("dhcp") {
        if let Ok(dhcp_config) = serde_json::from_value::<hr_dhcp::DhcpConfig>(dhcp_cfg.clone()) {
            let mut state = dhcp.write().await;
            state.config = dhcp_config;
        }
    }

    // Update Adblock whitelist
    if let Some(adblock_cfg) = config.get("adblock") {
        if let Ok(ab_config) = serde_json::from_value::<hr_adblock::config::AdblockConfig>(adblock_cfg.clone()) {
            let mut engine = adblock.write().await;
            engine.set_whitelist(ab_config.whitelist);
        }
    }

    leptos_axum::redirect("/dns?msg=Configuration+recharg%C3%A9e");
    Ok(())
}
