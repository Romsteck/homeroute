use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const BACKUP_SERVER_HOST_ID: &str = "877bcb76-4fb8-4164-940c-707201adf9bc";
const BACKUP_SERVER_IP: &str = "10.0.0.20";
const BACKUP_SERVER_USER: &str = "romain";
const BACKUP_SSH_KEY: &str = "/root/.ssh/id_ed25519_backup";
const HOMEROUTE_API_BASE: &str = "http://10.0.0.254:4000";
const BACKUP_STAGING_ROOT: &str = "/backup/staging";
const WAKE_TIMEOUT_SECS: u64 = 180;
const WAKE_POLL_INTERVAL_SECS: u64 = 10;
const REPO_BACKUP_TIMEOUT_SECS: u64 = 3600;
const TOTAL_PIPELINE_TIMEOUT_SECS: u64 = 10800;
const MAX_JOB_HISTORY: usize = 20;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackupPhase {
    Idle,
    Rsync,
    Rustic,
    Forget,
    Sleep,
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
    pub last_transferred_bytes: Option<u64>,
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
            last_transferred_bytes: None,
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
    pub speed: Option<String>,
    pub files_processed: Option<u64>,
    pub total_files: Option<u64>,
    pub bytes_transferred: u64,
    pub elapsed_secs: u64,
    pub remaining_secs: Option<u64>,
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
            speed: None,
            files_processed: None,
            total_files: None,
            bytes_transferred: 0,
            elapsed_secs: 0,
            remaining_secs: None,
            started_at: None,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RepoRunMetrics {
    transferred_bytes: u64,
}

pub struct BackupPipeline {
    pub status: Arc<RwLock<BackupStatus>>,
    pub progress: Arc<RwLock<BackupProgress>>,
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
            progress: Arc::new(RwLock::new(BackupProgress::default())),
            http,
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
            status.stage = PipelineStage::WakingServer;
            status.current_message = Some("Waking backup server...".to_string());
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

        let status = self.status.clone();
        let progress = self.progress.clone();
        let http = self.http.clone();

        tokio::spawn(async move {
            let started = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(TOTAL_PIPELINE_TIMEOUT_SECS),
                run_pipeline(status.clone(), progress.clone(), http),
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
                        info!("Backup pipeline completed successfully in {elapsed}s: {msg}");
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

            let mut p = progress.write().await;
            p.running = false;
            p.elapsed_secs = elapsed;
            p.remaining_secs = Some(0);
            p.phase = if matches!(result, Ok(Ok(_))) {
                BackupPhase::Done
            } else {
                BackupPhase::Failed
            };
            if matches!(result, Ok(Ok(_))) {
                p.progress = 100.0;
                if p.detail.is_none() {
                    p.detail = Some("Sauvegarde terminée".to_string());
                }
            }
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

async fn run_pipeline(
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    http: reqwest::Client,
) -> Result<String, String> {
    let repos = default_repos();

    set_stage(
        &status,
        &progress,
        PipelineStage::WakingServer,
        BackupPhase::Idle,
        "Envoi du paquet WOL...",
    )
    .await;

    let wake_url = format!("{HOMEROUTE_API_BASE}/api/hosts/{BACKUP_SERVER_HOST_ID}/wake");
    match http.post(&wake_url).send().await {
        Ok(resp) => info!("WOL response: {}", resp.status()),
        Err(e) => warn!("WOL request failed (may already be online): {e}"),
    }

    set_stage(
        &status,
        &progress,
        PipelineStage::WaitingForServer,
        BackupPhase::Idle,
        "Attente du serveur de backup...",
    )
    .await;

    wait_for_server(status.clone(), progress.clone()).await?;

    set_stage(
        &status,
        &progress,
        PipelineStage::RunningBackup,
        BackupPhase::Idle,
        "Exécution des sauvegardes...",
    )
    .await;

    let mut any_success = false;
    let mut all_success = true;
    let mut repo_messages = Vec::new();

    for repo in &repos {
        {
            let mut s = status.write().await;
            s.current_message = Some(format!("Backup du repo {}", repo.name));
        }
        {
            let mut p = progress.write().await;
            p.running = true;
            p.current_repo = Some(repo.name.clone());
            p.phase = BackupPhase::Rsync;
            p.progress = 0.0;
            p.speed = None;
            p.files_processed = None;
            p.total_files = None;
            p.bytes_transferred = 0;
            p.remaining_secs = None;
            p.detail = Some(format!("Préparation du repo {}", repo.name));
        }

        let job_id = format!("{}-{}", repo.name, Utc::now().timestamp_millis());
        let job_started = Utc::now().to_rfc3339();
        let t0 = Instant::now();
        let result = run_rustic_backup(repo, status.clone(), progress.clone()).await;
        let duration = t0.elapsed().as_secs();
        let finished_at = Utc::now().to_rfc3339();

        let (success, snapshot_id, message, metrics) = match result {
            Ok((snap_id, metrics)) => {
                any_success = true;
                (
                    true,
                    snap_id.clone(),
                    format!("Backup OK (snapshot: {})", snap_id.as_deref().unwrap_or("?")),
                    metrics,
                )
            }
            Err(e) => {
                all_success = false;
                (
                    false,
                    None,
                    format!("Backup FAILED: {e}"),
                    RepoRunMetrics::default(),
                )
            }
        };

        {
            let mut s = status.write().await;
            if let Some(repo_status) = s.repos.get_mut(&repo.name) {
                repo_status.last_backup_at = Some(finished_at.clone());
                repo_status.last_success = Some(success);
                repo_status.last_duration_secs = Some(duration);
                repo_status.last_snapshot_id = snapshot_id.clone();
                repo_status.last_transferred_bytes = Some(metrics.transferred_bytes);
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
                    snapshot_id,
                },
            );
            if s.jobs.len() > MAX_JOB_HISTORY {
                s.jobs.truncate(MAX_JOB_HISTORY);
            }
        }

        repo_messages.push(format!("{}: {}", repo.name, message));
    }

    set_stage(
        &status,
        &progress,
        PipelineStage::Verifying,
        BackupPhase::Forget,
        "Vérification / rétention...",
    )
    .await;

    set_stage(
        &status,
        &progress,
        PipelineStage::PuttingToSleep,
        BackupPhase::Sleep,
        "Mise en veille du serveur backup...",
    )
    .await;

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

async fn run_rustic_backup(
    repo: &RepoConfig,
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
) -> Result<(Option<String>, RepoRunMetrics), String> {
    let rustic_password = std::env::var("RUSTIC_PASSWORD").unwrap_or_else(|_| {
        warn!("RUSTIC_PASSWORD not set, using default password. Please set it in production!");
        "changeme".to_string()
    });

    ensure_remote_dir(&repo.staging_root, 60).await?;

    let mut staged_paths = Vec::new();
    let mut metrics = RepoRunMetrics::default();
    for source_path in &repo.source_paths {
        sync_source_to_backup(
            repo,
            source_path,
            REPO_BACKUP_TIMEOUT_SECS,
            status.clone(),
            progress.clone(),
        )
        .await?;
        staged_paths.push(remote_stage_path(&repo.staging_root, source_path));
    }
    metrics.transferred_bytes = progress.read().await.bytes_transferred;

    let init_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} init 2>&1 || true",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
    );
    let _ = run_ssh_command(&init_cmd, 60).await;

    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Rustic;
        p.progress = 0.0;
        p.speed = None;
        p.files_processed = None;
        p.total_files = None;
        p.remaining_secs = None;
        p.detail = Some(format!("Création du snapshot rustic pour {}", repo.name));
    }
    {
        let mut s = status.write().await;
        s.current_message = Some(format!("rustic backup: {}", repo.name));
    }

    let backup_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} backup {paths} --json --verbose 2>&1",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
        paths = staged_paths
            .iter()
            .map(|path| shell_escape(path))
            .collect::<Vec<_>>()
            .join(" "),
    );

    let output = run_ssh_command_streaming(
        &backup_cmd,
        REPO_BACKUP_TIMEOUT_SECS,
        Some(progress.clone()),
        Some(status.clone()),
        OutputKind::Rustic,
    )
    .await
    .map_err(|e| format!("SSH backup failed: {e}"))?;

    let snapshot_id = parse_snapshot_id(&output);

    {
        let mut p = progress.write().await;
        if p.progress < 100.0 {
            p.progress = 100.0;
        }
        p.detail = Some(format!("Snapshot rustic terminé pour {}", repo.name));
    }

    let forget_cmd = format!(
        "sudo env RUSTIC_PASSWORD={pw} rustic -r {repo} forget --keep-daily 7 --keep-weekly 4 --keep-monthly 6 --prune 2>&1",
        pw = shell_escape(&rustic_password),
        repo = shell_escape(&repo.rustic_repo),
    );

    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Forget;
        p.detail = Some(format!("Application de la rétention sur {}", repo.name));
    }

    match run_ssh_command(&forget_cmd, REPO_BACKUP_TIMEOUT_SECS).await {
        Ok(out) => info!("Forget/prune for {}: {}", repo.name, out.trim()),
        Err(e) => warn!("Forget/prune for {} failed: {e}", repo.name),
    }

    Ok((snapshot_id, metrics))
}

async fn sync_source_to_backup(
    repo: &RepoConfig,
    source_path: &str,
    timeout_secs: u64,
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
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

    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Rsync;
        p.progress = 0.0;
        p.speed = None;
        p.files_processed = None;
        p.total_files = None;
        p.bytes_transferred = 0;
        p.detail = Some(format!("rsync {} → {}", source_path, remote_target));
    }
    {
        let mut s = status.write().await;
        s.current_message = Some(format!("rsync: {}", source_path));
    }

    let mut args = vec![
        "-aHAX".to_string(),
        "--delete".to_string(),
        "--numeric-ids".to_string(),
        "--info=progress2".to_string(),
        "--human-readable".to_string(),
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

    let output = run_local_command_streaming(
        "rsync",
        &args,
        timeout_secs,
        Some(progress.clone()),
        Some(status.clone()),
        OutputKind::Rsync,
    )
    .await?;

    if !output.trim().is_empty() {
        info!("rsync {} -> {}: {}", source_path, remote_target, output.trim());
    }

    {
        let mut p = progress.write().await;
        p.progress = 100.0;
        p.detail = Some(format!("rsync terminé pour {}", source_path));
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

#[derive(Clone, Copy)]
enum OutputKind {
    Rsync,
    Rustic,
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

async fn run_local_command_streaming(
    binary: &str,
    args: &[String],
    timeout_secs: u64,
    progress: Option<Arc<RwLock<BackupProgress>>>,
    status: Option<Arc<RwLock<BackupStatus>>>,
    kind: OutputKind,
) -> Result<String, String> {
    let mut child = tokio::process::Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {binary}: {e}"))?;

    let stdout = child.stdout.take().ok_or_else(|| format!("{binary}: stdout unavailable"))?;
    let stderr = child.stderr.take().ok_or_else(|| format!("{binary}: stderr unavailable"))?;

    let out_task = tokio::spawn(read_stream(stdout, progress.clone(), status.clone(), kind));
    let err_task = tokio::spawn(read_stream(stderr, progress.clone(), status.clone(), kind));

    let exit = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait())
        .await
        .map_err(|_| format!("{binary} timed out after {timeout_secs}s"))?
        .map_err(|e| format!("Failed waiting for {binary}: {e}"))?;

    let mut combined = String::new();
    combined.push_str(&out_task.await.map_err(|e| format!("stdout task failed: {e}"))?);
    combined.push_str(&err_task.await.map_err(|e| format!("stderr task failed: {e}"))?);

    if exit.success() {
        Ok(combined.trim().to_string())
    } else {
        Err(format!(
            "{binary} exited with code {}. Output: {}",
            exit.code().unwrap_or(-1),
            combined.trim()
        ))
    }
}

async fn run_ssh_command(cmd: &str, timeout_secs: u64) -> Result<String, String> {
    run_local_command("ssh", &ssh_args(cmd), timeout_secs).await
}

async fn run_ssh_command_streaming(
    cmd: &str,
    timeout_secs: u64,
    progress: Option<Arc<RwLock<BackupProgress>>>,
    status: Option<Arc<RwLock<BackupStatus>>>,
    kind: OutputKind,
) -> Result<String, String> {
    run_local_command_streaming("ssh", &ssh_args(cmd), timeout_secs, progress, status, kind).await
}

fn ssh_args(cmd: &str) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "ConnectTimeout=15".into(),
        "-i".into(),
        BACKUP_SSH_KEY.into(),
        format!("{BACKUP_SERVER_USER}@{BACKUP_SERVER_IP}"),
        cmd.to_string(),
    ]
}

async fn read_stream<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    progress: Option<Arc<RwLock<BackupProgress>>>,
    status: Option<Arc<RwLock<BackupStatus>>>,
    kind: OutputKind,
) -> String {
    let mut lines = BufReader::new(reader).lines();
    let mut combined = String::new();

    while let Ok(Some(line)) = lines.next_line().await {
        let normalized = line.replace('\r', "\n");
        for chunk in normalized.lines() {
            let text = chunk.trim();
            if text.is_empty() {
                continue;
            }
            combined.push_str(text);
            combined.push('\n');
            match kind {
                OutputKind::Rsync => parse_rsync_progress(text, progress.clone(), status.clone()).await,
                OutputKind::Rustic => parse_rustic_progress(text, progress.clone(), status.clone()).await,
            }
        }
    }

    combined
}

async fn parse_rsync_progress(
    line: &str,
    progress: Option<Arc<RwLock<BackupProgress>>>,
    status: Option<Arc<RwLock<BackupStatus>>>,
) {
    let Some(progress) = progress else { return; };
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    let percent = trimmed
        .split_whitespace()
        .find_map(|token| token.strip_suffix('%'))
        .and_then(|v| v.replace(',', ".").parse::<f64>().ok());

    let speed = trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|w| {
            let unit = w[1].to_ascii_lowercase();
            if unit.contains("/s") {
                Some(format!("{} {}", w[0], w[1]))
            } else {
                None
            }
        });

    let bytes = trimmed
        .split_whitespace()
        .next()
        .map(|s| s.replace(',', ""))
        .and_then(|s| s.parse::<u64>().ok());

    let to_chk = trimmed
        .split("to-chk=")
        .nth(1)
        .and_then(|rest| rest.split(|c| c == ')' || c == ' ').next())
        .and_then(|pair| {
            let mut it = pair.split('/');
            let left = it.next()?.parse::<u64>().ok()?;
            let right = it.next()?.parse::<u64>().ok()?;
            Some((left, right))
        });

    let elapsed = trimmed
        .split_whitespace()
        .find(|t| t.matches(':').count() >= 2)
        .and_then(parse_hms);

    let mut p = progress.write().await;
    if let Some(v) = percent {
        p.progress = v.clamp(0.0, 100.0);
    }
    if let Some(v) = speed {
        p.speed = Some(v);
    }
    if let Some(v) = bytes {
        p.bytes_transferred = v;
    }
    if let Some((remaining, total)) = to_chk {
        p.total_files = Some(total);
        p.files_processed = Some(total.saturating_sub(remaining));
    }
    if let Some(elapsed_secs) = elapsed {
        p.elapsed_secs = elapsed_secs;
        if let Some(pct) = percent.filter(|v| *v > 0.0) {
            let total_est = (elapsed_secs as f64 / (pct / 100.0)) as u64;
            p.remaining_secs = Some(total_est.saturating_sub(elapsed_secs));
        }
    }
    p.detail = Some(trimmed.to_string());
    drop(p);

    if let Some(status) = status {
        status.write().await.current_message = Some(trimmed.to_string());
    }
}

async fn parse_rustic_progress(
    line: &str,
    progress: Option<Arc<RwLock<BackupProgress>>>,
    status: Option<Arc<RwLock<BackupStatus>>>,
) {
    let Some(progress) = progress else { return; };
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    let mut pct = None;
    let mut bytes = None;
    let mut files = None;
    let mut total_files = None;
    let mut speed = None;

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        pct = find_first_f64(&value, &["percent_done", "percent", "progress", "progress_percent"]);
        bytes = find_first_u64(&value, &["bytes_done", "bytes", "processed_bytes", "total_bytes_processed", "data_bytes", "size"]);
        files = find_first_u64(&value, &["files_done", "files", "processed_files", "current_files"]);
        total_files = find_first_u64(&value, &["total_files", "files_total"]);
        speed = find_first_stringish(&value, &["speed", "throughput"]);
    } else {
        pct = trimmed
            .split_whitespace()
            .find_map(|token| token.strip_suffix('%'))
            .and_then(|v| v.replace(',', ".").parse::<f64>().ok());
    }

    let mut p = progress.write().await;
    if let Some(v) = pct {
        p.progress = v.clamp(0.0, 100.0);
    }
    if let Some(v) = bytes {
        p.bytes_transferred = p.bytes_transferred.max(v);
    }
    if let Some(v) = files {
        p.files_processed = Some(v);
    }
    if let Some(v) = total_files {
        p.total_files = Some(v);
    }
    if let Some(v) = speed {
        p.speed = Some(v);
    }
    p.detail = Some(trimmed.to_string());
    drop(p);

    if let Some(status) = status {
        status.write().await.current_message = Some(trimmed.to_string());
    }
}

fn parse_hms(value: &str) -> Option<u64> {
    let parts: Vec<_> = value.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h = parts[0].parse::<u64>().ok()?;
    let m = parts[1].parse::<u64>().ok()?;
    let s = parts[2].parse::<u64>().ok()?;
    Some(h * 3600 + m * 60 + s)
}

fn find_first_f64(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(v) = map.get(*key) {
                    if let Some(n) = v
                        .as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
                    {
                        return Some(n);
                    }
                }
            }
            map.values().find_map(|v| find_first_f64(v, keys))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_first_f64(v, keys)),
        _ => None,
    }
}

fn find_first_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(v) = map.get(*key) {
                    if let Some(n) = v
                        .as_u64()
                        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
                        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    {
                        return Some(n);
                    }
                }
            }
            map.values().find_map(|v| find_first_u64(v, keys))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_first_u64(v, keys)),
        _ => None,
    }
}

fn find_first_stringish(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(v) = map.get(*key) {
                    if let Some(s) = v.as_str() {
                        return Some(s.to_string());
                    }
                    if let Some(n) = v.as_f64() {
                        return Some(format!("{n:.1} B/s"));
                    }
                }
            }
            map.values().find_map(|v| find_first_stringish(v, keys))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_first_stringish(v, keys)),
        _ => None,
    }
}

async fn wait_for_server(
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(WAKE_TIMEOUT_SECS);
    let mut attempt = 0u32;

    while Instant::now() < deadline {
        attempt += 1;
        let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
        let message = format!(
            "Attente du serveur backup… tentative {attempt} ({remaining}s restantes)"
        );
        {
            status.write().await.current_message = Some(message.clone());
            let mut p = progress.write().await;
            p.detail = Some(message);
            p.elapsed_secs = WAKE_TIMEOUT_SECS.saturating_sub(remaining);
            p.remaining_secs = Some(remaining);
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
    progress: &Arc<RwLock<BackupProgress>>,
    stage: PipelineStage,
    phase: BackupPhase,
    message: &str,
) {
    {
        let mut s = status.write().await;
        s.stage = stage;
        s.current_message = Some(message.to_string());
    }
    {
        let mut p = progress.write().await;
        p.phase = phase;
        p.detail = Some(message.to_string());
    }
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
