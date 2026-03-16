use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use hr_common::events::{BackupLiveEvent, EventBus};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const TOTAL_PIPELINE_TIMEOUT_SECS: u64 = 14400;
const MAX_JOB_HISTORY: usize = 20;
const EMIT_THROTTLE_MS: u64 = 200;

// ── Borg repo base path ──────────────────────────────────────────
const BORG_BASE_DIR: &str = "/ssd_pool/backup";

// ── Repo config ────────────────────────────────────────────────────

fn default_repos() -> Vec<RepoConfig> {
    vec![
        RepoConfig {
            name: "homeroute".to_string(),
            source_path: "/opt/homeroute".to_string(),
            excludes: vec![
                ".git".to_string(),
                "crates/target".to_string(),
                "web/node_modules".to_string(),
                "web-studio/node_modules".to_string(),
                "store_flutter/build".to_string(),
                "store_flutter/.dart_tool".to_string(),
            ],
        },
        RepoConfig {
            name: "pixel".to_string(),
            source_path: "/home/romain".to_string(),
            excludes: vec![".cache".to_string()],
        },
        RepoConfig {
            name: "containers".to_string(),
            source_path: "/var/lib/machines".to_string(),
            excludes: Vec::new(),
        },
        RepoConfig {
            name: "git".to_string(),
            source_path: "/opt/homeroute/data/git/repos".to_string(),
            excludes: Vec::new(),
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
    Creating,
    Pruning,
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
    pub last_archive_name: Option<String>,
    pub last_original_size: Option<u64>,
    pub last_deduplicated_size: Option<u64>,
    pub last_nfiles: Option<u64>,
}

impl RepoStatus {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            last_backup_at: None,
            last_success: None,
            last_duration_secs: None,
            last_error: None,
            last_archive_name: None,
            last_original_size: None,
            last_deduplicated_size: None,
            last_nfiles: None,
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
    pub archive_name: Option<String>,
    pub original_size: Option<u64>,
    pub deduplicated_size: Option<u64>,
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
    pub nfiles: Option<u64>,
    pub original_size: Option<u64>,
    pub deduplicated_size: Option<u64>,
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
            nfiles: None,
            original_size: None,
            deduplicated_size: None,
            elapsed_secs: 0,
            started_at: None,
            detail: None,
        }
    }
}

// ── BackupPipeline ─────────────────────────────────────────────────

pub struct BackupPipeline {
    pub status: Arc<RwLock<BackupStatus>>,
    pub progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
}

impl BackupPipeline {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            status: Arc::new(RwLock::new(BackupStatus::default())),
            progress: Arc::new(RwLock::new(BackupProgress::default())),
            events,
        }
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

        let status = self.status.clone();
        let progress = self.progress.clone();
        let events = self.events.clone();

        tokio::spawn(async move {
            let started = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(TOTAL_PIPELINE_TIMEOUT_SECS),
                run_pipeline(status.clone(), progress.clone(), events.clone()),
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
) -> Result<String, String> {
    let repos = default_repos();

    // Ensure borg base directory exists
    tokio::fs::create_dir_all(BORG_BASE_DIR)
        .await
        .map_err(|e| format!("Failed to create borg base dir {BORG_BASE_DIR}: {e}"))?;

    let mut any_success = false;
    let mut all_success = true;
    let mut repo_messages = Vec::new();

    for (i, repo) in repos.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        {
            let mut s = status.write().await;
            s.current_message = Some(format!("Backup du repo {}", repo.name));
        }
        {
            let mut p = progress.write().await;
            p.running = true;
            p.current_repo = Some(repo.name.clone());
            p.phase = BackupPhase::Creating;
            p.progress = 0.0;
            p.nfiles = None;
            p.original_size = None;
            p.deduplicated_size = None;
            p.detail = Some(format!("Backup borg de {}", repo.name));
        }
        emit_backup_live(&events, &status, &progress).await;

        let job_id = format!("{}-{}", repo.name, Utc::now().timestamp_millis());
        let job_started = Utc::now().to_rfc3339();
        let t0 = Instant::now();

        let result = run_repo_backup(
            repo,
            status.clone(),
            progress.clone(),
            events.clone(),
        )
        .await;
        let duration = t0.elapsed().as_secs();
        let finished_at = Utc::now().to_rfc3339();

        let (success, message, archive_name, nfiles, original_size, deduplicated_size) = match result {
            Ok(stats) => {
                any_success = true;
                let msg = format!(
                    "OK: {} fichiers, {} original, {} dédupliqué",
                    stats.nfiles,
                    format_bytes(stats.original_size),
                    format_bytes(stats.deduplicated_size),
                );
                (true, msg, Some(stats.archive_name), Some(stats.nfiles), Some(stats.original_size), Some(stats.deduplicated_size))
            }
            Err(e) => {
                all_success = false;
                (false, format!("FAILED: {e}"), None, None, None, None)
            }
        };

        {
            let mut s = status.write().await;
            if let Some(repo_status) = s.repos.get_mut(&repo.name) {
                repo_status.last_backup_at = Some(finished_at.clone());
                repo_status.last_success = Some(success);
                repo_status.last_duration_secs = Some(duration);
                repo_status.last_archive_name = archive_name.clone();
                repo_status.last_nfiles = nfiles;
                repo_status.last_original_size = original_size;
                repo_status.last_deduplicated_size = deduplicated_size;
                repo_status.last_error = if success {
                    None
                } else {
                    Some(message.clone())
                };
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
                    archive_name,
                    original_size,
                    deduplicated_size,
                },
            );
            if s.jobs.len() > MAX_JOB_HISTORY {
                s.jobs.truncate(MAX_JOB_HISTORY);
            }
        }

        emit_backup_live(&events, &status, &progress).await;
        repo_messages.push(format!("{}: {}", repo.name, message));
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

// ── Borg backup stats ──────────────────────────────────────────────

struct BorgStats {
    archive_name: String,
    nfiles: u64,
    original_size: u64,
    deduplicated_size: u64,
}

// ── Per-repo backup using borg ─────────────────────────────────────

async fn run_repo_backup(
    repo: &RepoConfig,
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
) -> Result<BorgStats, String> {
    let source_path = PathBuf::from(&repo.source_path);
    if !source_path.exists() {
        info!(repo = repo.name, path = %repo.source_path, "Source directory does not exist, skipping");
        return Ok(BorgStats {
            archive_name: "skipped".to_string(),
            nfiles: 0,
            original_size: 0,
            deduplicated_size: 0,
        });
    }

    let borg_repo = format!("{}/{}/borg", BORG_BASE_DIR, repo.name);

    // Initialize borg repo if it doesn't exist
    let repo_path = PathBuf::from(&borg_repo);
    // Ensure parent directory exists (borg init won't create parents)
    if let Some(parent) = repo_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create parent dir for borg repo: {e}"))?;
    }
    if !repo_path.join("config").exists() {
        info!(repo = repo.name, path = %borg_repo, "Initializing new borg repository");
        let init_output = tokio::process::Command::new("borg")
            .args(["init", "--encryption=none", &borg_repo])
            .env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes")
            .output()
            .await
            .map_err(|e| format!("Failed to spawn borg init: {e}"))?;

        if !init_output.status.success() {
            let stderr = String::from_utf8_lossy(&init_output.stderr);
            return Err(format!("borg init failed: {stderr}"));
        }
        info!(repo = repo.name, "Borg repository initialized");
    }

    // Build borg create command
    let archive_name = format!(
        "{}::{}",
        borg_repo,
        Utc::now().format("%Y-%m-%dT%H:%M:%S")
    );

    let mut cmd = tokio::process::Command::new("borg");
    cmd.args(["create", "--progress", "--stats", "--json"]);

    // Add excludes
    for exclude in &repo.excludes {
        cmd.args(["--exclude", &format!("*/{exclude}")]);
    }

    cmd.arg(&archive_name);
    cmd.arg(&repo.source_path);

    cmd.env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes");
    // Prevent borg from asking questions
    cmd.env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes");

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    info!(repo = repo.name, archive = %archive_name, "Starting borg create");

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn borg create: {e}"))?;

    // Read stderr for progress (borg writes progress to stderr, JSON result to stdout)
    let stderr = child.stderr.take().ok_or("borg stderr unavailable")?;
    let stderr_reader = BufReader::new(stderr);
    let mut lines = stderr_reader.lines();

    let progress_clone = progress.clone();
    let status_clone = status.clone();
    let events_clone = events.clone();
    let repo_name = repo.name.clone();

    // Spawn a task to read stderr progress lines
    let progress_task = tokio::spawn(async move {
        let mut last_emit = Instant::now();
        while let Ok(Some(line)) = lines.next_line().await {
            // borg --progress writes lines like:
            //  123.45 kB O 456 N ... (progress info)
            // We just use the line as detail
            if last_emit.elapsed() >= Duration::from_millis(EMIT_THROTTLE_MS) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    let mut p = progress_clone.write().await;
                    p.detail = Some(format!("[{}] {}", repo_name, truncate_str(trimmed, 120)));
                    drop(p);
                    emit_backup_live(&events_clone, &status_clone, &progress_clone).await;
                    last_emit = Instant::now();
                }
            }
        }
    });

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("borg create failed: {e}"))?;

    // Wait for progress reader to finish
    let _ = progress_task.await;

    // borg exit codes: 0 = success, 1 = warning (but backup was created), 2 = error
    if output.status.code().unwrap_or(2) >= 2 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("borg create failed (exit {}): {}", output.status, stderr));
    }

    // Parse JSON output from stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stats = parse_borg_json_output(&stdout, &repo.name)?;

    info!(
        repo = repo.name,
        archive = %stats.archive_name,
        nfiles = stats.nfiles,
        original = stats.original_size,
        dedup = stats.deduplicated_size,
        "Borg create completed"
    );

    // Update progress
    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Pruning;
        p.detail = Some(format!("Pruning des anciennes archives de {}", repo.name));
    }
    emit_backup_live(&events, &status, &progress).await;

    // Prune old archives: keep 7 daily, 4 weekly, 6 monthly
    let prune_output = tokio::process::Command::new("borg")
        .args([
            "prune",
            "--keep-daily=7",
            "--keep-weekly=4",
            "--keep-monthly=6",
            &borg_repo,
        ])
        .env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes")
        .env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes")
        .output()
        .await
        .map_err(|e| format!("Failed to spawn borg prune: {e}"))?;

    if !prune_output.status.success() {
        let stderr = String::from_utf8_lossy(&prune_output.stderr);
        warn!(repo = repo.name, "borg prune warning: {stderr}");
        // Don't fail the whole backup for a prune issue
    } else {
        info!(repo = repo.name, "Borg prune completed");
    }

    // Compact (borg 1.2+)
    let compact_output = tokio::process::Command::new("borg")
        .args(["compact", &borg_repo])
        .env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes")
        .env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes")
        .output()
        .await;

    match compact_output {
        Ok(out) if out.status.success() => info!(repo = repo.name, "Borg compact completed"),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!(repo = repo.name, "borg compact warning: {stderr}");
        }
        Err(e) => warn!(repo = repo.name, "borg compact failed: {e}"),
    }

    {
        let mut p = progress.write().await;
        p.progress = 100.0;
        p.nfiles = Some(stats.nfiles);
        p.original_size = Some(stats.original_size);
        p.deduplicated_size = Some(stats.deduplicated_size);
        p.detail = Some(format!("Backup terminé pour {}", repo.name));
    }
    emit_backup_live(&events, &status, &progress).await;

    Ok(stats)
}

// ── Parse borg JSON output ─────────────────────────────────────────

fn parse_borg_json_output(stdout: &str, repo_name: &str) -> Result<BorgStats, String> {
    // borg create --json outputs JSON with archive and cache stats
    let json: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("Failed to parse borg JSON: {e} — output: {}", truncate_str(stdout, 500)))?;

    let archive = json
        .get("archive")
        .ok_or_else(|| "Missing 'archive' in borg output".to_string())?;

    let archive_name = archive
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let stats = archive
        .get("stats")
        .ok_or_else(|| "Missing 'archive.stats' in borg output".to_string())?;

    let nfiles = stats
        .get("nfiles")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let original_size = stats
        .get("original_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let deduplicated_size = stats
        .get("deduplicated_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    info!(
        repo = repo_name,
        archive = %archive_name,
        nfiles,
        original_size,
        deduplicated_size,
        "Parsed borg stats"
    );

    Ok(BorgStats {
        archive_name,
        nfiles,
        original_size,
        deduplicated_size,
    })
}

// ── Formatting helpers ─────────────────────────────────────────────

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

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
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
    let target_hour = 20u32; // 20:00 UTC = ~22h Brussels
    let secs_today_at_target = target_hour as i64 * 3600;
    let secs_now =
        now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;
    let secs_until = if secs_now < secs_today_at_target {
        secs_today_at_target - secs_now
    } else {
        86400 - secs_now + secs_today_at_target
    };
    secs_until.max(60) as u64
}
