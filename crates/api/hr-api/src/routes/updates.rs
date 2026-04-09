use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use hr_common::events::UpdateEvent;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use hr_ipc::orchestrator::OrchestratorRequest;
use crate::state::ApiState;

static CHECK_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static UPGRADE_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(check_status))
        .route("/last", get(last_check))
        .route("/check", post(run_check))
        .route("/cancel", post(cancel_check))
        .route("/upgrade/status", get(upgrade_status))
        .route("/upgrade/apt", post(upgrade_apt))
        .route("/upgrade/apt-full", post(upgrade_apt_full))
        .route("/upgrade/snap", post(upgrade_snap))
        .route("/upgrade/cancel", post(cancel_upgrade))
        // Unified update scan endpoints
        .route("/scan-all", post(scan_all))
        .route("/scan-all/results", get(scan_all_results))
        .route("/upgrade-target", post(upgrade_target))
        .route("/history", get(update_history))
        .route("/count", get(update_count))
        .route("/upgrade-hosts", post(upgrade_hosts))
        .route("/upgrade-environments", post(upgrade_environments))
}

const LAST_CHECK_PATH: &str = "/var/lib/server-dashboard/last-update-check.json";

async fn check_status() -> Json<Value> {
    let running = CHECK_RUNNING.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({"success": true, "running": running}))
}

async fn last_check() -> Json<Value> {
    match tokio::fs::read_to_string(LAST_CHECK_PATH).await {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(result) => Json(json!({"success": true, "result": result})),
            Err(_) => Json(json!({"success": true, "result": null})),
        },
        Err(_) => Json(json!({"success": true, "result": null})),
    }
}

async fn run_check(State(state): State<ApiState>) -> Json<Value> {
    if CHECK_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        return Json(json!({"success": false, "error": "Verification deja en cours"}));
    }

    CHECK_RUNNING.store(true, std::sync::atomic::Ordering::Relaxed);
    let tx = state.events.updates.clone();

    // Spawn the check in background so it streams events via WebSocket
    tokio::spawn(async move {
        let _ = tx.send(UpdateEvent::Started);
        let start = std::time::Instant::now();

        // Phase 1: apt update
        let _ = tx.send(UpdateEvent::Phase {
            phase: "apt-update".to_string(),
            message: "Mise a jour des listes de paquets...".to_string(),
        });

        stream_command(&tx, "apt", &["update", "-q"]).await;

        // Phase 2: apt list --upgradable
        let _ = tx.send(UpdateEvent::Phase {
            phase: "apt-check".to_string(),
            message: "Verification des paquets APT...".to_string(),
        });

        let apt_output = tokio::process::Command::new("apt")
            .args(["list", "--upgradable"])
            .output()
            .await;

        let apt_packages = match apt_output {
            Ok(o) => parse_apt_list(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => vec![],
        };

        let security_count = apt_packages
            .iter()
            .filter(|p| {
                p.get("isSecurity")
                    .and_then(|s| s.as_bool())
                    .unwrap_or(false)
            })
            .count();

        let _ = tx.send(UpdateEvent::AptComplete {
            packages: apt_packages.clone(),
            security_count,
        });

        // Phase 3: snap refresh --list
        let _ = tx.send(UpdateEvent::Phase {
            phase: "snap-check".to_string(),
            message: "Verification des snaps...".to_string(),
        });

        let snap_output = tokio::process::Command::new("snap")
            .args(["refresh", "--list"])
            .output()
            .await;

        let snap_packages = match snap_output {
            Ok(o) if o.status.success() => {
                parse_snap_list(&String::from_utf8_lossy(&o.stdout))
            }
            _ => vec![],
        };

        let _ = tx.send(UpdateEvent::SnapComplete {
            snaps: snap_packages.clone(),
        });

        // Phase 4: needrestart
        let _ = tx.send(UpdateEvent::Phase {
            phase: "needrestart".to_string(),
            message: "Verification des services...".to_string(),
        });

        let needrestart = tokio::process::Command::new("needrestart")
            .args(["-b"])
            .output()
            .await;

        let needrestart_info = match needrestart {
            Ok(o) => parse_needrestart(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => json!({"kernelRebootNeeded": false, "services": []}),
        };

        let _ = tx.send(UpdateEvent::NeedrestartComplete(needrestart_info.clone()));

        let duration = start.elapsed().as_millis() as u64;

        let summary = json!({
            "totalUpdates": apt_packages.len() + snap_packages.len(),
            "securityUpdates": security_count,
            "servicesNeedingRestart": needrestart_info.get("services").and_then(|s| s.as_array()).map(|a| a.len()).unwrap_or(0)
        });

        let result = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "duration": duration,
            "apt": { "packages": apt_packages, "securityCount": security_count },
            "snap": { "packages": snap_packages },
            "needrestart": needrestart_info,
            "summary": summary
        });

        if let Ok(content) = serde_json::to_string_pretty(&result) {
            let _ = tokio::fs::write(LAST_CHECK_PATH, &content).await;
        }

        let _ = tx.send(UpdateEvent::Complete {
            success: true,
            summary,
            duration,
        });

        CHECK_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    Json(json!({"success": true, "message": "Verification lancee"}))
}

async fn cancel_check(State(state): State<ApiState>) -> Json<Value> {
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "apt update"])
        .output()
        .await;
    CHECK_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = state.events.updates.send(UpdateEvent::Cancelled);
    Json(json!({"success": true}))
}

async fn upgrade_status() -> Json<Value> {
    let running = UPGRADE_RUNNING.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({"success": true, "running": running}))
}

async fn upgrade_apt(State(state): State<ApiState>) -> Json<Value> {
    run_upgrade(state, "apt", &["upgrade", "-y"]).await
}

async fn upgrade_apt_full(State(state): State<ApiState>) -> Json<Value> {
    run_upgrade(state, "apt", &["full-upgrade", "-y"]).await
}

async fn upgrade_snap(State(state): State<ApiState>) -> Json<Value> {
    run_upgrade(state, "snap", &["refresh"]).await
}

async fn run_upgrade(state: ApiState, cmd: &str, args: &[&str]) -> Json<Value> {
    if UPGRADE_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        return Json(json!({"success": false, "error": "Mise a jour deja en cours"}));
    }

    UPGRADE_RUNNING.store(true, std::sync::atomic::Ordering::Relaxed);
    let tx = state.events.updates.clone();
    let cmd = cmd.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    tokio::spawn(async move {
        let upgrade_type = if cmd == "snap" {
            "snap".to_string()
        } else if args.contains(&"full-upgrade".to_string()) {
            "apt-full".to_string()
        } else {
            "apt".to_string()
        };

        let _ = tx.send(UpdateEvent::UpgradeStarted {
            upgrade_type: upgrade_type.clone(),
        });

        let start = std::time::Instant::now();

        let mut child = match tokio::process::Command::new(&cmd)
            .args(&args)
            .env("DEBIAN_FRONTEND", "noninteractive")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(UpdateEvent::UpgradeComplete {
                    upgrade_type,
                    success: false,
                    duration: 0,
                    error: Some(e.to_string()),
                });
                UPGRADE_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        };

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let tx_c = tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_c.send(UpdateEvent::UpgradeOutput { line });
                }
            });
        }

        // Stream stderr
        if let Some(stderr) = child.stderr.take() {
            let tx_c = tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_c.send(UpdateEvent::UpgradeOutput { line });
                }
            });
        }

        let status = child.wait().await;
        let duration = start.elapsed().as_millis() as u64;
        let success = status.map(|s| s.success()).unwrap_or(false);

        let _ = tx.send(UpdateEvent::UpgradeComplete {
            upgrade_type,
            success,
            duration,
            error: if success {
                None
            } else {
                Some("Upgrade failed".to_string())
            },
        });

        UPGRADE_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    Json(json!({"success": true, "message": "Mise a jour lancee"}))
}

async fn cancel_upgrade(State(state): State<ApiState>) -> Json<Value> {
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "apt upgrade|apt full-upgrade|snap refresh"])
        .output()
        .await;
    UPGRADE_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = state.events.updates.send(UpdateEvent::UpgradeCancelled);
    Json(json!({"success": true}))
}

/// Stream command output line by line as UpdateEvent::Output
async fn stream_command(tx: &broadcast::Sender<UpdateEvent>, cmd: &str, args: &[&str]) {
    let mut child = match tokio::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn {}: {}", cmd, e);
            return;
        }
    };

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(UpdateEvent::Output { line });
        }
    }

    let _ = child.wait().await;
}

fn parse_apt_list(output: &str) -> Vec<Value> {
    output
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

            Some(json!({
                "name": name,
                "currentVersion": current_version,
                "newVersion": new_version,
                "isSecurity": is_security
            }))
        })
        .collect()
}

fn parse_snap_list(output: &str) -> Vec<Value> {
    output
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            }
            Some(json!({
                "name": parts[0],
                "newVersion": parts.get(1).unwrap_or(&""),
                "publisher": parts.get(4).unwrap_or(&"")
            }))
        })
        .collect()
}

fn parse_needrestart(output: &str) -> Value {
    let kernel_reboot = output.contains("NEEDRESTART-KSTA: 3");
    let services: Vec<String> = output
        .lines()
        .filter(|l| l.starts_with("NEEDRESTART-SVC:"))
        .filter_map(|l| l.split(':').nth(1).map(|s| s.trim().to_string()))
        .collect();

    json!({
        "kernelRebootNeeded": kernel_reboot,
        "services": services
    })
}

// ── Unified Update Scan ────────────────────────────────────────

/// Run a background scan (used by the hourly scan task and the API endpoint).
/// Returns immediately after spawning the scan task.
pub async fn run_background_scan(
    events: std::sync::Arc<hr_common::events::EventBus>,
    orchestrator: std::sync::Arc<hr_ipc::orchestrator::OrchestratorClient>,
) {
    let scan_id = uuid::Uuid::new_v4().to_string();
    let _ = events.update_scan.send(
        hr_common::events::UpdateScanEvent::ScanStarted { scan_id: scan_id.clone() }
    );

    let scan_id_clone = scan_id.clone();
    tokio::spawn(async move {
        // Fan-out to all agents and host-agents via orchestrator
        let resp = orchestrator.request_long(&OrchestratorRequest::ScanUpdates).await;
        let expected = resp.ok()
            .and_then(|r| r.data)
            .and_then(|d| d.get("agents_scanned").and_then(|v| v.as_u64()))
            .unwrap_or(0) as usize;

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut seen = std::collections::HashSet::new();
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Ok(r) = orchestrator.request(&OrchestratorRequest::GetScanResults).await {
                if let Some(obj) = r.data.as_ref().and_then(|d| d.as_object()) {
                    for (id, val) in obj {
                        if seen.insert(id.clone()) {
                            if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(val.clone()) {
                                let _ = events.update_scan.send(
                                    hr_common::events::UpdateScanEvent::TargetScanned {
                                        scan_id: scan_id_clone.clone(),
                                        target: t,
                                    }
                                );
                            }
                        }
                    }
                    if expected > 0 && seen.len() >= expected {
                        break;
                    }
                }
            }
        }
        let _ = events.update_scan.send(
            hr_common::events::UpdateScanEvent::ScanComplete { scan_id: scan_id_clone }
        );
    });
}

async fn scan_all(State(state): State<ApiState>) -> Json<Value> {
    let scan_id = uuid::Uuid::new_v4().to_string();

    // Emit scan started event
    let _ = state.events.update_scan.send(
        hr_common::events::UpdateScanEvent::ScanStarted { scan_id: scan_id.clone() }
    );

    // Fan-out to all agents and host-agents via orchestrator
    let scan_id_clone = scan_id.clone();
    let events = state.events.clone();
    let orchestrator = state.orchestrator.clone();
    tokio::spawn(async move {
        let resp = orchestrator.request_long(&OrchestratorRequest::ScanUpdates).await;
        let expected = resp.ok()
            .and_then(|r| r.data)
            .and_then(|d| d.get("agents_scanned").and_then(|v| v.as_u64()))
            .unwrap_or(0) as usize;

        // Poll orchestrator for results and relay to frontend via events
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut seen = std::collections::HashSet::new();
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Ok(r) = orchestrator.request(&OrchestratorRequest::GetScanResults).await {
                if let Some(obj) = r.data.as_ref().and_then(|d| d.as_object()) {
                    for (id, val) in obj {
                        if seen.insert(id.clone()) {
                            if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(val.clone()) {
                                let _ = events.update_scan.send(
                                    hr_common::events::UpdateScanEvent::TargetScanned {
                                        scan_id: scan_id_clone.clone(),
                                        target: t,
                                    }
                                );
                            }
                        }
                    }
                    if expected > 0 && seen.len() >= expected {
                        break;
                    }
                }
            }
        }
        let _ = events.update_scan.send(
            hr_common::events::UpdateScanEvent::ScanComplete { scan_id: scan_id_clone }
        );
    });

    // Get targets count from orchestrator
    let targets_count = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => {
            r.data.and_then(|d| d.as_object().map(|o| o.len())).unwrap_or(0)
        }
        _ => 0,
    };

    Json(json!({
        "success": true,
        "scan_id": scan_id,
        "targets_count": targets_count
    }))
}

async fn scan_all_results(State(state): State<ApiState>) -> Json<Value> {
    let targets = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => r.data.unwrap_or(json!({})),
        _ => json!({}),
    };

    Json(json!({
        "success": true,
        "targets": targets
    }))
}

#[derive(serde::Deserialize)]
struct UpgradeTargetRequest {
    target_id: String,
    category: String,
}

async fn upgrade_target(
    State(state): State<ApiState>,
    Json(req): Json<UpgradeTargetRequest>,
) -> Json<Value> {
    // Log the upgrade start
    let _ = state.update_log.insert(
        &req.target_id, &req.target_id, &req.category,
        None, "started", None,
    );

    let _ = state.events.update_scan.send(
        hr_common::events::UpdateScanEvent::UpgradeStarted {
            target_id: req.target_id.clone(),
            category: req.category.clone(),
        }
    );

    if req.category == "hr_agent" {
        // Agent binary update — use TriggerAgentUpdate (pushes binary + restart)
        let target_id = req.target_id.clone();
        let events = state.events.clone();
        let orchestrator = state.orchestrator.clone();
        let update_log = state.update_log.clone();

        tokio::spawn(async move {
            let result = orchestrator.request_long(&OrchestratorRequest::TriggerAgentUpdate {
                agent_ids: Some(vec![target_id.clone()]),
            }).await;
            let (success, error) = match result {
                Ok(r) if r.ok => (true, None),
                Ok(r) => (false, Some(r.error.unwrap_or_else(|| "Unknown error".into()))),
                Err(e) => (false, Some(e.to_string())),
            };

            if success {
                // Wait for agent to reconnect after restart, then trigger re-scan
                // so the scan_results cache gets the new agent version
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = orchestrator.request(&OrchestratorRequest::SendToAgent {
                    app_id: target_id.clone(),
                    message: serde_json::json!({"type": "run_update_scan"}),
                }).await;
                // Poll until scan_results shows the updated agent version
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                let mut updated = false;
                while tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Ok(r) = orchestrator.request(&OrchestratorRequest::GetScanResults).await {
                        if let Some(target) = r.data.as_ref().and_then(|d| d.get(&target_id)) {
                            let av = target.get("agent_version").and_then(|v| v.as_str()).unwrap_or("");
                            let al = target.get("agent_version_latest").and_then(|v| v.as_str()).unwrap_or("");
                            if !av.is_empty() && av == al {
                                updated = true;
                                // Relay updated target to frontend
                                if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(target.clone()) {
                                    let _ = events.update_scan.send(
                                        hr_common::events::UpdateScanEvent::TargetScanned {
                                            scan_id: String::new(),
                                            target: t,
                                        }
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
                if !updated {
                    // Relay whatever we have even if version didn't change
                    if let Ok(r) = orchestrator.request(&OrchestratorRequest::GetScanResults).await {
                        if let Some(target) = r.data.as_ref().and_then(|d| d.get(&target_id)) {
                            if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(target.clone()) {
                                let _ = events.update_scan.send(
                                    hr_common::events::UpdateScanEvent::TargetScanned {
                                        scan_id: String::new(),
                                        target: t,
                                    }
                                );
                            }
                        }
                    }
                }
            }

            let status = if success { "success" } else { "failed" };
            let _ = update_log.update_status(&target_id, "hr_agent", status, error.as_deref());
            let _ = events.update_scan.send(
                hr_common::events::UpdateScanEvent::UpgradeComplete {
                    target_id: target_id.clone(),
                    category: "hr_agent".to_string(),
                    success,
                    error,
                }
            );
        });
    } else {
        // Determine if target is a host-agent or container agent from scan results
        // Determine target type from scan results
        let target_type = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
            Ok(r) => r.data.as_ref()
                .and_then(|d| d.get(&req.target_id))
                .and_then(|t| t.get("target_type").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .unwrap_or_default(),
            _ => String::new(),
        };
        let is_host = target_type == "remote_host" || target_type == "main_host";
        let is_env = target_type == "environment";

        let send_result = if is_host && req.category == "apt" {
            // Host-agent — send RunAptUpgrade via SendHostCommand
            info!(target_id = %req.target_id, category = %req.category, "Sending RunAptUpgrade to host-agent via IPC");
            let cmd = json!({"type": "RunAptUpgrade", "data": {"full_upgrade": false}});
            state.orchestrator.request(&OrchestratorRequest::SendHostCommand {
                host_id: req.target_id.clone(),
                command: cmd,
            }).await
        } else if is_env {
            // Env-agent — send RunUpgrade via SendToEnv
            info!(target_id = %req.target_id, category = %req.category, "Sending RunUpgrade to env-agent via IPC");
            let msg = json!({
                "type": "run_upgrade",
                "category": req.category,
            });
            state.orchestrator.request(&OrchestratorRequest::SendToEnv {
                env_slug: req.target_id.clone(),
                message: msg,
            }).await
        } else {
            // Container agent (legacy) — send RunUpgrade via SendToAgent
            info!(target_id = %req.target_id, category = %req.category, "Sending RunUpgrade to agent via IPC");
            let msg = json!({
                "type": "run_upgrade",
                "category": req.category,
            });
            state.orchestrator.request(&OrchestratorRequest::SendToAgent {
                app_id: req.target_id.clone(),
                message: msg,
            }).await
        };

        // Check for IPC transport error or orchestrator-level error
        let ipc_err = match &send_result {
            Err(e) => Some(format!("IPC error: {e}")),
            Ok(r) if !r.ok => Some(r.error.clone().unwrap_or_else(|| "Agent non connecté".into())),
            _ => None,
        };
        if let Some(err) = ipc_err {
            warn!(target_id = %req.target_id, category = %req.category, error = %err, "RunUpgrade send failed");
            let _ = state.update_log.update_status(&req.target_id, &req.category, "failed", Some(&err));
            let _ = state.events.update_scan.send(
                hr_common::events::UpdateScanEvent::UpgradeComplete {
                    target_id: req.target_id,
                    category: req.category,
                    success: false,
                    error: Some(err.clone()),
                }
            );
            return Json(json!({"success": false, "error": err}));
        }

        info!(target_id = %req.target_id, category = %req.category, "RunUpgrade sent successfully, starting poll");

        // Poll orchestrator scan_results to detect when agent completes upgrade
        let target_id = req.target_id.clone();
        let category = req.category.clone();
        let events = state.events.clone();
        let orchestrator = state.orchestrator.clone();
        let update_log = state.update_log.clone();

        // Capture the baseline value of the field that should change after upgrade.
        // This avoids false positives from concurrent scans changing scanned_at.
        let baseline_value = match orchestrator.request(&OrchestratorRequest::GetScanResults).await {
            Ok(r) => r.data.as_ref()
                .and_then(|d| d.get(&target_id))
                .map(|t| extract_upgrade_field(t, &category)),
            _ => None,
        };
        info!(target_id = %target_id, category = %category, baseline = ?baseline_value, "Captured baseline for polling");

        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);
            let mut success = false;
            while tokio::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if let Ok(r) = orchestrator.request(&OrchestratorRequest::GetScanResults).await {
                    if let Some(target) = r.data.as_ref().and_then(|d| d.get(&target_id)) {
                        let current_value = extract_upgrade_field(target, &category);
                        if let Some(ref baseline) = baseline_value {
                            if current_value != *baseline {
                                info!(
                                    target_id = %target_id, category = %category,
                                    before = %baseline, after = %current_value,
                                    "Upgrade detected via field change"
                                );
                                success = true;
                                // Relay updated target to frontend
                                if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(target.clone()) {
                                    let _ = events.update_scan.send(
                                        hr_common::events::UpdateScanEvent::TargetScanned {
                                            scan_id: String::new(),
                                            target: t,
                                        }
                                    );
                                }
                                break;
                            }
                        } else {
                            // No baseline (first scan) -- fall back to scanned_at change
                            let new_scanned_at = target.get("scanned_at")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if new_scanned_at.is_some() {
                                info!(target_id = %target_id, "No baseline, using first scan result");
                                success = true;
                                if let Ok(t) = serde_json::from_value::<hr_common::events::UpdateTarget>(target.clone()) {
                                    let _ = events.update_scan.send(
                                        hr_common::events::UpdateScanEvent::TargetScanned {
                                            scan_id: String::new(),
                                            target: t,
                                        }
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
            }
            let (status, error) = if success {
                ("success", None)
            } else {
                ("failed", Some("Timeout (600s)"))
            };
            if !success {
                warn!(target_id = %target_id, category = %category, "Upgrade polling timed out (600s)");
            }
            let _ = update_log.update_status(&target_id, &category, status, error);
            let _ = events.update_scan.send(
                hr_common::events::UpdateScanEvent::UpgradeComplete {
                    target_id,
                    category,
                    success,
                    error: error.map(|s| s.to_string()),
                }
            );
        });
    }

    Json(json!({"success": true}))
}

async fn update_history(State(state): State<ApiState>) -> Json<Value> {
    let entries = state.update_log.list(50);
    Json(json!({
        "success": true,
        "entries": entries
    }))
}

// ── Update Audit Log ───────────────────────────────────────────

pub struct UpdateAuditLog {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl UpdateAuditLog {
    pub fn new(data_dir: &std::path::Path) -> anyhow::Result<Self> {
        let db_path = data_dir.join("updates.db");
        let conn = rusqlite::Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS update_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                category TEXT NOT NULL,
                version_before TEXT,
                version_after TEXT,
                status TEXT NOT NULL DEFAULT 'started',
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_update_log_ts ON update_history(timestamp DESC);",
        )?;
        // Mark stale "started" entries as interrupted (e.g. from a previous crash/restart)
        let _ = conn.execute(
            "UPDATE update_history SET status = 'failed', error = 'Interrompu (redémarrage service)' WHERE status = 'started'",
            [],
        );
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    pub fn insert(
        &self,
        target_id: &str,
        target_name: &str,
        category: &str,
        version_before: Option<&str>,
        status: &str,
        error: Option<&str>,
    ) -> Option<i64> {
        let conn = self.conn.lock().ok()?;
        let ts = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO update_history (timestamp, target_id, target_name, category, version_before, status, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![ts, target_id, target_name, category, version_before, status, error],
        )
        .ok()?;
        Some(conn.last_insert_rowid())
    }

    pub fn update_status(&self, target_id: &str, category: &str, status: &str, error: Option<&str>) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "UPDATE update_history SET status = ?1, error = ?2
                 WHERE id = (SELECT id FROM update_history WHERE target_id = ?3 AND category = ?4 ORDER BY timestamp DESC LIMIT 1)",
                rusqlite::params![status, error, target_id, category],
            );
        }
    }

    pub fn list(&self, limit: u32) -> Vec<Value> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT id, timestamp, target_id, target_name, category, version_before, version_after, status, error
             FROM update_history ORDER BY timestamp DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "timestamp": row.get::<_, i64>(1)?,
                    "target_id": row.get::<_, String>(2)?,
                    "target_name": row.get::<_, String>(3)?,
                    "category": row.get::<_, String>(4)?,
                    "version_before": row.get::<_, Option<String>>(5)?,
                    "version_after": row.get::<_, Option<String>>(6)?,
                    "status": row.get::<_, String>(7)?,
                    "error": row.get::<_, Option<String>>(8)?,
                }))
            })
            .ok();

        match rows {
            Some(iter) => iter.filter_map(|r| r.ok()).collect(),
            None => vec![],
        }
    }
}

// ── Update count endpoint ──────────────────────────────────────

async fn update_count(State(state): State<ApiState>) -> Json<Value> {
    let count = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => {
            let targets = r.data.unwrap_or(json!({}));
            if let Some(obj) = targets.as_object() {
                obj.values()
                    .map(|t| t.get("os_upgradable").and_then(|v| v.as_u64()).unwrap_or(0))
                    .sum::<u64>()
            } else {
                0
            }
        }
        _ => 0,
    };
    Json(json!({ "count": count }))
}

// ── Batch upgrade endpoints ────────────────────────────────────

async fn upgrade_hosts(State(state): State<ApiState>) -> Json<Value> {
    let targets = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => r.data.unwrap_or(json!({})),
        Ok(r) => return Json(json!({"success": false, "error": r.error.unwrap_or_else(|| "Orchestrator error".into())})),
        Err(e) => return Json(json!({"success": false, "error": format!("IPC error: {e}")})),
    };

    let mut launched = vec![];
    // All hosts (remote_host and main_host) — upgrade via host-agent
    if let Some(obj) = targets.as_object() {
        for (id, t) in obj {
            let tt = t.get("target_type").and_then(|v| v.as_str()).unwrap_or("");
            let is_host = tt == "remote_host" || tt == "main_host";
            let has_updates = t.get("os_upgradable").and_then(|v| v.as_u64()).unwrap_or(0) > 0;
            if is_host && has_updates {
                let target_name = t.get("name").and_then(|v| v.as_str()).unwrap_or(id).to_string();
                let _ = state.update_log.insert(id, &target_name, "apt", None, "started", None);
                let _ = state.events.update_scan.send(
                    hr_common::events::UpdateScanEvent::UpgradeStarted {
                        target_id: id.clone(),
                        category: "apt".to_string(),
                    }
                );
                let cmd = json!({"type": "RunAptUpgrade", "data": {"full_upgrade": false}});
                let _ = state.orchestrator.request(&OrchestratorRequest::SendHostCommand {
                    host_id: id.clone(),
                    command: cmd,
                }).await;
                launched.push(id.clone());
            }
        }
    }

    Json(json!({"success": true, "launched": launched}))
}

async fn upgrade_environments(State(state): State<ApiState>) -> Json<Value> {
    let targets = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => r.data.unwrap_or(json!({})),
        Ok(r) => return Json(json!({"success": false, "error": r.error.unwrap_or_else(|| "Orchestrator error".into())})),
        Err(e) => return Json(json!({"success": false, "error": format!("IPC error: {e}")})),
    };

    let mut launched = vec![];
    if let Some(obj) = targets.as_object() {
        for (id, t) in obj {
            let is_env = t.get("target_type").and_then(|v| v.as_str()) == Some("environment");
            let has_updates = t.get("os_upgradable").and_then(|v| v.as_u64()).unwrap_or(0) > 0;
            if is_env && has_updates {
                let target_name = t.get("name").and_then(|v| v.as_str()).unwrap_or(id).to_string();
                let _ = state.update_log.insert(id, &target_name, "apt", None, "started", None);
                let _ = state.events.update_scan.send(
                    hr_common::events::UpdateScanEvent::UpgradeStarted {
                        target_id: id.clone(),
                        category: "apt".to_string(),
                    }
                );
                let msg = json!({"type": "run_upgrade", "category": "apt"});
                let _ = state.orchestrator.request(&OrchestratorRequest::SendToEnv {
                    env_slug: id.clone(),
                    message: msg,
                }).await;
                launched.push(id.clone());
            }
        }
    }

    Json(json!({"success": true, "launched": launched}))
}

/// Extract the relevant field value from scan results based on upgrade category.
/// Returns a string representation for comparison (e.g., "4.108.2" for code_server).
/// This avoids false positives from concurrent scans that change scanned_at
/// without actually upgrading anything.
fn extract_upgrade_field(target: &Value, category: &str) -> String {
    let field_name = match category {
        "code_server" => "code_server_installed",
        "claude_cli" => "claude_cli_installed",
        "claude_ext" => "claude_ext_installed",
        "apt" => "os_upgradable",
        _ => "scanned_at", // fallback for unknown categories
    };
    target.get(field_name)
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Null => "null".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "null".to_string())
}
