// backup_pipeline.rs — Automated Borg backup pipeline
//
// Pipeline: WOL → wait-for-up → SSH backup scripts → verify → sleep → log
//
// Triggered manually via IPC (TriggerBackup) or automatically on a schedule.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ── Constants ─────────────────────────────────────────────────────────────────

const BACKUP_SERVER_HOST_ID: &str = "877bcb76-4fb8-4164-940c-707201adf9bc";
const BACKUP_SERVER_IP: &str = "10.0.0.20";
const BACKUP_SERVER_USER: &str = "romain";
const BACKUP_SCRIPT: &str = "/opt/backup-scripts/backup-all.sh";
const HOMEROUTE_API_BASE: &str = "http://10.0.0.254:4000";

/// Maximum time to wait for the backup server to come online after WOL (3 min).
const WAKE_TIMEOUT_SECS: u64 = 180;
/// Interval between ping checks while waiting for server.
const WAKE_POLL_INTERVAL_SECS: u64 = 10;
/// Maximum time to let the backup script run (2 hours for large backups).
const BACKUP_TIMEOUT_SECS: u64 = 7200;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    Idle,
    WakingServer,
    WaitingForServer,
    RunningBackup,
    Verifying,
    PuttingToSleep,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatus {
    pub stage: PipelineStage,
    pub running: bool,
    pub last_run_at: Option<String>,
    pub last_run_success: Option<bool>,
    pub last_run_duration_secs: Option<u64>,
    pub last_run_message: Option<String>,
    pub current_message: Option<String>,
}

impl Default for BackupStatus {
    fn default() -> Self {
        Self {
            stage: PipelineStage::Idle,
            running: false,
            last_run_at: None,
            last_run_success: None,
            last_run_duration_secs: None,
            last_run_message: None,
            current_message: None,
        }
    }
}

// ── BackupPipeline ────────────────────────────────────────────────────────────

pub struct BackupPipeline {
    pub status: Arc<RwLock<BackupStatus>>,
    http: reqwest::Client,
}

impl BackupPipeline {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            status: Arc::new(RwLock::new(BackupStatus::default())),
            http,
        }
    }

    /// Trigger the backup pipeline. Returns immediately — runs in background.
    /// Returns Err if a pipeline is already running.
    pub async fn trigger(&self) -> Result<(), String> {
        {
            let status = self.status.read().await;
            if status.running {
                return Err(format!(
                    "Backup pipeline already running (stage: {:?})",
                    status.stage
                ));
            }
        }
        // Mark as running
        {
            let mut status = self.status.write().await;
            status.running = true;
            status.stage = PipelineStage::WakingServer;
            status.current_message = Some("Waking backup server...".to_string());
        }

        let status = self.status.clone();
        let http = self.http.clone();

        tokio::spawn(async move {
            let started = Instant::now();
            let result = run_pipeline(status.clone(), http).await;
            let elapsed = started.elapsed().as_secs();

            let mut s = status.write().await;
            s.running = false;
            s.last_run_at = Some(Utc::now().to_rfc3339());
            s.last_run_duration_secs = Some(elapsed);

            match result {
                Ok(msg) => {
                    info!("Backup pipeline completed successfully in {elapsed}s: {msg}");
                    s.stage = PipelineStage::Done;
                    s.last_run_success = Some(true);
                    s.last_run_message = Some(msg);
                    s.current_message = None;
                }
                Err(e) => {
                    error!("Backup pipeline failed after {elapsed}s: {e}");
                    s.stage = PipelineStage::Failed;
                    s.last_run_success = Some(false);
                    s.last_run_message = Some(e);
                    s.current_message = None;
                }
            }
        });

        Ok(())
    }

    pub async fn get_status(&self) -> BackupStatus {
        self.status.read().await.clone()
    }
}

// ── Pipeline implementation ───────────────────────────────────────────────────

async fn run_pipeline(
    status: Arc<RwLock<BackupStatus>>,
    http: reqwest::Client,
) -> Result<String, String> {
    // ── Step 1: Wake the backup server ──────────────────────────────────────
    set_stage(
        &status,
        PipelineStage::WakingServer,
        "Sending WOL magic packet via HomeRoute API...",
    )
    .await;

    info!("Backup pipeline: waking server {BACKUP_SERVER_HOST_ID}");
    let wake_url = format!("{HOMEROUTE_API_BASE}/api/hosts/{BACKUP_SERVER_HOST_ID}/wake");
    match http.post(&wake_url).send().await {
        Ok(resp) => {
            info!("WOL response: {}", resp.status());
        }
        Err(e) => {
            warn!("WOL request failed (may already be online): {e}");
        }
    }

    // ── Step 2: Wait for the backup server to come online ───────────────────
    set_stage(
        &status,
        PipelineStage::WaitingForServer,
        "Waiting for backup server to come online...",
    )
    .await;

    wait_for_server(status.clone()).await?;

    // ── Step 3: Run backup scripts via SSH ──────────────────────────────────
    set_stage(
        &status,
        PipelineStage::RunningBackup,
        "Running Borg backup scripts on backup server...",
    )
    .await;

    info!("Backup pipeline: executing {BACKUP_SCRIPT} on {BACKUP_SERVER_IP}");
    let backup_result = run_ssh_backup().await;

    // ── Step 4: Verify ──────────────────────────────────────────────────────
    set_stage(&status, PipelineStage::Verifying, "Verifying backup...").await;

    let backup_ok = match &backup_result {
        Ok(output) => {
            info!("Backup script succeeded:\n{output}");
            true
        }
        Err(e) => {
            error!("Backup script failed: {e}");
            false
        }
    };

    // ── Step 5: Put server to sleep (even if backup failed) ─────────────────
    set_stage(
        &status,
        PipelineStage::PuttingToSleep,
        "Putting backup server to sleep...",
    )
    .await;

    info!("Backup pipeline: putting server to sleep");
    let sleep_url = format!("{HOMEROUTE_API_BASE}/api/hosts/{BACKUP_SERVER_HOST_ID}/sleep");
    match http.post(&sleep_url).send().await {
        Ok(resp) => {
            info!("Sleep response: {}", resp.status());
        }
        Err(e) => {
            warn!("Sleep request failed: {e}");
        }
    }

    // ── Step 6: Return result ────────────────────────────────────────────────
    if backup_ok {
        Ok(format!(
            "Backup completed successfully. Output: {}",
            backup_result.unwrap_or_default()
        ))
    } else {
        Err(format!(
            "Backup script failed: {}",
            backup_result.unwrap_err()
        ))
    }
}

async fn wait_for_server(status: Arc<RwLock<BackupStatus>>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(WAKE_TIMEOUT_SECS);
    let mut attempt = 0u32;

    while Instant::now() < deadline {
        attempt += 1;
        let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
        {
            let mut s = status.write().await;
            s.current_message = Some(format!(
                "Waiting for backup server... attempt {attempt} ({remaining}s remaining)"
            ));
        }

        // Try a TCP connection to SSH port (22) as health check
        match tokio::net::TcpStream::connect((BACKUP_SERVER_IP, 22)).await {
            Ok(_) => {
                info!("Backup server is online (attempt {attempt})");
                // Extra grace period for SSH daemon to be fully ready
                tokio::time::sleep(Duration::from_secs(3)).await;
                return Ok(());
            }
            Err(_) => {
                info!("Backup server not yet reachable (attempt {attempt}), retrying in {WAKE_POLL_INTERVAL_SECS}s...");
            }
        }

        tokio::time::sleep(Duration::from_secs(WAKE_POLL_INTERVAL_SECS)).await;
    }

    Err(format!(
        "Backup server did not come online within {WAKE_TIMEOUT_SECS}s"
    ))
}

async fn run_ssh_backup() -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(BACKUP_TIMEOUT_SECS),
        tokio::process::Command::new("ssh")
            .args([
                "-o", "BatchMode=yes",
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=15",
                &format!("{BACKUP_SERVER_USER}@{BACKUP_SERVER_IP}"),
                &format!("sudo {BACKUP_SCRIPT}"),
            ])
            .output(),
    )
    .await
    .map_err(|_| format!("Backup script timed out after {BACKUP_TIMEOUT_SECS}s"))?
    .map_err(|e| format!("Failed to spawn SSH process: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("{stdout}\n{stderr}").trim().to_string())
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(format!(
            "SSH backup script exited with code {code}.\nSTDOUT: {stdout}\nSTDERR: {stderr}"
        ))
    }
}

async fn set_stage(
    status: &Arc<RwLock<BackupStatus>>,
    stage: PipelineStage,
    message: &str,
) {
    let mut s = status.write().await;
    s.stage = stage;
    s.current_message = Some(message.to_string());
    info!("Backup pipeline: {message}");
}

// ── Scheduled runner ──────────────────────────────────────────────────────────

/// Spawn a background task that runs the backup pipeline daily at 03:00 UTC.
pub fn spawn_daily_scheduler(pipeline: Arc<BackupPipeline>) {
    tokio::spawn(async move {
        loop {
            let next = next_scheduled_run_secs();
            info!("Backup scheduler: next run in {next}s ({:.1}h)", next as f64 / 3600.0);
            tokio::time::sleep(Duration::from_secs(next)).await;

            info!("Backup scheduler: triggering daily backup pipeline");
            if let Err(e) = pipeline.trigger().await {
                warn!("Backup scheduler: could not trigger pipeline: {e}");
            }
        }
    });
}

/// Returns seconds until the next 03:00 UTC.
fn next_scheduled_run_secs() -> u64 {
    use chrono::{Timelike, Utc};
    let now = Utc::now();
    let target_hour = 3u32;
    let secs_today_at_target = target_hour as i64 * 3600;
    let secs_now = now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;
    let secs_until = if secs_now < secs_today_at_target {
        secs_today_at_target - secs_now
    } else {
        // Next day
        86400 - secs_now + secs_today_at_target
    };
    secs_until.max(60) as u64
}
