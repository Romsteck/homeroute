use anyhow::{Context, Result};
use tokio::process::Command;
use tracing::{info, warn};

const PROFILE_NAME: &str = "homeroute-agent";

/// Ensure the `homeroute-agent` LXD profile exists, creating it if needed.
/// The profile attaches containers to the specified LAN bridge so they get
/// IPv4 via DHCP, ULA via SLAAC, and can receive a GUA from hr-agent.
///
/// Retries up to 5 times to handle the case where LXD snap isn't ready at boot.
pub async fn ensure_profile(lan_bridge: &str, storage_pool: &str) -> Result<()> {
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }

        match try_ensure_profile(lan_bridge, storage_pool).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < 4 {
                    warn!(attempt, "LXD profile setup failed, retrying: {e}");
                } else {
                    return Err(e);
                }
            }
        }
    }

    unreachable!()
}

async fn try_ensure_profile(lan_bridge: &str, storage_pool: &str) -> Result<()> {
    // Check if profile already exists
    let output = Command::new("lxc")
        .args(["profile", "show", PROFILE_NAME])
        .output()
        .await
        .context("failed to run lxc")?;

    if output.status.success() {
        // Check if devices are already configured
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("nictype:") || stdout.contains("parent:") {
            info!(profile = PROFILE_NAME, "LXD profile already configured");
            return Ok(());
        }
        // Profile exists but no devices — reconfigure below
        info!(profile = PROFILE_NAME, "LXD profile exists but needs device configuration");
    } else {
        // Create the profile
        info!(
            profile = PROFILE_NAME,
            bridge = lan_bridge,
            "Creating LXD profile"
        );

        let output = Command::new("lxc")
            .args(["profile", "create", PROFILE_NAME])
            .output()
            .await
            .context("failed to create LXD profile")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lxc profile create failed: {stderr}");
        }
    }

    // Configure devices via `lxc profile device add` (more reliable than piped YAML edit)
    let parent_arg = format!("parent={lan_bridge}");
    let pool_arg = format!("pool={storage_pool}");

    let device_cmds: &[&[&str]] = &[
        &["profile", "device", "add", PROFILE_NAME, "eth0", "nic", "nictype=bridged", &parent_arg],
        &["profile", "device", "add", PROFILE_NAME, "root", "disk", "path=/", &pool_arg],
    ];

    for cmd in device_cmds {
        let output = Command::new("lxc")
            .args(*cmd)
            .output()
            .await
            .context("failed to configure LXD profile device")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "already exists" is fine — device was already added
            if !stderr.contains("already exists") {
                anyhow::bail!("lxc profile device add failed: {stderr}");
            }
        }
    }

    info!(profile = PROFILE_NAME, "LXD profile configured");
    Ok(())
}
