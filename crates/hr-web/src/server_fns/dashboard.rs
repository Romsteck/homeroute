use leptos::prelude::*;

use crate::types::DashboardData;
#[cfg(feature = "ssr")]
use crate::types::{AdblockInfo, DdnsInfo, InterfaceInfo, LeaseInfo, ServiceInfo};

/// Fetch all dashboard data in a single server function call.
#[server]
pub async fn get_dashboard_data() -> Result<DashboardData, ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;
    use hr_common::config::EnvConfig;
    use hr_common::service_registry::SharedServiceRegistry;
    use hr_dhcp::SharedDhcpState;

    // ── Services ─────────────────────────────────────────────────────
    let registry: SharedServiceRegistry = expect_context();
    let services_map = registry.read().await;
    let mut services: Vec<ServiceInfo> = services_map
        .values()
        .map(|s| {
            let state = serde_json::to_value(&s.state)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "unknown".into());
            let priority = serde_json::to_value(&s.priority)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "background".into());
            ServiceInfo {
                name: s.name.clone(),
                state,
                priority,
                restart_count: s.restart_count,
                error: s.error.clone(),
            }
        })
        .collect();
    drop(services_map);

    // Sort: critical first, then important, then background; within group by name
    services.sort_by(|a, b| {
        fn prio_ord(p: &str) -> u8 {
            match p {
                "critical" => 0,
                "important" => 1,
                _ => 2,
            }
        }
        prio_ord(&a.priority)
            .cmp(&prio_ord(&b.priority))
            .then(a.name.cmp(&b.name))
    });

    // ── DHCP leases ──────────────────────────────────────────────────
    let dhcp: SharedDhcpState = expect_context();
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

    // ── Adblock stats ────────────────────────────────────────────────
    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let engine = adblock.read().await;
    let domain_count = engine.domain_count();
    drop(engine);

    let env: Arc<EnvConfig> = expect_context();
    let (source_count, enabled) = tokio::fs::read_to_string(&env.dns_dhcp_config_path)
        .await
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            let sc = v
                .get("adblock")
                .and_then(|a| a.get("sources"))
                .and_then(|s| s.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let en = v
                .get("adblock")
                .and_then(|a| a.get("enabled"))
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            (sc, en)
        })
        .unwrap_or((0, false));

    // ── Network interfaces ───────────────────────────────────────────
    let interfaces = fetch_interfaces().await;

    // ── DDNS info ────────────────────────────────────────────────────
    let ddns = fetch_ddns_info(&env).await;

    Ok(DashboardData {
        interfaces,
        leases,
        adblock: AdblockInfo {
            domain_count,
            source_count,
            enabled,
        },
        ddns,
        services,
    })
}

// ── SSR-only helpers ─────────────────────────────────────────────────

#[cfg(feature = "ssr")]
async fn fetch_interfaces() -> Vec<InterfaceInfo> {
    use tokio::process::Command;

    let output = match Command::new("ip").args(["-j", "addr", "show"]).output().await {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let ifaces: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap_or_default();

    ifaces
        .into_iter()
        .filter_map(|iface| {
            let name = iface.get("ifname")?.as_str()?;
            // Skip virtual / loopback interfaces
            if name.starts_with("veth")
                || name.starts_with("lo")
                || name.starts_with("br-")
                || name.starts_with("docker")
                || name.starts_with("lxd")
            {
                return None;
            }
            let state = iface
                .get("operstate")
                .and_then(|s| s.as_str())
                .unwrap_or("DOWN")
                .to_string();
            Some(InterfaceInfo {
                name: name.to_string(),
                state,
            })
        })
        .collect()
}

#[cfg(feature = "ssr")]
async fn fetch_ddns_info(env: &hr_common::config::EnvConfig) -> DdnsInfo {
    use tokio::process::Command;

    let current_ipv6 = Command::new("ip")
        .args(["-6", "addr", "show", &env.cf_interface, "scope", "global"])
        .output()
        .await
        .ok()
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.lines().find_map(|line| {
                let line = line.trim();
                if line.starts_with("inet6") && !line.contains("deprecated") {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|addr| addr.split('/').next())
                        .map(String::from)
                } else {
                    None
                }
            })
        });

    DdnsInfo {
        record_name: env.cf_record_name.clone(),
        current_ipv6,
    }
}
