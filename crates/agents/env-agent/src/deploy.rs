//! Artifact deployment and rollback for apps.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Download an artifact, verify its SHA256, extract it to `app_path`.
///
/// Steps:
/// 1. Download from `artifact_url`
/// 2. Verify SHA256 against `expected_sha256`
/// 3. Backup current app dir to `{app_path}.prev`
/// 4. Extract tar.gz into fresh `app_path`
/// 5. On failure, restore `.prev` backup
pub async fn deploy_artifact(
    artifact_url: &str,
    expected_sha256: &str,
    app_path: &Path,
) -> Result<String> {
    let tmp_path = PathBuf::from(format!("{}.tar.gz.tmp", app_path.display()));
    let prev_path = PathBuf::from(format!("{}.prev", app_path.display()));

    // 1. Download artifact
    info!(url = %artifact_url, "Downloading artifact");
    let response = reqwest::get(artifact_url)
        .await
        .map_err(|e| anyhow!("download failed: {e}"))?;

    if !response.status().is_success() {
        bail!(
            "download failed: HTTP {}",
            response.status()
        );
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| anyhow!("failed to read response body: {e}"))?;

    let size = bytes.len();
    info!(size, "Artifact downloaded");

    // 2. Verify SHA256
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual_sha256 = hex::encode(hasher.finalize());

    if actual_sha256 != expected_sha256 {
        bail!(
            "SHA256 mismatch: expected {}, got {}",
            expected_sha256,
            actual_sha256
        );
    }
    debug!("SHA256 verified");

    // 3. Write to temp file
    fs::write(&tmp_path, &bytes).await.map_err(|e| {
        anyhow!("failed to write temp file {}: {e}", tmp_path.display())
    })?;

    // 4. Backup current app dir → .prev
    if app_path.exists() {
        // Remove old .prev if it exists
        if prev_path.exists() {
            if let Err(e) = fs::remove_dir_all(&prev_path).await {
                warn!(path = %prev_path.display(), "Failed to remove old .prev: {e}");
            }
        }
        fs::rename(app_path, &prev_path).await.map_err(|e| {
            anyhow!(
                "failed to backup {} → {}: {e}",
                app_path.display(),
                prev_path.display()
            )
        })?;
        debug!(prev = %prev_path.display(), "Backed up current app dir");
    }

    // 5. Create new app dir and extract
    fs::create_dir_all(app_path).await.map_err(|e| {
        anyhow!("failed to create {}: {e}", app_path.display())
    })?;

    let tmp_str = tmp_path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF8 temp path"))?;
    let app_str = app_path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF8 app path"))?;

    let output = Command::new("tar")
        .args(["xzf", tmp_str, "-C", app_str])
        .output()
        .await
        .map_err(|e| anyhow!("failed to run tar: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr, "tar extraction failed, restoring backup");

        // Restore .prev backup on extraction failure
        let _ = fs::remove_dir_all(app_path).await;
        if prev_path.exists() {
            if let Err(e) = fs::rename(&prev_path, app_path).await {
                warn!("Failed to restore backup: {e}");
            }
        }

        // Clean up temp file
        let _ = fs::remove_file(&tmp_path).await;
        bail!("tar extraction failed: {}", stderr.trim());
    }

    // 6. Clean up temp file
    let _ = fs::remove_file(&tmp_path).await;

    let msg = format!(
        "Deployed {} bytes to {}",
        size,
        app_path.display()
    );
    info!("{msg}");
    Ok(msg)
}

/// Rollback an app by restoring the `.prev` backup.
///
/// Steps:
/// 1. Check `.prev` exists
/// 2. Rename current → `.failed`
/// 3. Rename `.prev` → current
/// 4. Remove `.failed`
pub async fn rollback_app(app_path: &Path) -> Result<String> {
    let prev_path = PathBuf::from(format!("{}.prev", app_path.display()));
    let failed_path = PathBuf::from(format!("{}.failed", app_path.display()));

    if !prev_path.exists() {
        bail!(
            "No previous version found at {}",
            prev_path.display()
        );
    }

    // Move current → .failed
    if app_path.exists() {
        // Remove stale .failed if present
        if failed_path.exists() {
            let _ = fs::remove_dir_all(&failed_path).await;
        }
        fs::rename(app_path, &failed_path).await.map_err(|e| {
            anyhow!(
                "failed to move current {} → {}: {e}",
                app_path.display(),
                failed_path.display()
            )
        })?;
    }

    // Move .prev → current
    fs::rename(&prev_path, app_path).await.map_err(|e| {
        anyhow!(
            "failed to restore {} → {}: {e}",
            prev_path.display(),
            app_path.display()
        )
    })?;

    // Clean up .failed
    if failed_path.exists() {
        if let Err(e) = fs::remove_dir_all(&failed_path).await {
            warn!(path = %failed_path.display(), "Failed to clean up .failed dir: {e}");
        }
    }

    let msg = format!("Rolled back to previous version at {}", app_path.display());
    info!("{msg}");
    Ok(msg)
}
