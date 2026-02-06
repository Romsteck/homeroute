use leptos::prelude::*;

use crate::types::UpdatesPageData;
#[cfg(feature = "ssr")]
use crate::types::{AptPackage, SnapPackage};

#[cfg(feature = "ssr")]
const LAST_CHECK_PATH: &str = "/var/lib/server-dashboard/last-update-check.json";

#[server]
pub async fn get_updates_data() -> Result<UpdatesPageData, ServerFnError> {
    let content = tokio::fs::read_to_string(LAST_CHECK_PATH)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let data: serde_json::Value =
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}));

    let last_check = data.get("timestamp").and_then(|v| v.as_str()).map(String::from);

    // APT packages
    let apt_packages: Vec<AptPackage> = data
        .get("apt")
        .and_then(|a| a.get("packages"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(AptPackage {
                        name: p.get("name")?.as_str()?.to_string(),
                        current_version: p
                            .get("currentVersion")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        new_version: p
                            .get("newVersion")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        is_security: p
                            .get("isSecurity")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Snap packages
    let snap_packages: Vec<SnapPackage> = data
        .get("snap")
        .and_then(|s| s.get("packages"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(SnapPackage {
                        name: p.get("name")?.as_str()?.to_string(),
                        new_version: p
                            .get("newVersion")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        revision: p
                            .get("revision")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        publisher: p
                            .get("publisher")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let security_count = apt_packages.iter().filter(|p| p.is_security).count();

    // Needrestart info
    let needrestart = data.get("needrestart");
    let kernel_reboot_needed = needrestart
        .and_then(|n| n.get("kernelRebootNeeded"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let services_to_restart: Vec<String> = needrestart
        .and_then(|n| n.get("services"))
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(UpdatesPageData {
        last_check,
        apt_packages,
        snap_packages,
        security_count,
        kernel_reboot_needed,
        services_to_restart,
    })
}

#[server]
pub async fn check_for_updates() -> Result<(), ServerFnError> {
    // Run apt update
    let apt_update = tokio::process::Command::new("apt")
        .args(["update", "-q"])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    if !apt_update.status.success() {
        return Err(ServerFnError::new("apt update a échoué"));
    }

    // Get upgradable packages
    let apt_list = tokio::process::Command::new("apt")
        .args(["list", "--upgradable"])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    let apt_stdout = String::from_utf8_lossy(&apt_list.stdout);
    let apt_packages: Vec<serde_json::Value> = apt_stdout
        .lines()
        .filter(|line| line.contains('/'))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                return None;
            }
            let name_source: Vec<&str> = parts[0].split('/').collect();
            let name = name_source.first()?.to_string();
            let source = name_source.get(1).unwrap_or(&"").to_string();
            let new_version = parts.get(1).unwrap_or(&"").to_string();
            let current_version = if line.contains("upgradable from:") {
                line.split("upgradable from: ")
                    .nth(1)
                    .map(|v| v.trim_end_matches(']').to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let is_security = source.contains("security");
            Some(serde_json::json!({
                "name": name,
                "currentVersion": current_version,
                "newVersion": new_version,
                "isSecurity": is_security
            }))
        })
        .collect();

    let security_count = apt_packages
        .iter()
        .filter(|p| p.get("isSecurity").and_then(|s| s.as_bool()).unwrap_or(false))
        .count();

    // Snap check
    let snap_packages: Vec<serde_json::Value> = match tokio::process::Command::new("snap")
        .args(["refresh", "--list"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .skip(1)
                .filter(|line| !line.is_empty())
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return None;
                    }
                    Some(serde_json::json!({
                        "name": parts[0],
                        "newVersion": parts.get(1).unwrap_or(&""),
                        "publisher": parts.get(4).unwrap_or(&"")
                    }))
                })
                .collect()
        }
        _ => vec![],
    };

    // Save result
    let timestamp = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let total_updates = apt_packages.len() + snap_packages.len();
    let result = serde_json::json!({
        "timestamp": timestamp,
        "apt": { "packages": apt_packages, "securityCount": security_count },
        "snap": { "packages": snap_packages },
        "summary": {
            "totalUpdates": total_updates,
            "securityUpdates": security_count,
        }
    });

    if let Ok(content) = serde_json::to_string_pretty(&result) {
        let _ = tokio::fs::write(LAST_CHECK_PATH, &content).await;
    }

    leptos_axum::redirect("/updates?msg=V%C3%A9rification+termin%C3%A9e");
    Ok(())
}

#[server]
pub async fn run_apt_upgrade() -> Result<(), ServerFnError> {
    let output = tokio::process::Command::new("apt")
        .args(["upgrade", "-y"])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        leptos_axum::redirect(&format!(
            "/updates?msg=error&detail={}",
            stderr.chars().take(100).collect::<String>().replace(' ', "+")
        ));
        return Ok(());
    }

    leptos_axum::redirect("/updates?msg=Mise+%C3%A0+jour+termin%C3%A9e");
    Ok(())
}
