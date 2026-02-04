use anyhow::Result;
use tracing::{info, warn};

/// Get the current GUA (Global Unicast Address) from an interface.
/// Prefers DHCPv6-assigned addresses (marked 'dynamic') over manually-added ones.
pub async fn get_gua_address(interface: &str, _prefix_hint: Option<&str>) -> Option<String> {
    let output = tokio::process::Command::new("ip")
        .args(["-6", "-o", "addr", "show", "dev", interface, "scope", "global"])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut dhcpv6_addr: Option<String> = None;
    let mut fallback_addr: Option<String> = None;

    for line in stdout.lines() {
        // Format: "2: eth0 inet6 2a0d:3341:b5b1:7500::18/128 scope global dynamic ..."
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(addr_idx) = parts.iter().position(|&p| p == "inet6") {
            if let Some(addr_cidr) = parts.get(addr_idx + 1) {
                let addr = addr_cidr.split('/').next().unwrap_or(addr_cidr);

                // Skip link-local (fe80::), ULA (fd00::), and SLAAC EUI-64 (long addresses)
                if addr.starts_with("fe80:") || addr.starts_with("fd") {
                    continue;
                }

                // Skip SLAAC addresses (typically /64 with EUI-64 suffix)
                // DHCPv6 stateful addresses are usually /128
                if addr_cidr.ends_with("/64") {
                    continue;
                }

                // Check if this is a DHCPv6-assigned address (has 'dynamic' flag)
                let is_dynamic = line.contains("dynamic");

                if is_dynamic && dhcpv6_addr.is_none() {
                    dhcpv6_addr = Some(addr.to_string());
                } else if fallback_addr.is_none() {
                    fallback_addr = Some(addr.to_string());
                }
            }
        }
    }

    // Prefer DHCPv6-assigned address over manually-added ones
    dhcpv6_addr.or(fallback_addr)
}

/// Get the IPv4 address from an interface (for local DNS A records).
pub async fn get_ipv4_address(interface: &str) -> Option<String> {
    let output = tokio::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", "dev", interface, "scope", "global"])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        // Format: "2: eth0 inet 10.0.0.100/24 brd 10.0.0.255 scope global dynamic eth0"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(addr_idx) = parts.iter().position(|&p| p == "inet") {
            if let Some(addr_cidr) = parts.get(addr_idx + 1) {
                let addr = addr_cidr.split('/').next().unwrap_or(addr_cidr);
                // Skip loopback and link-local
                if addr.starts_with("127.") || addr.starts_with("169.254.") {
                    continue;
                }
                return Some(addr.to_string());
            }
        }
    }
    None
}

/// Add an IPv6 address to an interface.
pub async fn add_address(interface: &str, addr: &str) -> Result<()> {
    // Check if already assigned
    let output = tokio::process::Command::new("ip")
        .args(["-6", "addr", "show", "dev", interface])
        .output()
        .await?;

    let current = String::from_utf8_lossy(&output.stdout);
    let addr_no_prefix = addr.split('/').next().unwrap_or(addr);

    if current.contains(addr_no_prefix) {
        info!(addr, interface, "IPv6 address already assigned");
        return Ok(());
    }

    let full_addr = if addr.contains('/') {
        addr.to_string()
    } else {
        format!("{addr}/128")
    };

    let status = tokio::process::Command::new("ip")
        .args(["-6", "addr", "add", &full_addr, "dev", interface, "nodad"])
        .status()
        .await?;

    if !status.success() {
        anyhow::bail!("ip addr add {full_addr} dev {interface} failed with {status}");
    }

    info!(addr = full_addr, interface, "IPv6 address added");
    Ok(())
}

/// Remove an IPv6 address from an interface.
pub async fn remove_address(interface: &str, addr: &str) -> Result<()> {
    let full_addr = if addr.contains('/') {
        addr.to_string()
    } else {
        format!("{addr}/128")
    };

    let status = tokio::process::Command::new("ip")
        .args(["-6", "addr", "del", &full_addr, "dev", interface])
        .status()
        .await?;

    if !status.success() {
        warn!(addr = full_addr, interface, "ip addr del failed (may not exist)");
    } else {
        info!(addr = full_addr, interface, "IPv6 address removed");
    }

    Ok(())
}
