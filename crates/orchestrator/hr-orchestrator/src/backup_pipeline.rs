use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use hr_common::events::{BackupLiveEvent, EventBus};
use hr_registry::protocol::HostRegistryMessage;
use hr_registry::{AgentRegistry, BackupSignal};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const BACKUP_SERVER_HOST_ID: &str = "877bcb76-4fb8-4164-940c-707201adf9bc";
const REPO_BACKUP_TIMEOUT_SECS: u64 = 7200;
const TOTAL_PIPELINE_TIMEOUT_SECS: u64 = 14400;
const MAX_JOB_HISTORY: usize = 20;
const CHUNK_SIZE: usize = 524288; // 512KB
const EMIT_THROTTLE_MS: u64 = 100;

// ── Manifest types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileManifest {
    pub created_at: String,
    pub repo_name: String,
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    #[serde(default)]
    pub hash: Option<u32>, // xxhash32, computed when mtime is ambiguous
}

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
    CheckingServer,
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
    pub last_transferred_bytes: Option<u64>,
    pub last_files_changed: Option<u64>,
    pub last_files_total: Option<u64>,
}

impl RepoStatus {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            last_backup_at: None,
            last_success: None,
            last_duration_secs: None,
            last_error: None,
            last_transferred_bytes: None,
            last_files_changed: None,
            last_files_total: None,
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
    pub files_changed: Option<u64>,
    pub bytes_transferred: Option<u64>,
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
    pub files_changed: Option<u64>,
    pub files_total: Option<u64>,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
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
            files_changed: None,
            files_total: None,
            bytes_transferred: 0,
            total_bytes: 0,
            elapsed_secs: 0,
            remaining_secs: None,
            started_at: None,
            detail: None,
        }
    }
}

// ── BackupPipeline ─────────────────────────────────────────────────

pub struct BackupPipeline {
    pub status: Arc<RwLock<BackupStatus>>,
    pub progress: Arc<RwLock<BackupProgress>>,
    http: reqwest::Client,
    events: Arc<EventBus>,
    registry: Arc<AgentRegistry>,
}

impl BackupPipeline {
    pub fn new(events: Arc<EventBus>, registry: Arc<AgentRegistry>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            status: Arc::new(RwLock::new(BackupStatus::default())),
            progress: Arc::new(RwLock::new(BackupProgress::default())),
            http,
            events,
            registry,
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
            status.stage = PipelineStage::CheckingServer;
            status.current_message = Some("Checking backup server connection...".to_string());
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
        let http = self.http.clone();
        let events = self.events.clone();
        let registry = self.registry.clone();

        tokio::spawn(async move {
            let started = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(TOTAL_PIPELINE_TIMEOUT_SECS),
                run_pipeline(
                    status.clone(),
                    progress.clone(),
                    http,
                    events.clone(),
                    registry,
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
                p.remaining_secs = Some(0);
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

async fn set_stage(
    status: &Arc<RwLock<BackupStatus>>,
    progress: &Arc<RwLock<BackupProgress>>,
    stage: PipelineStage,
    phase: BackupPhase,
    message: &str,
    events: &EventBus,
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
    emit_backup_live(events, status, progress).await;
}

// ── Pipeline ───────────────────────────────────────────────────────

async fn run_pipeline(
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    _http: reqwest::Client,
    events: Arc<EventBus>,
    registry: Arc<AgentRegistry>,
) -> Result<String, String> {
    let repos = default_repos();

    // NOTE: WOL désactivé post-migration. Le backup server = machine locale.
    // On vérifie juste que le host-agent est connecté.
    set_stage(
        &status,
        &progress,
        PipelineStage::CheckingServer,
        BackupPhase::Idle,
        "Vérification de la connexion du serveur de backup...",
        &events,
    )
    .await;

    if !registry.is_host_connected(BACKUP_SERVER_HOST_ID).await {
        let msg = "Host agent not connected for backup. Aborting.".to_string();
        error!("{msg}");
        let mut s = status.write().await;
        s.stage = PipelineStage::Failed;
        s.current_message = Some(msg.clone());
        drop(s);
        return Err(msg);
    }
    info!("Backup server host-agent connected (local mode)");

    // ── Run backups ─────────────────────────────────────────────
    set_stage(
        &status,
        &progress,
        PipelineStage::RunningBackup,
        BackupPhase::Idle,
        "Exécution des sauvegardes...",
        &events,
    )
    .await;

    let mut any_success = false;
    let mut all_success = true;
    let mut repo_messages = Vec::new();

    for (i, repo) in repos.iter().enumerate() {
        // Small delay between repos to let WebSocket settle after finalization I/O
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        // Early abort: if host disconnected, skip remaining repos
        if !registry.is_host_connected(BACKUP_SERVER_HOST_ID).await {
            warn!("Backup server disconnected, aborting remaining repos");
            let msg = format!("Backup FAILED: host disconnected before {}", repo.name);
            repo_messages.push(format!("{}: {}", repo.name, msg));
            all_success = false;
            continue;
        }

        {
            let mut s = status.write().await;
            s.current_message = Some(format!("Backup du repo {}", repo.name));
        }
        {
            let mut p = progress.write().await;
            p.running = true;
            p.current_repo = Some(repo.name.clone());
            p.phase = BackupPhase::Scanning;
            p.progress = 0.0;
            p.speed = None;
            p.files_changed = None;
            p.files_total = None;
            p.bytes_transferred = 0;
            p.total_bytes = 0;
            p.remaining_secs = None;
            p.detail = Some(format!("Scan du repo {}", repo.name));
        }

        emit_backup_live(&events, &status, &progress).await;

        let job_id = format!("{}-{}", repo.name, Utc::now().timestamp_millis());
        let job_started = Utc::now().to_rfc3339();
        let t0 = Instant::now();
        let result = run_repo_backup(
            repo,
            &registry,
            status.clone(),
            progress.clone(),
            events.clone(),
        )
        .await;
        let duration = t0.elapsed().as_secs();
        let finished_at = Utc::now().to_rfc3339();

        let (success, message, files_changed, bytes_transferred) = match result {
            Ok((fc, bt)) => {
                any_success = true;
                (
                    true,
                    format!("Backup OK ({fc} fichiers modifiés, {bt} octets transférés)"),
                    Some(fc),
                    Some(bt),
                )
            }
            Err(e) => {
                all_success = false;
                (false, format!("Backup FAILED: {e}"), None, None)
            }
        };

        {
            let mut s = status.write().await;
            if let Some(repo_status) = s.repos.get_mut(&repo.name) {
                repo_status.last_backup_at = Some(finished_at.clone());
                repo_status.last_success = Some(success);
                repo_status.last_duration_secs = Some(duration);
                repo_status.last_transferred_bytes = bytes_transferred;
                repo_status.last_files_changed = files_changed;
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
                    files_changed,
                    bytes_transferred,
                },
            );
            if s.jobs.len() > MAX_JOB_HISTORY {
                s.jobs.truncate(MAX_JOB_HISTORY);
            }
        }

        emit_backup_live(&events, &status, &progress).await;
        repo_messages.push(format!("{}: {}", repo.name, message));
    }

    // NOTE: PowerOff du backup server désactivé post-migration.
    // Le backup server est maintenant la même machine que le routeur.
    // Voir audit backup-audit-001 du 2026-03-16.
    if all_success {
        info!("Backup pipeline complete. Skipping backup server shutdown (local backup mode).");
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

// ── Per-repo backup ────────────────────────────────────────────────

async fn run_repo_backup(
    repo: &RepoConfig,
    registry: &Arc<AgentRegistry>,
    status: Arc<RwLock<BackupStatus>>,
    progress: Arc<RwLock<BackupProgress>>,
    events: Arc<EventBus>,
) -> Result<(u64, u64), String> {
    let transfer_id = uuid::Uuid::new_v4().to_string();

    // Register backup signal to receive responses
    let mut signal_rx = registry.register_backup_signal(&transfer_id).await;

    // Verify host is still connected
    if !registry.is_host_connected(BACKUP_SERVER_HOST_ID).await {
        return Err("Backup server disconnected before repo backup".to_string());
    }

    // Skip repos with empty/missing source directory
    let source_path = PathBuf::from(&repo.source_path);
    if !source_path.exists() {
        info!(repo = repo.name, path = %repo.source_path, "Source directory does not exist, skipping");
        registry.remove_backup_signal(&transfer_id).await;
        return Ok((0, 0));
    }

    // Step 1: Tell host-agent to prepare and send us the previous manifest
    info!(repo = repo.name, "Sending StartBackupRepo");
    registry
        .send_host_command(
            BACKUP_SERVER_HOST_ID,
            HostRegistryMessage::StartBackupRepo {
                repo_name: repo.name.clone(),
                transfer_id: transfer_id.clone(),
            },
        )
        .await
        .map_err(|e| format!("Failed to send StartBackupRepo: {e}"))?;

    // Wait for BackupRepoReady
    tokio::time::timeout(Duration::from_secs(60), signal_rx.recv())
        .await
        .map_err(|_| "Timeout waiting for BackupRepoReady".to_string())?
        .ok_or_else(|| "Backup signal channel closed".to_string())
        .and_then(|sig| match sig {
            BackupSignal::RepoReady { .. } => Ok(()),
            _ => Err("Unexpected backup signal".to_string()),
        })?;

    // Load previous manifest from local cache (stored after each successful backup)
    let manifest_dir = PathBuf::from("/var/lib/server-dashboard/backup-manifests");
    let manifest_path = manifest_dir.join(format!("{}.json", repo.name));
    let old_manifest: Option<FileManifest> = tokio::fs::read_to_string(&manifest_path)
        .await
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok());

    info!(
        repo = repo.name,
        has_previous = old_manifest.is_some(),
        "Received previous manifest"
    );

    // Step 2: Scan local directory
    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Scanning;
        p.detail = Some(format!("Scan des fichiers de {}", repo.name));
    }
    emit_backup_live(&events, &status, &progress).await;

    let source_path = PathBuf::from(&repo.source_path);
    let current_files = scan_directory(&source_path, &repo.excludes).await?;
    let total_files = current_files.len() as u64;

    // Step 3: Compute diff
    let old_index: HashMap<String, &ManifestEntry> = old_manifest
        .as_ref()
        .map(|m| m.files.iter().map(|e| (e.path.clone(), e)).collect())
        .unwrap_or_default();

    let mut changed_files: Vec<String> = Vec::new();
    for entry in &current_files {
        match old_index.get(&entry.path) {
            Some(old) if old.size == entry.size && old.mtime == entry.mtime => {
                // Unchanged
            }
            _ => {
                changed_files.push(entry.path.clone());
            }
        }
    }

    let files_changed = changed_files.len() as u64;
    info!(
        repo = repo.name,
        total = total_files,
        changed = files_changed,
        "Diff computed"
    );

    {
        let mut p = progress.write().await;
        p.files_changed = Some(files_changed);
        p.files_total = Some(total_files);
        p.detail = Some(format!(
            "{files_changed} fichiers modifiés sur {total_files} ({})",
            repo.name
        ));
    }
    emit_backup_live(&events, &status, &progress).await;

    // Step 4: Transfer changed files via binary pipe
    let bytes_transferred = if changed_files.is_empty() {
        info!(repo = repo.name, "No changes to transfer");
        {
            let mut p = progress.write().await;
            p.phase = BackupPhase::Transferring;
            p.progress = 100.0;
            p.detail = Some(format!("Aucun changement pour {}", repo.name));
        }
        emit_backup_live(&events, &status, &progress).await;

        // Send TransferComplete even if no data (to close tar on agent side)
        info!(repo = repo.name, "Sending TransferComplete (no data)");
        registry
            .send_host_command(
                BACKUP_SERVER_HOST_ID,
                HostRegistryMessage::TransferComplete {
                    transfer_id: transfer_id.clone(),
                },
            )
            .await
            .map_err(|e| format!("Failed to send TransferComplete: {e}"))?;
        info!(repo = repo.name, "TransferComplete sent OK");

        0u64
    } else {
        {
            let mut p = progress.write().await;
            p.phase = BackupPhase::Transferring;
            p.progress = 0.0;
            p.detail = Some(format!(
                "Transfert de {} fichiers pour {}",
                files_changed, repo.name
            ));
        }
        emit_backup_live(&events, &status, &progress).await;

        // Write file list to temp file for tar
        let file_list_path = format!("/tmp/backup-filelist-{}.txt", transfer_id);
        let file_list_content = changed_files.join("\n");
        tokio::fs::write(&file_list_path, &file_list_content)
            .await
            .map_err(|e| format!("Failed to write file list: {e}"))?;

        // Spawn tar process to create archive of changed files
        let mut tar_child = tokio::process::Command::new("tar")
            .args([
                "cf",
                "-",
                "--numeric-owner",
                "-C",
                &repo.source_path,
                "-T",
                &file_list_path,
                "--ignore-failed-read",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn tar: {e}"))?;

        let mut tar_stdout = tar_child.stdout.take().ok_or("tar stdout unavailable")?;

        // Estimate total bytes from changed files
        let estimated_bytes: u64 = current_files
            .iter()
            .filter(|e| changed_files.contains(&e.path))
            .map(|e| e.size)
            .sum();

        {
            let mut p = progress.write().await;
            p.total_bytes = estimated_bytes;
        }

        // Stream chunks to remote host-agent
        let bytes = stream_backup_to_remote(
            registry,
            &transfer_id,
            &mut tar_stdout,
            estimated_bytes,
            &status,
            &progress,
            &events,
            &repo.name,
        )
        .await?;

        // Wait for tar to finish
        let _ = tar_child.wait().await;
        // Clean up file list
        let _ = tokio::fs::remove_file(&file_list_path).await;

        // Send TransferComplete
        registry
            .send_host_command(
                BACKUP_SERVER_HOST_ID,
                HostRegistryMessage::TransferComplete {
                    transfer_id: transfer_id.clone(),
                },
            )
            .await
            .map_err(|e| format!("Failed to send TransferComplete: {e}"))?;

        bytes
    };

    // Step 5: Send new manifest and ask host-agent to finalize
    {
        let mut p = progress.write().await;
        p.phase = BackupPhase::Verifying;
        p.detail = Some(format!("Finalisation du backup {}", repo.name));
    }
    emit_backup_live(&events, &status, &progress).await;

    let new_manifest = FileManifest {
        created_at: Utc::now().to_rfc3339(),
        repo_name: repo.name.clone(),
        files: current_files,
    };
    let manifest_bytes =
        serde_json::to_vec(&new_manifest).map_err(|e| format!("Manifest serialize: {e}"))?;

    // Stream manifest as binary chunks (can be very large for repos with many files)
    info!(repo = repo.name, size = manifest_bytes.len(), "Sending BackupManifestStart");
    registry
        .send_host_command(
            BACKUP_SERVER_HOST_ID,
            HostRegistryMessage::BackupManifestStart {
                repo_name: repo.name.clone(),
                transfer_id: transfer_id.clone(),
                manifest_size: manifest_bytes.len() as u64,
            },
        )
        .await
        .map_err(|e| format!("Failed to send BackupManifestStart: {e}"))?;

    // Send manifest in chunks
    let chunk_count = (manifest_bytes.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
    info!(repo = repo.name, chunks = chunk_count, "Sending manifest chunks");
    for chunk in manifest_bytes.chunks(CHUNK_SIZE) {
        let checksum = xxhash_rust::xxh32::xxh32(chunk, 0);
        registry
            .send_host_command(
                BACKUP_SERVER_HOST_ID,
                HostRegistryMessage::ReceiveChunkBinary {
                    transfer_id: transfer_id.clone(),
                    sequence: 0, // not used for manifest
                    size: chunk.len() as u32,
                    checksum,
                },
            )
            .await
            .map_err(|e| format!("Failed to send manifest chunk metadata: {e}"))?;
        registry
            .send_host_binary(BACKUP_SERVER_HOST_ID, chunk.to_vec())
            .await
            .map_err(|e| format!("Failed to send manifest chunk: {e}"))?;
    }

    // Signal finalization
    info!(repo = repo.name, "Sending FinishBackupRepo");
    registry
        .send_host_command(
            BACKUP_SERVER_HOST_ID,
            HostRegistryMessage::FinishBackupRepo {
                repo_name: repo.name.clone(),
                transfer_id: transfer_id.clone(),
            },
        )
        .await
        .map_err(|e| format!("Failed to send FinishBackupRepo: {e}"))?;
    info!(repo = repo.name, "FinishBackupRepo sent, waiting for BackupRepoComplete...");

    // Wait for BackupRepoComplete (5 min max for finalization)
    let complete_result = tokio::time::timeout(
        Duration::from_secs(300),
        signal_rx.recv(),
    )
    .await
    .map_err(|_| "Timeout waiting for BackupRepoComplete".to_string())?
    .ok_or_else(|| "Backup signal channel closed".to_string())
    .and_then(|sig| match sig {
        BackupSignal::RepoComplete {
            success,
            message,
            snapshot_name,
        } => {
            if success {
                info!(
                    repo = repo.name,
                    snapshot = ?snapshot_name,
                    "Backup repo complete"
                );
                Ok(())
            } else {
                Err(format!("Host-agent finalization failed: {message}"))
            }
        }
        _ => Err("Unexpected backup signal".to_string()),
    });

    // Clean up signal
    registry.remove_backup_signal(&transfer_id).await;
    complete_result?;

    // Save manifest locally for future differential comparisons
    let manifest_dir = PathBuf::from("/var/lib/server-dashboard/backup-manifests");
    let _ = tokio::fs::create_dir_all(&manifest_dir).await;
    let manifest_path = manifest_dir.join(format!("{}.json", repo.name));
    if let Ok(json) = serde_json::to_string(&new_manifest) {
        if let Err(e) = tokio::fs::write(&manifest_path, &json).await {
            warn!(repo = repo.name, "Failed to save local manifest: {e}");
        }
    }

    {
        let mut p = progress.write().await;
        p.progress = 100.0;
        p.detail = Some(format!("Backup terminé pour {}", repo.name));
    }
    emit_backup_live(&events, &status, &progress).await;

    Ok((files_changed, bytes_transferred))
}

// ── Directory scanner ──────────────────────────────────────────────

async fn scan_directory(
    source_path: &Path,
    excludes: &[String],
) -> Result<Vec<ManifestEntry>, String> {
    let source = source_path.to_path_buf();
    let excludes = excludes.to_vec();

    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        scan_dir_recursive(&source, &source, &excludes, &mut entries)?;
        Ok(entries)
    })
    .await
    .map_err(|e| format!("Scan task failed: {e}"))?
}

fn scan_dir_recursive(
    base: &Path,
    dir: &Path,
    excludes: &[String],
    entries: &mut Vec<ManifestEntry>,
) -> Result<(), String> {
    let read_dir = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let rel_path = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        // Check excludes
        if excludes.iter().any(|ex| {
            rel_path == *ex
                || rel_path.starts_with(&format!("{ex}/"))
                || path.file_name().map_or(false, |n| n.to_string_lossy() == *ex)
        }) {
            continue;
        }

        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if ft.is_dir() {
            scan_dir_recursive(base, &path, excludes, entries)?;
        } else if ft.is_file() || ft.is_symlink() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            entries.push(ManifestEntry {
                path: rel_path,
                size: meta.len(),
                mtime,
                hash: None,
            });
        }
    }
    Ok(())
}

// ── Binary streaming to remote ─────────────────────────────────────

async fn stream_backup_to_remote(
    registry: &Arc<AgentRegistry>,
    transfer_id: &str,
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    estimated_bytes: u64,
    status: &Arc<RwLock<BackupStatus>>,
    progress: &Arc<RwLock<BackupProgress>>,
    events: &Arc<EventBus>,
    repo_name: &str,
) -> Result<u64, String> {
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut transferred: u64 = 0;
    let mut sequence: u32 = 0;
    let mut last_emit = Instant::now();
    let transfer_start = Instant::now();

    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("Read error: {e}")),
        };

        let chunk = &buf[..n];
        let checksum = xxhash_rust::xxh32::xxh32(chunk, 0);

        if let Err(e) = registry
            .send_host_command(
                BACKUP_SERVER_HOST_ID,
                HostRegistryMessage::ReceiveChunkBinary {
                    transfer_id: transfer_id.to_string(),
                    sequence,
                    size: n as u32,
                    checksum,
                },
            )
            .await
        {
            return Err(format!("Send chunk metadata failed: {e}"));
        }

        if let Err(e) = registry
            .send_host_binary(BACKUP_SERVER_HOST_ID, chunk.to_vec())
            .await
        {
            return Err(format!("Send binary chunk failed: {e}"));
        }

        transferred += n as u64;
        sequence += 1;

        // Update progress with throttle
        if last_emit.elapsed() >= Duration::from_millis(EMIT_THROTTLE_MS) || transferred >= estimated_bytes {
            let pct = if estimated_bytes > 0 {
                (transferred as f64 / estimated_bytes as f64 * 100.0).min(99.9)
            } else {
                0.0
            };
            let elapsed_secs = transfer_start.elapsed().as_secs_f64();
            let speed = if elapsed_secs > 0.0 {
                transferred as f64 / elapsed_secs
            } else {
                0.0
            };
            let speed_str = format_speed(speed);
            let remaining = if speed > 0.0 && estimated_bytes > transferred {
                Some(((estimated_bytes - transferred) as f64 / speed) as u64)
            } else {
                None
            };

            {
                let mut p = progress.write().await;
                p.progress = pct;
                p.bytes_transferred = transferred;
                p.speed = Some(speed_str);
                p.remaining_secs = remaining;
                p.detail = Some(format!(
                    "Transfert {} : {} / {} ({}%)",
                    repo_name,
                    format_bytes(transferred),
                    format_bytes(estimated_bytes),
                    pct as u32
                ));
            }

            emit_backup_live(events, status, progress).await;
            last_emit = Instant::now();
        }
    }

    info!(
        repo = repo_name,
        bytes = transferred,
        chunks = sequence,
        "Transfer complete"
    );
    Ok(transferred)
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

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0} B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
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

fn next_scheduled_run_secs() -> u64 {
    use chrono::{Timelike, Utc};
    let now = Utc::now();
    let target_hour = 3u32;
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
