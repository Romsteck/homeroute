//! Auto-update module for hr-agent.
//! Downloads a new binary, verifies SHA256, replaces itself, and restarts.

use anyhow::{Context, Result};
use tracing::{error, info};

const SELF_PATH: &str = "/usr/local/bin/hr-agent";

/// Download, verify and replace the current binary, then restart.
pub async fn apply_update(download_url: &str, expected_sha256: &str, version: &str) -> Result<()> {
    info!(version, download_url, "Starting auto-update");

    // Download to a temporary file
    let tmp_path = format!("{}.update", SELF_PATH);
    let response = reqwest::get(download_url)
        .await
        .context("Failed to download update")?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed with status {}", response.status());
    }

    let bytes = response
        .bytes()
        .await
        .context("Failed to read update body")?;

    // Verify SHA256
    use std::io::Write;
    let digest = sha256_hex(&bytes);
    if digest != expected_sha256 {
        anyhow::bail!(
            "SHA256 mismatch: expected {expected_sha256}, got {digest}"
        );
    }

    info!(sha256 = digest, bytes = bytes.len(), "Download verified");

    // Write to tmp
    let mut file = std::fs::File::create(&tmp_path)
        .context("Failed to create temp file")?;
    file.write_all(&bytes)
        .context("Failed to write temp file")?;
    drop(file);

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .context("Failed to set permissions")?;
    }

    // Atomic replace
    std::fs::rename(&tmp_path, SELF_PATH)
        .context("Failed to replace binary")?;

    info!(version, "Binary replaced, restarting via systemd");

    // Restart via systemd (the agent runs as a systemd service inside the LXC)
    let status = tokio::process::Command::new("systemctl")
        .args(["restart", "hr-agent"])
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            info!("Restart command sent successfully");
        }
        Ok(s) => {
            error!("Restart command exited with: {s}");
        }
        Err(e) => {
            error!("Failed to send restart command: {e}");
        }
    }

    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write;
    // Use ring for SHA256 since it's already in the dependency tree via rustls
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        write!(hex, "{:02x}", byte).unwrap();
    }
    hex
}
