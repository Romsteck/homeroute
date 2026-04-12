use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use hr_common::events::{BackupLiveEvent, EventBus};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const TOTAL_PIPELINE_TIMEOUT_SECS: u64 = 14400;
const MAX_JOB_HISTORY: usize = 20;

// ── SSH backup target ────────────────────────────────────────────
const BACKUP_SSH_USER: &str = "romain";
const BACKUP_SSH_HOST: &str = "10.0.0.30";
const BACKUP_BASE_DIR: &str = "/backup";

// ── Repo config ────────────────────────────────────────────────────

fn default_repos() -> Vec<RepoConfig> {
    vec![
        RepoConfig {
            name: "homeroute".to_string(),
            source_path: "/opt/homeroute/data".to_string(),
            excludes: vec![],
        },
        RepoConfig {
            name: "containers".to_string(),
            source_path: "/var/lib/machines".to_string(),
            excludes: vec![],
        },
        RepoConfig {
            name: "git".to_string(),
            source_path: "/opt/homeroute/data/git/repos".to_string(),
            excludes: vec![],
        },
        RepoConfig {
            name: "pixel".to_string(),
            source_path: "/root/.openclaw/workspace".to_string(),
            excludes: vec![".cache".to_string()],
        },
        RepoConfig {
            name: "homecloud".to_string(),
            source_path: "/ssd_pool/homecloud/data".to_string(),
            excludes: vec![],
        },
    ]
}

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub source_path: String,
    pub excludes: Vec<String>,
}

// ── Status types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    Idle,
    RunningBackup,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackupPhase {
    Idle,
    Scanning,
    Transferring,
    Verifying,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatus {
    pub name: String,
    pub last_backup_at: Option<String>,
    pub last_success: Option<bool>,
    pub last_duration_secs: Option<u64>,
    pub last_error: Option<String>,
    pub last_files_total: Option<u64>,
    pub last_files_changed: Option<u64>,
    pub last_transferred_bytes: Option<u64>,
    pub last_total_size: Option<u64>,
}

impl RepoStatus {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            last_backup_at: None,
            last_success: None,
            last_duration_secs: None,
            last_error: None,
            last_files_total: None,
            last_files_changed: None,
            last_transferred_bytes: None,
            last_total_size: None,
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
    pub files_total: Option<u64>,
    pub files_changed: Option<u64>,
    pub transferred_bytes: Option<u64>,
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
    pub repos: HashMap<String, RepoStatus>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProgress {
    pub running: bool,
    pub current_repo: Option<String>,
    pub phase: BackupPhase,
    pub progress: f64,
    pub files_total: Option<u64>,
    pub files_changed: Option<u64>,
    pub bytes_transferred: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed: Option<String>,
    pub remaining_secs: Option<u64>,
    pub elapsed_secs: u64,
    pub started_at: Option<String>,
    pub detail: Option<String>,
}

impl Default for BackupProgress {
    fn default() -> Self {
        Self {
            running: false,
            current_repo: None,
            phase: BackupPhase::Idle,
            progress: 0.0,
            files_total: None,
            files_changed: None,
            bytes_transferred: None,
            total_bytes: None,
            speed: None,
            remaining_secs: None,
            elapsed_secs: 0,
            started_at: None,
            detail: None,
        }
    }
}

// ── SSH session (ControlMaster multiplexing) ──────────────────────

struct SshSession {
    control_path: PathBuf,
}

impl SshSession {
    async fn start() -> Result<Self, String> {
        let control_path = PathBuf::from("/tmp/hr-backup-ssh-ctrl");

        // Clean up any stale socket
        let _ = tokio::fs::remove_file(&control_path).await;

        let output = tokio::process::Command::new("ssh")
            .args([
                "-o",
                "ControlMaster=yes",
                "-o",
                &format!("ControlPath={}", control_path.display()),
                "-o",
                "ControlPersist=600",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-fN",
                &format!("{BACKUP_SSH_USER}@{BACKUP_SSH_HOST}"),
            ])
            .output()
            .await
            .map_err(|e| format!("Failed to start SSH ControlMaster: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("SSH ControlMaster failed: {stderr}"));
        }

        // Wait briefly for the socket to be ready
        for _ in 0..20 {
            if control_path.exists() {
                return Ok(Self { control_path });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(Self { control_path })
    }

    fn ssh_cmd_str(&self) -> String {
        format!(
            "ssh -o ControlPath={} -o BatchMode=yes",
            self.control_path.display()
        )
    }

    async fn run_command(&self, cmd: &str) -> Result<String, String> {
        let output = tokio::process::Command::new("ssh")
            .args([
                "-o",
                &format!("ControlPath={}", self.control_path.display()),
                "-o",
                "BatchMode=yes",
                &format!("{BACKUP_SSH_USER}@{BACKUP_SSH_HOST}"),
            ])
            .arg(cmd)
            .output()
            .await
            .map_err(|e| format!("SSH command failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "SSH command failed (exit {}): {stderr}",
                output.status
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn close(&self) {
        let _ = tokio::process::Command::new("ssh")
            .args([
                "-o",
                &format!("ControlPath={}", self.control_path.display()),
                "-O",
                "exit",
                &format!("{BACKUP_SSH_USER}@{BACKUP_SSH_HOST}"),
            ])
            .output()
            .await;
        let _ = tokio::fs::remove_file(&self.control_path).await;
    }
}

// ── BackupPipeline ─────────────────────────────────────────────────

pub struct BackupPipeline {
    pub status: Arc<RwLock<BackupStatus>>,
    pub progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
    cancelled: Arc<AtomicBool>,
    rsync_pid: Arc<RwLock<Option<u32>>>,
}

impl BackupPipeline {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            status: Arc::new(RwLock::new(BackupStatus::default())),
            progress: Arc::new(RwLock::new(BackupProgress::default())),
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
            rsync_pid: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn cancel(&self) -> Result<(), String> {
        {
            let status = self.status.read().await;
            if !status.running {
                return Err("No backup running".to_string());
            }
        }

        info!("Cancelling backup pipeline");
        self.cancelled.store(true, Ordering::SeqCst);

        // Kill rsync process if running
        if let Some(pid) = *self.rsync_pid.read().await {
            info!("Killing rsync process (PID {pid})");
            let _ = tokio::process::Command::new("kill")
                .arg(pid.to_string())
                .output()
                .await;
        }

        // Update status
        {
            let mut s = self.status.write().await;
            s.running = false;
            s.stage = PipelineStage::Idle;
            s.current_message = None;
            s.last_run_success = Some(false);
            s.last_run_message = Some("Annulé par l'utilisateur".to_string());
        }
        {
            let mut p = self.progress.write().await;
            p.running = false;
            p.phase = BackupPhase::Idle;
        }

        emit_backup_live(&self.events, &self.status, &self.progress).await;
        Ok(())
    }

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
        {
            let mut status = self.status.write().await;
            status.running = true;
            status.stage = PipelineStage::RunningBackup;
            status.current_message = Some("Initialisation du pipeline de backup...".to_string());
        }
        {
            let mut progress = self.progress.write().await;
            *progress = BackupProgress {
                running: true,
                phase: BackupPhase::Idle,
                started_at: Some(Utc::now().to_rfc3339()),
                detail: Some("Initialisation du pipeline".to_string()),
                ..BackupProgress::default()
            };
        }

        emit_backup_live(&self.events, &self.status, &self.progress).await;

        self.cancelled.store(false, Ordering::SeqCst);

        let status = self.status.clone();
        let progress = self.progress.clone();
        let events = self.events.clone();
        let cancelled = self.cancelled.clone();
        let rsync_pid = self.rsync_pid.clone();

        tokio::spawn(async move {
            let started = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(TOTAL_PIPELINE_TIMEOUT_SECS),
                run_pipeline(
                    status.clone(),
                    progress.clone(),
                    events.clone(),
                    cancelled.clone(),
                    rsync_pid.clone(),
                ),
            )
            .await;
            let elapsed = started.elapsed().as_secs();

            {
                let mut s = status.write().await;
                s.running = false;
                s.last_run_at = Some(Utc::now().to_rfc3339());
                s.last_run_duration_secs = Some(elapsed);

                match &result {
                    Ok(Ok(msg)) => {
                        info!("Backup pipeline completed in {elapsed}s: {msg}");
                        s.stage = PipelineStage::Done;
                        s.last_run_success = Some(true);
                        s.last_run_message = Some(msg.clone());
                        s.current_message = None;
                    }
                    Ok(Err(e)) => {
                        error!("Backup pipeline failed after {elapsed}s: {e}");
                        s.stage = PipelineStage::Failed;
                        s.last_run_success = Some(false);
                        s.last_run_message = Some(e.clone());
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
            }

            {
                let mut p = progress.write().await;
                p.running = false;
                p.elapsed_secs = elapsed;
                p.phase = if matches!(result, Ok(Ok(_))) {
                    BackupPhase::Done
                } else {
                    BackupPhase::Failed
                };
                if matches!(result, Ok(Ok(_))) {
                    p.progress = 100.0;
                }
            }

            emit_backup_live(&events, &status, &progress).await;
        });

        Ok(())
    }

    pub async fn get_status(&self) -> BackupStatus {
        self.status.read().await.clone()
    }

    pub async fn get_repos(&self) -> Vec<RepoStatus> {
        self.status.read().await.repos.values().cloned().collect()
    }

    pub async fn get_jobs(&self) -> Vec<BackupJob> {
        self.status.read().await.jobs.clone()
    }

    pub async fn get_progress(&self) -> BackupProgress {
        self.progress.read().await.clone()
    }
}

// ── Event emission ─────────────────────────────────────────────────

async fn emit_backup_live(
    events: &EventBus,
    status: &RwLock<BackupStatus>,
    progress: &RwLock<BackupProgress>,
) {
    let s = status.read().await;
    let p = progress.read().await;
    let event = BackupLiveEvent {
        status: serde_json::json!({
            "running": s.running,
            "stage": s.stage,
            "current_message": s.current_message,
        }),
        progress: serde_json::to_value(&*p).unwrap_or_default(),
        repos: s
            .repos
            .values()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect(),
        latest_job: s
            .jobs
            .first()
            .map(|j| serde_json::to_value(j).unwrap_or_default()),
    };
    let _ = events.backup_live.send(event);
}

// ── Pipeline ───────────────────────────────────────────────────────

async fn run_pipeline(
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
    cancelled: Arc<AtomicBool>,
    rsync_pid: Arc<RwLock<Option<u32>>>,
) -> Result<String, String> {
    let repos = default_repos();

    // Establish SSH connection to backup server
    info!("Establishing SSH connection to {BACKUP_SSH_USER}@{BACKUP_SSH_HOST}");
    {
        let mut s = status.write().await;
        s.current_message = Some("Connexion SSH au serveur de backup...".to_string());
    }
    emit_backup_live(&events, &status, &progress).await;

    let ssh = SshSession::start().await?;

    // Ensure base backup directory exists on remote
    ssh.run_command(&format!("mkdir -p {BACKUP_BASE_DIR}"))
        .await?;

    let mut any_success = false;
    let mut all_success = true;
    let mut repo_messages = Vec::new();

    for repo in repos.iter() {
        if cancelled.load(Ordering::SeqCst) {
            ssh.close().await;
            return Err("Annulé par l'utilisateur".to_string());
        }
        {
            let mut s = status.write().await;
            s.current_message = Some(format!("Backup {}...", repo.name));
        }
        {
            let mut p = progress.write().await;
            p.running = true;
            p.current_repo = Some(repo.name.clone());
            p.phase = BackupPhase::Scanning;
            p.progress = 0.0;
            p.files_total = Some(0);
            p.files_changed = Some(0);
            p.bytes_transferred = Some(0);
            p.total_bytes = Some(0);
            p.speed = Some("0 B/s".to_string());
            p.remaining_secs = Some(0);
            p.detail = Some(format!("Scan {}...", repo.name));
        }
        emit_backup_live(&events, &status, &progress).await;

        let job_id = format!("{}-{}", repo.name, Utc::now().timestamp_millis());
        let job_started = Utc::now().to_rfc3339();
        let t0 = Instant::now();

        let result = run_repo_backup(
            repo,
            &ssh,
            status.clone(),
            progress.clone(),
            events.clone(),
            cancelled.clone(),
            rsync_pid.clone(),
        )
        .await;
        let duration = t0.elapsed().as_secs();
        let finished_at = Utc::now().to_rfc3339();

        let (success, message, files_total, files_changed, transferred_bytes) = match result {
            Ok(stats) => {
                any_success = true;
                let msg = format!(
                    "OK: {} fichiers, {} modifiés, {} transférés",
                    stats.files_total,
                    stats.files_changed,
                    format_bytes(stats.transferred_bytes),
                );
                (
                    true,
                    msg,
                    Some(stats.files_total),
                    Some(stats.files_changed),
                    Some(stats.transferred_bytes),
                )
            }
            Err(e) => {
                all_success = false;
                (false, format!("FAILED: {e}"), None, None, None)
            }
        };

        {
            let mut s = status.write().await;
            if let Some(repo_status) = s.repos.get_mut(&repo.name) {
                repo_status.last_backup_at = Some(finished_at.clone());
                repo_status.last_success = Some(success);
                repo_status.last_duration_secs = Some(duration);
                repo_status.last_files_total = files_total;
                repo_status.last_files_changed = files_changed;
                repo_status.last_transferred_bytes = transferred_bytes;
                repo_status.last_error = if success { None } else { Some(message.clone()) };
            }

            s.jobs.insert(
                0,
                BackupJob {
                    id: job_id,
                    repo_name: repo.name.clone(),
                    started_at: job_started,
                    finished_at: Some(finished_at),
                    success,
                    duration_secs: Some(duration),
                    message: message.clone(),
                    files_total,
                    files_changed,
                    transferred_bytes,
                },
            );
            if s.jobs.len() > MAX_JOB_HISTORY {
                s.jobs.truncate(MAX_JOB_HISTORY);
            }
        }

        emit_backup_live(&events, &status, &progress).await;
        repo_messages.push(format!("{}: {}", repo.name, message));
    }

    // Close SSH session
    ssh.close().await;

    let summary = repo_messages.join("; ");
    if all_success {
        Ok(format!("All repos backed up successfully. {summary}"))
    } else if any_success {
        Err(format!("Some repos failed. {summary}"))
    } else {
        Err(format!("All repos failed. {summary}"))
    }
}

// ── Rsync stats ──────────────────────────────────────────────────

struct RsyncStats {
    files_total: u64,
    files_changed: u64,
    transferred_bytes: u64,
}

// ── Rsync progress parsing ───────────────────────────────────────

struct RsyncProgressUpdate {
    bytes_transferred: u64,
    percentage: f64,
    remaining_secs: Option<u64>,
    files_transferred: Option<u64>,
    files_total: Option<u64>,
}

/// Parse a line from rsync --info=progress2 output.
///
/// Format: `  1,234,567  42%  123.45MB/s    0:02:30 (xfr#5, to-chk=95/100)`
fn parse_progress2_line(line: &str) -> Option<RsyncProgressUpdate> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    // First field: bytes transferred (with commas)
    let bytes_str = parts[0].replace(',', "").replace('.', "");
    let bytes: u64 = bytes_str.parse().ok()?;

    // Second field: percentage (e.g. "42%")
    let pct_str = parts[1].strip_suffix('%')?;
    let pct: f64 = pct_str.parse().ok()?;

    // Third field: speed (e.g. "123.45MB/s")
    let speed = parts[2].to_string();

    // Fourth field: ETA (e.g. "0:02:30")
    let remaining_secs = parse_eta_to_secs(parts[3]);

    // Optional: xfr# and to-chk fields
    let mut files_transferred = None;
    let mut files_total = None;

    let remainder = parts[4..].join(" ");
    if let Some(xfr_start) = remainder.find("xfr#") {
        let after = &remainder[xfr_start + 4..];
        if let Some(end) = after.find(|c: char| !c.is_ascii_digit()) {
            files_transferred = after[..end].parse().ok();
        } else {
            files_transferred = after.parse().ok();
        }
    }
    if let Some(chk_start) = remainder.find("to-chk=") {
        let after = &remainder[chk_start + 7..];
        if let Some((remaining, total)) = after.split_once('/') {
            let remaining_count: u64 = remaining.parse().ok()?;
            let total_str = total.trim_end_matches(')');
            let total_count: u64 = total_str.parse().ok()?;
            files_total = Some(total_count);
            // files_transferred = total - remaining (if xfr not found)
            if files_transferred.is_none() {
                files_transferred = Some(total_count.saturating_sub(remaining_count));
            }
        }
    }
    // ir-chk is used during incremental recursion
    if files_total.is_none() {
        if let Some(chk_start) = remainder.find("ir-chk=") {
            let after = &remainder[chk_start + 7..];
            if let Some((_remaining, total)) = after.split_once('/') {
                let total_str = total.trim_end_matches(')');
                files_total = total_str.parse().ok();
            }
        }
    }

    // speed is parsed but not used (we compute our own EMA-smoothed speed)
    let _ = speed;

    Some(RsyncProgressUpdate {
        bytes_transferred: bytes,
        percentage: pct,
        remaining_secs,
        files_transferred,
        files_total,
    })
}

/// Parse rsync ETA format "H:MM:SS" or "M:SS" into seconds.
fn parse_eta_to_secs(eta: &str) -> Option<u64> {
    let parts: Vec<&str> = eta.split(':').collect();
    match parts.len() {
        3 => {
            let h: u64 = parts[0].parse().ok()?;
            let m: u64 = parts[1].parse().ok()?;
            let s: u64 = parts[2].parse().ok()?;
            Some(h * 3600 + m * 60 + s)
        }
        2 => {
            let m: u64 = parts[0].parse().ok()?;
            let s: u64 = parts[1].parse().ok()?;
            Some(m * 60 + s)
        }
        _ => None,
    }
}

/// Parse rsync --stats summary lines from stdout.
/// Extracts: Number of files, Number of regular files transferred, Total transferred file size.
fn parse_rsync_stats(output: &str) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
    let mut files_total = None;
    let mut files_changed = None;
    let mut transferred_bytes = None;
    let mut total_size = None;

    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Number of files:") {
            // "Number of files: 1,234 (reg: 900, dir: 334)"
            let num_str = rest.trim().split_whitespace().next().unwrap_or("");
            files_total = num_str.replace(',', "").replace('.', "").parse().ok();
        } else if let Some(rest) = line.strip_prefix("Number of regular files transferred:") {
            files_changed = rest.trim().replace(',', "").replace('.', "").parse().ok();
        } else if let Some(rest) = line.strip_prefix("Total transferred file size:") {
            // "Total transferred file size: 1,234,567 bytes"
            let num_str = rest.trim().split_whitespace().next().unwrap_or("");
            transferred_bytes = num_str.replace(',', "").replace('.', "").parse().ok();
        } else if let Some(rest) = line.strip_prefix("Total file size:") {
            let num_str = rest.trim().split_whitespace().next().unwrap_or("");
            total_size = num_str.replace(',', "").replace('.', "").parse().ok();
        }
    }

    (files_total, files_changed, transferred_bytes, total_size)
}

// ── Per-repo backup using rsync ──────────────────────────────────

async fn run_repo_backup(
    repo: &RepoConfig,
    ssh: &SshSession,
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
    cancelled: Arc<AtomicBool>,
    rsync_pid: Arc<RwLock<Option<u32>>>,
) -> Result<RsyncStats, String> {
    let source_path = std::path::Path::new(&repo.source_path);
    if !source_path.exists() {
        info!(repo = repo.name, path = %repo.source_path, "Source directory does not exist, skipping");
        return Ok(RsyncStats {
            files_total: 0,
            files_changed: 0,
            transferred_bytes: 0,
        });
    }

    let remote_dest = format!(
        "{BACKUP_SSH_USER}@{BACKUP_SSH_HOST}:{BACKUP_BASE_DIR}/{}/",
        repo.name
    );

    // Ensure remote directory exists
    ssh.run_command(&format!("mkdir -p '{BACKUP_BASE_DIR}/{}'", repo.name))
        .await?;

    // Pre-count files with find for scanning phase feedback
    let file_count = match tokio::process::Command::new("find")
        .arg(&repo.source_path)
        .arg("-type")
        .arg("f")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(output) => {
            let count = output.stdout.iter().filter(|&&b| b == b'\n').count() as u64;
            info!(repo = repo.name, file_count = count, "Pre-scan file count");
            count
        }
        Err(e) => {
            warn!(repo = repo.name, "find pre-count failed: {e}");
            0
        }
    };

    // Update progress with pre-scan count
    {
        let mut p = progress.write().await;
        p.files_total = Some(file_count);
        p.detail = Some(format!("Scan {}… {} fichiers", repo.name, file_count));
    }
    info!(
        "WS backup:live → phase=Scanning files_total={} repo={}",
        file_count, repo.name
    );
    emit_backup_live(&events, &status, &progress).await;

    // Build rsync command
    // --archive: preserve permissions, times, symlinks, etc.
    // --delete: remove files on dest that don't exist on source (differential)
    // --info=progress2: show overall transfer progress
    // --no-inc-recursive: full file list scan upfront for accurate progress %
    // LC_ALL=C: ensure consistent number formatting (no locale-specific decimals)
    let ssh_cmd = ssh.ssh_cmd_str();

    // Use stdbuf -oL to force line-buffered stdout from rsync (avoids pipe buffering)
    let mut cmd = tokio::process::Command::new("stdbuf");
    cmd.env("LC_ALL", "C");
    cmd.args(["-oL", "rsync"]);
    cmd.args([
        "--archive",
        "--delete",
        "--info=progress2",
        "--no-inc-recursive",
        "--stats",
    ]);
    cmd.args(["-e", &ssh_cmd]);

    for excl in &repo.excludes {
        cmd.args(["--exclude", excl]);
    }

    // Ensure source path ends with /
    let source = if repo.source_path.ends_with('/') {
        repo.source_path.clone()
    } else {
        format!("{}/", repo.source_path)
    };

    cmd.arg(&source);
    cmd.arg(&remote_dest);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    info!(repo = repo.name, source = %source, dest = %remote_dest, "Starting rsync");

    // Pre-measure source size with du -sb for stable total_bytes (Problem 1)
    let pre_total_bytes: u64 = match tokio::process::Command::new("du")
        .args(["-sb", &repo.source_path])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout);
            out.split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        }
        _ => 0,
    };
    if pre_total_bytes > 0 {
        info!(
            repo = repo.name,
            pre_total_bytes, "Pre-measured source size with du"
        );
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn rsync for {}: {e}", repo.name))?;

    // Store PID for cancellation (Problem 2)
    if let Some(pid) = child.id() {
        *rsync_pid.write().await = Some(pid);
    }

    let stdout = child.stdout.take().ok_or("No rsync stdout")?;

    // Read rsync output in real-time, splitting on \r and \n
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();
    let mut saw_progress = false;
    let rsync_start = Instant::now();
    let mut stats_output = String::new();

    // Track latest stats from rsync
    let mut last_files_total: u64 = 0;
    let mut last_files_changed: u64 = 0;
    let mut last_bytes_transferred: u64 = 0;
    let mut last_broadcast = Instant::now();

    // EMA smoothing for speed (Problem 4)
    let ema_alpha: f64 = 0.1;
    let mut ema_speed: f64 = 0.0; // bytes per second
    let mut last_ema_time = Instant::now();
    let mut last_ema_bytes: u64 = 0;

    loop {
        // Check cancellation
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            *rsync_pid.write().await = None;
            return Err("Annulé par l'utilisateur".to_string());
        }

        let n = reader
            .read(&mut buf)
            .await
            .map_err(|e| format!("Failed to read rsync output: {e}"))?;
        if n == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buf[..n]);
        for ch in chunk.chars() {
            if ch == '\r' || ch == '\n' {
                if !line_buf.is_empty() {
                    if let Some(update) = parse_progress2_line(&line_buf) {
                        // This is a progress line
                        last_bytes_transferred = update.bytes_transferred;
                        if let Some(ft) = update.files_total {
                            last_files_total = ft;
                        }
                        if let Some(fc) = update.files_transferred {
                            last_files_changed = fc;
                        }

                        // Problem 1: Use pre-measured total from du -sb (stable)
                        // Fall back to rsync percentage estimate only if du failed
                        let total_bytes = if pre_total_bytes > 0 {
                            pre_total_bytes
                        } else if update.percentage > 0.0 {
                            (update.bytes_transferred as f64 / (update.percentage / 100.0)) as u64
                        } else {
                            0
                        };

                        // Recompute progress from stable total_bytes
                        let progress_pct = if total_bytes > 0 {
                            (update.bytes_transferred as f64 / total_bytes as f64 * 100.0)
                                .min(100.0)
                        } else {
                            update.percentage
                        };

                        let elapsed = rsync_start.elapsed().as_secs();

                        // Problem 4: EMA smoothing for speed
                        let now_ema = Instant::now();
                        let dt = now_ema.duration_since(last_ema_time).as_secs_f64();
                        if dt > 0.05 {
                            let bytes_delta =
                                update.bytes_transferred.saturating_sub(last_ema_bytes) as f64;
                            let current_speed = bytes_delta / dt;
                            if ema_speed < 1.0 {
                                ema_speed = current_speed; // init on first sample
                            } else {
                                ema_speed =
                                    ema_alpha * current_speed + (1.0 - ema_alpha) * ema_speed;
                            }
                            last_ema_time = now_ema;
                            last_ema_bytes = update.bytes_transferred;
                        }
                        let smooth_remaining =
                            if ema_speed > 1.0 && total_bytes > update.bytes_transferred {
                                ((total_bytes - update.bytes_transferred) as f64 / ema_speed) as u64
                            } else {
                                update.remaining_secs.unwrap_or(0)
                            };
                        let smooth_speed_str = format_speed(ema_speed);

                        // Phase transition: scanning → transferring on first progress line
                        if !saw_progress {
                            saw_progress = true;
                        }

                        // Update in-memory progress on every line, but throttle WS broadcasts
                        {
                            let mut p = progress.write().await;
                            p.phase = BackupPhase::Transferring;
                            p.progress = progress_pct;
                            p.bytes_transferred = Some(update.bytes_transferred);
                            p.total_bytes = Some(total_bytes);
                            p.speed = Some(smooth_speed_str.clone());
                            p.remaining_secs = Some(smooth_remaining);
                            p.files_total = Some(last_files_total);
                            p.files_changed = Some(last_files_changed);
                            p.elapsed_secs = elapsed;
                            p.detail = Some(format!(
                                "{:.0}% — {} — {}",
                                progress_pct,
                                format_bytes(update.bytes_transferred),
                                smooth_speed_str,
                            ));
                        }
                        {
                            let mut s = status.write().await;
                            s.current_message = Some(format!(
                                "Transfert {} ({:.0}%) — {}",
                                repo.name, progress_pct, smooth_speed_str,
                            ));
                        }
                        // Throttle WS broadcasts to max once per 100ms
                        if last_broadcast.elapsed() >= Duration::from_millis(100) {
                            last_broadcast = Instant::now();
                            info!(
                                "WS backup:live → phase=Transferring progress={:.1}% files_total={} files_changed={} speed={} bytes={}",
                                progress_pct,
                                last_files_total,
                                last_files_changed,
                                smooth_speed_str,
                                update.bytes_transferred
                            );
                            emit_backup_live(&events, &status, &progress).await;
                        }
                    } else {
                        // Non-progress line (stats summary, file list, etc.)
                        stats_output.push_str(&line_buf);
                        stats_output.push('\n');
                    }
                    line_buf.clear();
                }
            } else {
                line_buf.push(ch);
            }
        }
    }

    // Clear PID
    *rsync_pid.write().await = None;

    // Check cancellation after loop exit
    if cancelled.load(Ordering::SeqCst) {
        let _ = child.kill().await;
        return Err("Annulé par l'utilisateur".to_string());
    }

    // Wait for rsync to finish
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("rsync wait failed for {}: {e}", repo.name))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // rsync exit codes: 0 = success, 24 = partial transfer (vanished files, non-critical)
    if exit_code != 0 && exit_code != 24 {
        error!(
            repo = repo.name,
            exit_code,
            stderr = %stderr.trim(),
            "rsync failed"
        );
        return Err(format!(
            "rsync exited with code {exit_code}: {}",
            stderr.trim()
        ));
    }

    if exit_code == 24 {
        warn!(
            repo = repo.name,
            "rsync completed with partial transfer (exit 24): some files vanished"
        );
    }

    // Parse --stats summary from collected stdout lines
    let (stat_files_total, stat_files_changed, stat_transferred, stat_total_size) =
        parse_rsync_stats(&stats_output);
    // Prefer stats summary over progress2 counters (more accurate)
    if let Some(ft) = stat_files_total {
        last_files_total = ft;
    }
    if let Some(fc) = stat_files_changed {
        last_files_changed = fc;
    }
    if let Some(tb) = stat_transferred {
        if tb > 0 || last_bytes_transferred == 0 {
            last_bytes_transferred = tb;
        }
    }

    // Final progress update
    {
        let mut p = progress.write().await;
        p.progress = 100.0;
        p.phase = BackupPhase::Verifying;
        p.bytes_transferred = Some(last_bytes_transferred);
        p.total_bytes = stat_total_size.or(Some(0));
        p.files_total = Some(last_files_total);
        p.files_changed = Some(last_files_changed);
        p.speed = Some("0 B/s".to_string());
        p.remaining_secs = Some(0);
        p.elapsed_secs = rsync_start.elapsed().as_secs();
        p.detail = Some(format!("Vérification {}...", repo.name));
    }
    {
        let mut s = status.write().await;
        s.current_message = Some(format!("Vérification {}...", repo.name));
    }
    info!(
        "WS backup:live → phase=Verifying files_total={} files_changed={} bytes={}",
        last_files_total, last_files_changed, last_bytes_transferred
    );
    emit_backup_live(&events, &status, &progress).await;

    let files_total = last_files_total;
    let files_changed = last_files_changed;

    info!(
        repo = repo.name,
        files_total,
        files_changed,
        transferred_bytes = last_bytes_transferred,
        exit_code,
        "rsync completed"
    );

    Ok(RsyncStats {
        files_total,
        files_changed,
        transferred_bytes: last_bytes_transferred,
    })
}

// ── Formatting helpers ─────────────────────────────────────────────

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0} B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else if bytes_per_sec < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB/s", bytes_per_sec / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ── Daily scheduler ────────────────────────────────────────────────

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

/// Schedule at 20:00 UTC = 22:00 Brussels (CEST) / 21:00 Brussels (CET)
fn next_scheduled_run_secs() -> u64 {
    use chrono::{Timelike, Utc};
    let now = Utc::now();
    let target_hour = 20u32;
    let secs_today_at_target = target_hour as i64 * 3600;
    let secs_now = now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;
    let secs_until = if secs_now < secs_today_at_target {
        secs_today_at_target - secs_now
    } else {
        86400 - secs_now + secs_today_at_target
    };
    secs_until.max(60) as u64
}
