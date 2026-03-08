use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use hr_common::events::UpdateEvent;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use tracing::error;

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

async fn scan_all(State(state): State<ApiState>) -> Json<Value> {
    let scan_id = uuid::Uuid::new_v4().to_string();

    // Emit scan started event
    let _ = state.events.update_scan.send(
        hr_common::events::UpdateScanEvent::ScanStarted { scan_id: scan_id.clone() }
    );

    // 1. Scan main host (reuse existing apt-check logic)
    let scan_id_clone = scan_id.clone();
    let events = state.events.clone();
    let orchestrator = state.orchestrator.clone();
    tokio::spawn(async move {
        let (os_upgradable, os_security, scan_error) = scan_main_host_apt().await;
        let target = hr_common::events::UpdateTarget {
            id: "main".to_string(),
            name: "Hôte principal".to_string(),
            target_type: "main_host".to_string(),
            environment: None,
            online: true,
            os_upgradable,
            os_security,
            agent_version: None,
            agent_version_latest: None,
            claude_cli_installed: None,
            claude_cli_latest: None,
            code_server_installed: None,
            code_server_latest: None,
            claude_ext_installed: None,
            claude_ext_latest: None,
            scan_error,
            scanned_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = events.update_scan.send(
            hr_common::events::UpdateScanEvent::TargetScanned {
                scan_id: scan_id_clone.clone(),
                target,
            }
        );

        // 2. Fan-out to agents and host-agents via IPC
        let _ = orchestrator.request_long(&OrchestratorRequest::ScanUpdates).await;

        // 3. Wait for all targets to report (or timeout after 90s)
        let mut rx = events.update_scan.subscribe();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            let remaining = deadline - tokio::time::Instant::now();
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(hr_common::events::UpdateScanEvent::ScanComplete { .. })) => break,
                Ok(Ok(_)) => {} // ignore other events
                Ok(Err(_)) => break, // channel closed
                Err(_) => break, // timeout
            }
        }
        let _ = events.update_scan.send(
            hr_common::events::UpdateScanEvent::ScanComplete { scan_id: scan_id_clone }
        );
    });

    // Get targets count from orchestrator
    let targets_count = match state.orchestrator.request(&OrchestratorRequest::GetScanResults).await {
        Ok(r) if r.ok => {
            r.data.and_then(|d| d.as_object().map(|o| o.len())).unwrap_or(0) + 1
        }
        _ => 1, // at least main host
    };

    Json(json!({
        "success": true,
        "scan_id": scan_id,
        "targets_count": targets_count
    }))
}

async fn scan_main_host_apt() -> (u32, u32, Option<String>) {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("bash")
            .args(["-c", "/usr/lib/update-notifier/apt-check 2>&1"])
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let text = text.trim();
            if let Some((total, security)) = text.split_once(';') {
                (
                    total.parse().unwrap_or(0),
                    security.parse().unwrap_or(0),
                    None,
                )
            } else {
                (0, 0, Some(format!("apt-check unexpected: {text}")))
            }
        }
        Ok(Err(e)) => (0, 0, Some(format!("apt-check error: {e}"))),
        Err(_) => (0, 0, Some("apt-check timed out".into())),
    }
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

    if req.target_id == "main" && req.category == "apt" {
        // Main host APT upgrade -- reuse existing logic
        let events = state.events.clone();
        let update_log = state.update_log.clone();
        tokio::spawn(async move {
            let result = tokio::process::Command::new("bash")
                .args(["-c", "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get upgrade -y"])
                .output()
                .await;
            let (success, error) = match result {
                Ok(out) if out.status.success() => (true, None),
                Ok(out) => (false, Some(String::from_utf8_lossy(&out.stderr).to_string())),
                Err(e) => (false, Some(e.to_string())),
            };
            let _ = events.update_scan.send(
                hr_common::events::UpdateScanEvent::UpgradeComplete {
                    target_id: "main".to_string(),
                    category: "apt".to_string(),
                    success,
                    error: error.clone(),
                }
            );
            let _ = update_log.update_status("main", "apt", if success { "success" } else { "failed" }, error.as_deref());
        });
    } else {
        // Container agent -- send RunUpgrade via IPC to orchestrator
        let msg = json!({
            "type": "run_upgrade",
            "category": req.category,
        });
        let _ = state.orchestrator.request(&OrchestratorRequest::SendToAgent {
            app_id: req.target_id,
            message: msg,
        }).await;
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
