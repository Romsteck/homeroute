use std::collections::HashMap;

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};

use crate::state::ApiState;

/// Convert a MAC address to EUI-64 identifier (lower 64 bits of IPv6 address)
fn mac_to_eui64(mac: &str) -> Option<u128> {
    let parts: Vec<u8> = mac
        .split(':')
        .filter_map(|s| u8::from_str_radix(s, 16).ok())
        .collect();
    if parts.len() != 6 {
        return None;
    }
    // EUI-64: insert FF:FE in the middle and flip the U/L bit (bit 1 of first byte)
    let eui64: u64 = ((parts[0] ^ 0x02) as u64) << 56
        | (parts[1] as u64) << 48
        | (parts[2] as u64) << 40
        | 0xFF << 32
        | 0xFE << 24
        | (parts[3] as u64) << 16
        | (parts[4] as u64) << 8
        | (parts[5] as u64);
    Some(eui64 as u128)
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/interfaces", get(interfaces))
        .route("/routes", get(ipv4_routes))
        .route("/routes6", get(ipv6_routes))
        .route("/clients", get(lan_clients))
}

async fn interfaces() -> Json<Value> {
    match run_json_command("ip", &["-j", "addr", "show"]).await {
        Ok(raw) => {
            // Filter out veth interfaces and transform to frontend format
            let filtered: Vec<Value> = raw
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter(|iface| {
                    iface
                        .get("ifname")
                        .and_then(|n| n.as_str())
                        .is_some_and(|name| !name.starts_with("veth"))
                })
                .map(|iface| transform_interface(iface))
                .collect();
            Json(json!({"success": true, "interfaces": filtered}))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

/// Transform raw `ip -j addr show` entry to frontend-expected format.
fn transform_interface(raw: &Value) -> Value {
    let flags = raw.get("flags").and_then(|f| f.as_array());
    let state = if flags.is_some_and(|f| f.iter().any(|v| v.as_str() == Some("UP"))) {
        "UP"
    } else {
        "DOWN"
    };

    let addresses: Vec<Value> = raw
        .get("addr_info")
        .and_then(|a| a.as_array())
        .unwrap_or(&vec![])
        .iter()
        .map(|addr| {
            json!({
                "address": addr.get("local").and_then(|v| v.as_str()).unwrap_or(""),
                "family": addr.get("family").and_then(|v| v.as_str()).unwrap_or(""),
                "prefixlen": addr.get("prefixlen"),
                "scope": addr.get("scope").and_then(|v| v.as_str()).unwrap_or("")
            })
        })
        .collect();

    json!({
        "name": raw.get("ifname").and_then(|v| v.as_str()).unwrap_or(""),
        "state": state,
        "mac": raw.get("address").and_then(|v| v.as_str()).unwrap_or(""),
        "mtu": raw.get("mtu"),
        "addresses": addresses
    })
}

async fn ipv4_routes() -> Json<Value> {
    match run_json_command("ip", &["-j", "route", "show"]).await {
        Ok(raw) => {
            let routes: Vec<Value> = raw
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|r| {
                    json!({
                        "destination": r.get("dst").and_then(|v| v.as_str()).unwrap_or(""),
                        "gateway": r.get("gateway").and_then(|v| v.as_str()),
                        "device": r.get("dev").and_then(|v| v.as_str()).unwrap_or(""),
                        "metric": r.get("metric")
                    })
                })
                .collect();
            Json(json!({"success": true, "routes": routes}))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

async fn ipv6_routes() -> Json<Value> {
    match run_json_command("ip", &["-j", "-6", "route", "show"]).await {
        Ok(raw) => {
            let routes: Vec<Value> = raw
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|r| {
                    json!({
                        "destination": r.get("dst").and_then(|v| v.as_str()).unwrap_or(""),
                        "gateway": r.get("gateway").and_then(|v| v.as_str()),
                        "device": r.get("dev").and_then(|v| v.as_str()).unwrap_or(""),
                        "metric": r.get("metric")
                    })
                })
                .collect();
            Json(json!({"success": true, "routes": routes}))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

/// List LAN clients by merging DHCPv4 leases with IPv6 neighbor table.
async fn lan_clients(State(state): State<ApiState>) -> Json<Value> {
    // 1. Get DHCPv4 leases (hostname + MAC + IPv4)
    let dhcp = state.dhcp.read().await;
    let leases = dhcp.lease_store.all_leases();

    // Build map: MAC -> (hostname, ipv4)
    let mut clients: HashMap<String, Value> = HashMap::new();
    for lease in &leases {
        let mac = lease.mac.to_lowercase();
        clients.insert(mac.clone(), json!({
            "mac": mac,
            "hostname": lease.hostname.as_deref().unwrap_or(""),
            "ipv4": lease.ip.to_string(),
            "ipv6_addresses": [],
        }));
    }
    drop(dhcp);

    // 2. Get IPv6 prefixes and ping EUI-64 derived addresses to refresh neighbor table
    let mut prefixes: Vec<(u128, u8)> = Vec::new();
    if let Ok(output) = tokio::process::Command::new("ip")
        .args(["-6", "addr", "show", "dev", "br-lan", "scope", "global"])
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                if line.starts_with("inet6 ") {
                    if let Some(addr_part) = line.strip_prefix("inet6 ") {
                        if let Some((addr_str, rest)) = addr_part.split_once('/') {
                            if let Ok(addr) = addr_str.parse::<std::net::Ipv6Addr>() {
                                let prefix_len: u8 = rest
                                    .split_whitespace()
                                    .next()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(64);
                                let prefix_mask = !0u128 << (128 - prefix_len);
                                let prefix = u128::from(addr) & prefix_mask;
                                prefixes.push((prefix, prefix_len));
                            }
                        }
                    }
                }
            }
        }
    }

    // Spawn background task to ping EUI-64 addresses (fire-and-forget)
    // Refreshes neighbor table for subsequent requests without blocking this one
    let macs: Vec<String> = clients.keys().cloned().collect();
    let prefixes_clone = prefixes.clone();
    tokio::spawn(async move {
        for mac in macs {
            if let Some(eui64) = mac_to_eui64(&mac) {
                for (prefix, _) in &prefixes_clone {
                    let addr = prefix | eui64;
                    let ipv6 = std::net::Ipv6Addr::from(addr);
                    tokio::spawn(async move {
                        let _ = tokio::process::Command::new("ping")
                            .args(["-6", "-c", "1", "-W", "1", &ipv6.to_string()])
                            .output()
                            .await;
                    });
                }
            }
        }
    });

    // 3. Read neighbor table
    if let Ok(output) = tokio::process::Command::new("ip")
        .args(["-6", "neigh", "show", "dev", "br-lan"])
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Format: "<ipv6> lladdr <mac> <state>"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 && parts[1] == "lladdr" {
                    let ipv6 = parts[0];
                    let mac = parts[2].to_lowercase();

                    // Skip link-local addresses only (fe80::)
                    if ipv6.starts_with("fe80:") {
                        continue;
                    }

                    if let Some(client) = clients.get_mut(&mac) {
                        if let Some(arr) = client.get_mut("ipv6_addresses").and_then(|v| v.as_array_mut()) {
                            arr.push(json!(ipv6));
                        }
                    } else {
                        // IPv6 neighbor without DHCPv4 lease
                        clients.insert(mac.clone(), json!({
                            "mac": mac,
                            "hostname": "",
                            "ipv4": "",
                            "ipv6_addresses": [ipv6],
                        }));
                    }
                }
            }
        }
    }

    let mut result: Vec<Value> = clients.into_values().collect();
    // Sort by hostname then by MAC
    result.sort_by(|a, b| {
        let ha = a["hostname"].as_str().unwrap_or("");
        let hb = b["hostname"].as_str().unwrap_or("");
        if ha.is_empty() && !hb.is_empty() {
            std::cmp::Ordering::Greater
        } else if !ha.is_empty() && hb.is_empty() {
            std::cmp::Ordering::Less
        } else {
            ha.to_lowercase().cmp(&hb.to_lowercase())
        }
    });

    Json(json!({ "success": true, "clients": result }))
}

async fn run_json_command(cmd: &str, args: &[&str]) -> Result<Value, String> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to execute {}: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} failed: {}", cmd, stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse output: {}", e))
}
