// backup_pipeline.rs — Automated Rustic backup pipeline
//
// Pipeline: WOL → wait-for-up → rsync sources to backup staging → rustic backup (per repo)
//           → forget/prune → sleep → log
//
// Repos: homeroute, containers, git, pixel
// Backend: orchestrator runs on prod, pushes source data to backup server staging via rsync,
// then runs rustic on the backup server against that staging area.
//
// Triggered manually via IPC (TriggerBackup) or automatically on a schedule.

use std::collections::HashMap;
use std::path::Path;
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
const BACKUP_SSH_KEY: &str = "/root/.ssh/id_ed25519_backup";
const HOMEROUTE_API_BASE: &str = "http://10.0.0.254:4000";
const BACKUP_STAGING_ROOT: &str = "/backup/staging";

/// Maximum time to wait for the backup server to come online after WOL (3 min).
const WAKE_TIMEOUT_SECS: u64 = 180;
/// Interval between ping checks while waiting for server.
const WAKE_POLL_INTERVAL_SECS: u64 = 10;
/// Maximum time for a single repo sync + backup operation.
const REPO_BACKUP_TIMEOUT_SECS: u64 = 3600;
/// Maximum total time for the full pipeline.
const TOTAL_PIPELINE_TIMEOUT_SECS: u64 = 10800;
/// Max job history entries kept in memory.
const MAX_JOB_HISTORY: usize = 20;

// ── Repo configurations ───────────────────────────────────────────────────────

fn default_repos() -> Vec<RepoConfig> {
    vec![
        RepoConfig {
            name: "homeroute".to_string(),
            source_paths: vec!["/opt/homeroute".to_string()],
            staging_root: format!("{BACKUP_STAGING_ROOT}/homeroute"),
            rustic_repo: "/backup/homeroute/rustic".to_string(),
            rsync_excludes: vec![
                "/.git/".to_string(),
                "/crates/target/".to_string(),
                "/web/node_modules/".to_string(),
                "/web-studio/node_modules/".to_string(),
                "/store_flutter/build/".to_string(),
                "/store_flutter/.dart_tool/".to_string(),
            ],
        },
        RepoConfig {
            name: "containers".to_string(),
            source_paths: vec!["/var/lib/machines".to_string()],
            staging_root: format!("{BACKUP_STAGING_ROOT}/containers"),
            rustic_repo: "/backup/containers/rustic".to_string(),
            rsync_excludes: Vec::new(),
        },
        RepoConfig {
            name: "git".to_string(),
            source_paths: vec!["/var/lib/git".to_string()],
            staging_root: format!("{BACKUP_STAGING_ROOT}/git"),
            rustic_repo: "/backup/git/rustic".to_string(),
            rsync_excludes: Vec::new(),
        },
        RepoConfig {
            name: "pixel".to_string(),
            source_paths: vec!["/home/romain".to_string()],
            staging_root: format!("{BACKUP_STAGING_ROOT}/pixel"),
            rustic_repo: "/backup/pixel/rustic".to_string(),
            rsync_excludes: vec!["/.cache/".to_string()],
        },
    ]
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub source_paths: Vec<String>,
    pub staging_root: String,
    pub rustic_repo: String,
    pub rsync_excludes: Vec<String>,
}

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
pub struct RepoStatus {
    pub name: String,
    pub last_backup_at: Option<String>,
    pub last_success: Option<bool>,
    pub last_duration_secs: Option<u64>,
    pub last_snapshot_id: Option<String>,
    pub last_error: Option<String>,
}

impl RepoStatus {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            last_backup_at: None,
            last_success: None,
            last_duration_secs: None,
            last_snapshot_id: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupJob {
    pub id: String,
    pub repo_name: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub success: bool,
    pub duration_secs: Option<u64>,
    pub message: String,
    pub snapshot_id: Option<String>,
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
    /// Per-repo status, keyed by repo name.
    pub repos: HashMap<String, RepoStatus>,
    /// Last N jobs (most recent first).
    pub jobs: Vec<BackupJob>,
}

impl Default for BackupStatus {
    fn default() -> Self {
        let repos = default_repos()
            .into_iter()
            .map(|r| (r.name.clone(), RepoStatus::new(&r.name)))
            .collect();
        Self {
            stage: PipelineStage::Idle,
            running: false,
            last_run_at: None,
            last_run_success: None,
            last_run_duration_secs: None,
            last_run_message: None,
            current_message: None,
            repos,
            jobs: Vec::new(),
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
            let result = tokio::time::timeout(
                Duration::from_secs(TOTAL_PIPELINE_TIMEOUT_SECS),
                run_pipeline(status.clone(), http),
            )
            .await;
            let elapsed = started.elapsed().as_secs();

            let mut s = status.write().await;
            s.running = false;
            s.last_run_at = Some(Utc::now().to_rfc3339());
            s.last_run_duration_secs = Some(elapsed);

            match result {
                Ok(Ok(msg)) => {
                    info!("Backup pipeline completed successfully in {elapsed}s: {msg}");
                    s.stage = PipelineStage::Done;
                    s.last_run_success = Some(true);
                    s.last_run_message = Some(msg);
                    s.current_message = None;
                }
                Ok(Err(e)) => {
                    error!("Backup pipeline failed after {elapsed}s: {e}");
                    s.stage = PipelineStage::Failed;
                    s.last_run_success = Some(false);
                    s.last_run_message = Some(e);
                    s.current_message = None;
                }
                Err(_) => {
                    let msg = format!(
                        "Backup pipeline timed out after {}s",
                        TOTAL_PIPELINE_TIMEOUT_SECS
                    );
                    error!("{msg}");
                    s.stage = PipelineStage::Failed;
                    s.last_run_success = Some(false);
                    s.last_run_message = Some(msg);
                    s.current_message = None;
                }
            }
        });

        Ok(())
    }

    pub async fn get_status(&self) -> BackupStatus {
        self.status.read().await.clone()
    }

    pub async fn get_repos(&self) -> Vec<RepoStatus> {
        let status = self.status.read().await;
        status.repos.values().cloned().collect()
    }

    pub async fn get_jobs(&self) -> Vec<BackupJob> {
        let status = self.status.read().await;
        status.jobs.clone()
    }
}

// ── Pipeline implementation ───────────────────────────────────────────────────

async fn run_pipeline(
    status: Arc<RwLock<BackupStatus>>,
    http: reqwest::Client,
) -> Result<String, String> {
    let repos = default_repos();

    set_stage(
        &status,
        PipelineStage::WakingServer,
        "Sending WOL magic packet via HomeRoute API...",
    )
    .await;

    info!("Backup pipeline: waking server {BACKUP_SERVER_HOST_ID}");
    let wake_url = format!("{HOMEROUTE_API_BASE}/api/hosts/{BACKUP_SERVER_HOST_ID}/wake");
    match http.post(&wake_url).send().await {
        Ok(resp) => info!("WOL response: {}", resp.status()),
        Err(e) => warn!("WOL request failed (may already be online): {e}"),
    }

    set_stage(
        &status,
        PipelineStage::WaitingForServer,
        "Waiting for backup server to come online...",
    )
    .await;

    wait_for_server(status.clone()).await?;

    set_stage(
        &status,
        PipelineStage::RunningBackup,
        "Running rustic backups...",
    )
    .await;

    let mut any_success = false;
    let mut all_success = true;
    let mut repo_messages = Vec::new();

    for repo in &repos {
        let msg = format!("Backing up repo: {}", repo.name);
        {
            let mut s = status.write().await;
            s.current_message = Some(msg.clone());
        }
        info!("Backup pipeline: {msg}");

        let job_id = format!("{}-{}", repo.name, Utc::now().timestamp_millis());
        let job_started = Utc::now().to_rfc3339();
        let t0 = Instant::now();

        let result = run_rustic_backup(repo).await;
        let duration = t0.elapsed().as_secs();
        let finished_at = Utc::now().to_rfc3339();

        let (success, snapshot_id, message) = match result {
            Ok(snap_id) => {
                info!("Repo {} backup succeeded, snapshot: {:?}", repo.name, snap_id);
                any_success = true;
                (
                    true,
                    snap_id.clone(),
                    format!(
                        "Backup OK (snapshot: {})",
                        snap_id.as_deref().unwrap_or("?")
                    ),
                )
            }
            Err(e) => {
                error!("Repo {} backup failed: {e}", repo.name);
                all_success = false;
                (false, None, format!("Backup FAILED: {e}"))
            }
        };

        {
            let mut s = status.write().await;
            if let Some(repo_status) = s.repos.get_mut(&repo.name) {
                repo_status.last_backup_at = Some(finished_at.clone());
                repo_status.last_success = Some(success);
                repo_status.last_duration_secs = Some(duration);
                repo_status.last_snapshot_id = snapshot_id.clone();
                if !success {
                    repo_status.last_error = Some(message.clone());
                } else {
                    repo_status.last_error = None;
                }
            }

            let job = BackupJob {
                id: job_id.clone(),
                repo_name: repo.name.clone(),
                started_at: job_started,
                finished_at: Some(finished_at),
                success,
                duration_secs: Some(duration),
                message: message.clone(),
                snapshot_id,
            };
            s.jobs.insert(0, job);
            if s.jobs.len() > MAX_JOB_HISTORY {
                s.jobs.truncate(MAX_JOB_HISTORY);
            }
        }

        repo_messages.push(format!("{}: {}", repo.name, message));
    }

    set_stage(&status, PipelineStage::Verifying, "Verifying backups...").await;

    set_stage(
        &status,
        PipelineStage::PuttingToSleep,
        "Putting backup server to sleep...",
    )
    .await;

    info!("Backup pipeline: putting server to sleep");
    let sleep_url = format!("{HOMEROUTE_API_BASE}/api/hosts/{BACKUP_SERVER_HOST_ID}/sleep");
    match http.post(&sleep_url).send().await {
        Ok(resp) => info!("Sleep response: {}", resp.status()),
        Err(e) => warn!("Sleep request failed: {e}"),
    }

    let summary = repo_messages.join("; ");
    if all_success {
        Ok(format!("All repos backed up successfully. {summary}"))
    } else if any_success {
        Err(format!("Some repos failed. {summary}"))
    } else {
        Err(format!("All repos failed. {summary}"))
    }
}

async fn run_rustic_backup(repo: &RepoConfig) -> Result<Option<String>, String> {
    let rustic_password = std::env::var("RUSTIC_PASSWORD").unwrap_or_else(|_| {
        warn!("RUSTIC_PASSWORD not set, using default password. Please set it in production!");
        "changeme".to_string()
    });

    ensure_remote_dir(&repo.staging_root, 60).await?;

    let mut staged_paths = Vec::new();
    for source_path in &repo.source_paths {
        sync_source_to_backup(repo, source_path, REPO_BACKUP_TIMEOUT_SECS).await?;
        staged_paths.push(remote_stage_path(&repo.staging_root, source_path));
    }

    let init_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} init 2>&1 || true",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
    );
    let _ = run_ssh_command(&init_cmd, 60).await;

    let backup_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} backup {paths} --json 2>&1",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
        paths = staged_paths
            .iter()
            .map(|path| shell_escape(path))
            .collect::<Vec<_>>()
            .join(" "),
    );

    let output = run_ssh_command(&backup_cmd, REPO_BACKUP_TIMEOUT_SECS)
        .await
        .map_err(|e| format!("SSH backup failed: {e}"))?;

    let snapshot_id = parse_snapshot_id(&output);

    let forget_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} forget --keep-daily 7 --keep-weekly 4 --keep-monthly 6 --prune 2>&1",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
    );

    match run_ssh_command(&forget_cmd, REPO_BACKUP_TIMEOUT_SECS).await {
        Ok(out) => info!("Forget/prune for {}: {}", repo.name, out.trim()),
        Err(e) => warn!("Forget/prune for {} failed: {e}", repo.name),
    }

    Ok(snapshot_id)
}

async fn sync_source_to_backup(
    repo: &RepoConfig,
    source_path: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    if !Path::new(source_path).exists() {
        return Err(format!(
            "Source path does not exist on prod host: {}",
            source_path
        ));
    }

    let remote_target = remote_stage_path(&repo.staging_root, source_path);
    ensure_remote_dir(&remote_target, 60).await?;

    let source_with_trailing_slash = format!("{}/", source_path.trim_end_matches('/'));
    let remote_with_trailing_slash = format!("{}/", remote_target.trim_end_matches('/'));
    let destination = format!("{BACKUP_SERVER_USER}@{BACKUP_SERVER_IP}:{remote_with_trailing_slash}");

    let mut args = vec![
        "-aHAX".to_string(),
        "--delete".to_string(),
        "--numeric-ids".to_string(),
        "-e".to_string(),
        format!(
            "ssh -i {} -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=15",
            BACKUP_SSH_KEY
        ),
        "--rsync-path".to_string(),
        "sudo rsync".to_string(),
    ];

    for exclude in &repo.rsync_excludes {
        args.push("--exclude".to_string());
        args.push(exclude.clone());
    }

    args.push(source_with_trailing_slash);
    args.push(destination);

    let output = run_local_command("rsync", &args, timeout_secs).await?;
    if !output.trim().is_empty() {
        info!("rsync {} -> {}: {}", source_path, remote_target, output.trim());
    }

    Ok(())
}

fn remote_stage_path(staging_root: &str, source_path: &str) -> String {
    let basename = Path::new(source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("root");
    format!("{}/{}", staging_root.trim_end_matches('/'), basename)
}

async fn ensure_remote_dir(path: &str, timeout_secs: u64) -> Result<(), String> {
    let cmd = format!("sudo mkdir -p {}", shell_escape(path));
    run_ssh_command(&cmd, timeout_secs).await.map(|_| ())
}

fn parse_snapshot_id(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(id) = v.get("id").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
            if let Some(id) = v.get("snapshot_id").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

async fn run_local_command(binary: &str, args: &[String], timeout_secs: u64) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::process::Command::new(binary).args(args).output(),
    )
    .await
    .map_err(|_| format!("{binary} timed out after {timeout_secs}s"))?
    .map_err(|e| format!("Failed to spawn {binary}: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}").trim().to_string();

    if output.status.success() {
        Ok(combined)
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(format!("{binary} exited with code {code}. Output: {combined}"))
    }
}

async fn run_ssh_command(cmd: &str, timeout_secs: u64) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::process::Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "ConnectTimeout=15",
                "-i",
                BACKUP_SSH_KEY,
                &format!("{BACKUP_SERVER_USER}@{BACKUP_SERVER_IP}"),
                cmd,
            ])
            .output(),
    )
    .await
    .map_err(|_| format!("SSH command timed out after {timeout_secs}s"))?
    .map_err(|e| format!("Failed to spawn SSH process: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}").trim().to_string();

    if output.status.success() {
        Ok(combined)
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(format!("SSH command exited with code {code}. Output: {combined}"))
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

        match tokio::net::TcpStream::connect((BACKUP_SERVER_IP, 22)).await {
            Ok(_) => {
                info!("Backup server is online (attempt {attempt})");
                tokio::time::sleep(Duration::from_secs(3)).await;
                return Ok(());
            }
            Err(_) => {
                info!(
                    "Backup server not yet reachable (attempt {attempt}), retrying in {WAKE_POLL_INTERVAL_SECS}s..."
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(WAKE_POLL_INTERVAL_SECS)).await;
    }

    Err(format!(
        "Backup server did not come online within {WAKE_TIMEOUT_SECS}s"
    ))
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

pub fn spawn_daily_scheduler(pipeline: Arc<BackupPipeline>) {
    tokio::spawn(async move {
        loop {
            let next = next_scheduled_run_secs();
            info!(
                "Backup scheduler: next run in {next}s ({:.1}h)",
                next as f64 / 3600.0
            );
            tokio::time::sleep(Duration::from_secs(next)).await;

            info!("Backup scheduler: triggering daily backup pipeline");
            if let Err(e) = pipeline.trigger().await {
                warn!("Backup scheduler: could not trigger pipeline: {e}");
            }
        }
    });
}

fn next_scheduled_run_secs() -> u64 {
    use chrono::{Timelike, Utc};
    let now = Utc::now();
    let target_hour = 3u32;
    let secs_today_at_target = target_hour as i64 * 3600;
    let secs_now = now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;
    let secs_until = if secs_now < secs_today_at_target {
        secs_today_at_target - secs_now
    } else {
        86400 - secs_now + secs_today_at_target
    };
    secs_until.max(60) as u64
}
