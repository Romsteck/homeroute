use leptos::prelude::*;

use crate::types::NetworkData;
#[cfg(feature = "ssr")]
use crate::types::{AddrInfo, NetworkIface, RouteEntry};

#[server]
pub async fn get_network_data() -> Result<NetworkData, ServerFnError> {
    let (interfaces, ipv4_routes, ipv6_routes) =
        tokio::join!(fetch_interfaces(), fetch_routes(false), fetch_routes(true));

    Ok(NetworkData {
        interfaces,
        ipv4_routes,
        ipv6_routes,
    })
}

#[cfg(feature = "ssr")]
async fn fetch_interfaces() -> Vec<NetworkIface> {
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
            let mac = iface
                .get("address")
                .and_then(|a| a.as_str())
                .unwrap_or("")
                .to_string();
            let mtu = iface.get("mtu").and_then(|m| m.as_u64());

            let addresses = iface
                .get("addr_info")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| {
                            Some(AddrInfo {
                                address: a.get("local")?.as_str()?.to_string(),
                                family: a
                                    .get("family")
                                    .and_then(|f| f.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                prefixlen: a.get("prefixlen").and_then(|p| p.as_u64()),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(NetworkIface {
                name: name.to_string(),
                state,
                mac,
                mtu,
                addresses,
            })
        })
        .collect()
}

#[cfg(feature = "ssr")]
async fn fetch_routes(ipv6: bool) -> Vec<RouteEntry> {
    use tokio::process::Command;

    let args: Vec<&str> = if ipv6 {
        vec!["-j", "-6", "route", "show"]
    } else {
        vec!["-j", "route", "show"]
    };

    let output = match Command::new("ip").args(&args).output().await {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let routes: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap_or_default();

    routes
        .into_iter()
        .filter_map(|r| {
            let dst = r.get("dst")?.as_str()?.to_string();
            // Skip link-local and multicast routes for cleaner display
            if dst.starts_with("fe80") || dst.starts_with("ff00") {
                return None;
            }
            let gateway = r.get("gateway").and_then(|g| g.as_str()).map(String::from);
            let device = r.get("dev")?.as_str()?.to_string();
            let metric = r.get("metric").and_then(|m| m.as_u64());
            Some(RouteEntry {
                destination: dst,
                gateway,
                device,
                metric,
            })
        })
        .collect()
}
