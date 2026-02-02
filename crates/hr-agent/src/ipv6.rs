use anyhow::Result;
use tracing::{info, warn};

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
        .args(["-6", "addr", "add", &full_addr, "dev", interface])
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
