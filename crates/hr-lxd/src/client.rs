use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;
use tracing::{info, warn};

const PROFILE_NAME: &str = "homeroute-agent";
const IMAGE: &str = "ubuntu:24.04";

/// Information about an LXD container.
#[derive(Debug, Clone, Deserialize)]
pub struct ContainerInfo {
    pub name: String,
    pub status: ContainerState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum ContainerState {
    Running,
    Stopped,
    Unknown,
}

/// Client for managing LXD containers via the `lxc` CLI.
pub struct LxdClient;

impl LxdClient {
    /// Create and start a container with the `homeroute-agent` profile.
    pub async fn create_container(name: &str) -> Result<()> {
        info!(container = name, "Creating LXC container");

        let output = Command::new("lxc")
            .args(["launch", IMAGE, name, "--profile", PROFILE_NAME])
            .output()
            .await
            .context("failed to run lxc launch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lxc launch failed: {stderr}");
        }

        // Wait for the container to get network connectivity
        Self::wait_ready(name).await?;

        info!(container = name, "Container created and running");
        Ok(())
    }

    /// Wait for a container to be running and have a network interface up.
    async fn wait_ready(name: &str) -> Result<()> {
        for i in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let output = Command::new("lxc")
                .args(["exec", name, "--", "ip", "link", "show", "eth0"])
                .output()
                .await?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("state UP") || stdout.contains("UP") {
                    return Ok(());
                }
            }

            if i == 29 {
                warn!(container = name, "Container network not ready after 30s, proceeding anyway");
            }
        }
        Ok(())
    }

    /// Push a local file into a container.
    pub async fn push_file(container: &str, src: &Path, dest: &str) -> Result<()> {
        let target = format!("{container}/{dest}");
        let output = Command::new("lxc")
            .args(["file", "push", src.to_str().unwrap_or(""), &target])
            .output()
            .await
            .context("failed to run lxc file push")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lxc file push to {target} failed: {stderr}");
        }
        Ok(())
    }

    /// Execute a command inside a container and return stdout.
    pub async fn exec(container: &str, cmd: &[&str]) -> Result<String> {
        let mut args = vec!["exec", container, "--"];
        args.extend_from_slice(cmd);

        let output = Command::new("lxc")
            .args(&args)
            .output()
            .await
            .context("failed to run lxc exec")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lxc exec {cmd:?} failed: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Execute a command inside a container with retries (useful for network-dependent commands).
    pub async fn exec_with_retry(container: &str, cmd: &[&str], max_retries: u32) -> Result<String> {
        let mut last_error = None;
        for attempt in 0..max_retries {
            match Self::exec(container, cmd).await {
                Ok(output) => return Ok(output),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < max_retries {
                        warn!(
                            container,
                            attempt = attempt + 1,
                            max_retries,
                            "Command failed, retrying in 3s..."
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    /// Wait for network connectivity inside a container (DNS resolution working).
    pub async fn wait_for_network(container: &str, timeout_secs: u32) -> Result<()> {
        for i in 0..timeout_secs {
            // Try to resolve a known domain to verify DNS is working
            let result = Self::exec(container, &["bash", "-c", "getent hosts archive.ubuntu.com > /dev/null 2>&1"]).await;
            if result.is_ok() {
                info!(container, elapsed_secs = i + 1, "Network connectivity confirmed");
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        warn!(container, timeout_secs, "Network connectivity not confirmed after timeout, proceeding anyway");
        Ok(())
    }

    /// Create a storage volume and attach it to a container at the given path.
    pub async fn attach_storage_volume(
        container: &str,
        volume_name: &str,
        mount_path: &str,
    ) -> Result<()> {
        // Create the storage volume (on the default pool)
        let output = Command::new("lxc")
            .args(["storage", "volume", "create", "default", volume_name])
            .output()
            .await
            .context("failed to create storage volume")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "already exists" errors
            if !stderr.contains("already exists") {
                anyhow::bail!("lxc storage volume create failed: {stderr}");
            }
        }

        // Attach the volume as a disk device
        let device_name = format!("{volume_name}-disk");
        let output = Command::new("lxc")
            .args([
                "config", "device", "add", container, &device_name, "disk",
                &format!("pool=default"),
                &format!("source={volume_name}"),
                &format!("path={mount_path}"),
            ])
            .output()
            .await
            .context("failed to attach storage volume")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("already exists") {
                anyhow::bail!("lxc config device add failed: {stderr}");
            }
        }

        info!(container, volume = volume_name, path = mount_path, "Storage volume attached");
        Ok(())
    }

    /// Stop and delete a container (and its workspace volume).
    pub async fn delete_container(name: &str) -> Result<()> {
        info!(container = name, "Deleting LXC container");

        // Force stop (ignore error if already stopped)
        let _ = Command::new("lxc")
            .args(["stop", name, "--force"])
            .output()
            .await;

        let output = Command::new("lxc")
            .args(["delete", name])
            .output()
            .await
            .context("failed to run lxc delete")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("lxc delete {name} failed: {stderr}");
        }

        // Delete the workspace storage volume (ignore errors if it doesn't exist)
        let vol_name = format!("{name}-workspace");
        let _ = Command::new("lxc")
            .args(["storage", "volume", "delete", "default", &vol_name])
            .output()
            .await;

        info!(container = name, "Container deleted");
        Ok(())
    }

    /// List containers that use the `homeroute-agent` profile.
    pub async fn list_containers() -> Result<Vec<ContainerInfo>> {
        let output = Command::new("lxc")
            .args(["list", "--format", "json"])
            .output()
            .await
            .context("failed to run lxc list")?;

        if !output.status.success() {
            anyhow::bail!("lxc list failed");
        }

        let all: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).context("failed to parse lxc list JSON")?;

        let mut containers = Vec::new();
        for entry in all {
            // Check if this container uses the homeroute-agent profile
            let profiles = entry
                .get("profiles")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if !profiles.contains(&PROFILE_NAME) {
                continue;
            }

            let name = entry
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();

            let status = match entry.get("status").and_then(|s| s.as_str()) {
                Some("Running") => ContainerState::Running,
                Some("Stopped") => ContainerState::Stopped,
                _ => ContainerState::Unknown,
            };

            containers.push(ContainerInfo { name, status });
        }

        Ok(containers)
    }
}
