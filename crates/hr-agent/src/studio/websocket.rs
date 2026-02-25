use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, error, warn};

use super::types::{FileEntry, ImageAttachment, WsInMessage, WsOutMessage};
use super::StudioBridge;

const WORKSPACE_ROOT: &str = "/root/workspace";
const STUDIO_USER: &str = "studio";
const STUDIO_HOME: &str = "/home/studio";
const MAX_FILE_PREVIEW_BYTES: u64 = 512_000;
const MAX_DIR_ENTRIES: usize = 1000;

/// Run credential sync + chmod only once per process lifetime to avoid
/// triggering Vite's file watcher (inotify IN_ATTRIB) on every prompt.
static CREDENTIALS_SYNCED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Sync Claude credentials from root to studio user's home.
/// Studio user is pre-created during container provisioning.
async fn sync_studio_credentials() {
    let studio_claude = format!("{STUDIO_HOME}/.claude");
    let _ = tokio::fs::create_dir_all(&studio_claude).await;

    // Ensure studio user owns its home
    let _ = tokio::process::Command::new("chown")
        .args(["-R", &format!("{STUDIO_USER}:{STUDIO_USER}"), STUDIO_HOME])
        .output()
        .await;

    // Ensure workspace files are readable/writable by studio user
    // (workspace may contain files owned by different UIDs from nspawn mapping)
    let _ = tokio::process::Command::new("chmod")
        .args(["-R", "a+rwX", WORKSPACE_ROOT])
        .output()
        .await;
}

/// Resolve a client-provided relative path to an absolute path within workspace.
/// Returns None if the path escapes the sandbox.
fn resolve_safe_path(relative: &str) -> Option<PathBuf> {
    let root = Path::new(WORKSPACE_ROOT);
    let candidate = root.join(relative);
    match candidate.canonicalize() {
        Ok(resolved) => {
            if resolved.starts_with(root) {
                Some(resolved)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// Main WebSocket session loop.
/// Called from StudioBridge::start_ws_server() after tokio-tungstenite accept_hdr_async().
pub async fn run_ws_session<S>(ws: WebSocketStream<S>, studio: &Arc<StudioBridge>, conn_id: &str, user: Option<String>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (ws_tx, mut ws_rx) = ws.split();

    // Channel for outgoing messages
    let (out_tx, mut out_rx) = mpsc::channel::<WsOutMessage>(256);

    // Register this connection for cross-client broadcast
    studio.register_connection(conn_id, out_tx.clone()).await;

    // Task to forward channel messages to WebSocket
    let mut ws_tx = ws_tx;
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    warn!("Failed to serialize WS message: {e}");
                    continue;
                }
            };
            if ws_tx
                .send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Process incoming messages
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                debug!("WS read error: {e}");
                break;
            }
        };

        let text = match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
            tokio_tungstenite::tungstenite::Message::Close(_) => break,
            tokio_tungstenite::tungstenite::Message::Ping(_) | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
            _ => continue,
        };

        let parsed: WsInMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = out_tx
                    .send(WsOutMessage::Error {
                        message: format!("Invalid message: {e}"),
                        session_id: None,
                    })
                    .await;
                continue;
            }
        };

        match parsed {
            WsInMessage::Prompt { prompt, session_id, mode, model, images } => {
                // If session_id provided and that session is already active, kill it first
                if let Some(ref sid) = session_id {
                    if studio.is_session_active(sid).await {
                        studio.kill_session(sid).await;
                    }
                }
                // Check concurrency limit
                if studio.active_count().await >= studio.max_concurrent {
                    let _ = out_tx.send(WsOutMessage::Error {
                        message: format!("Maximum concurrent sessions ({}) reached", studio.max_concurrent),
                        session_id: session_id.clone(),
                    }).await;
                } else {
                    // Spawn new claude process
                    spawn_claude(studio, conn_id, &prompt, session_id, &mode, model.as_deref(), images, out_tx.clone(), user.as_deref()).await;
                }
            }
            WsInMessage::Abort { session_id } => {
                if let Some(sid) = session_id {
                    // Abort a specific session
                    studio.kill_session(&sid).await;
                    studio.broadcast_all(&WsOutMessage::Done { session_id: Some(sid) }).await;
                } else {
                    // Abort all sessions (backward compat)
                    let active_ids = studio.active_session_ids().await;
                    studio.kill_all_active().await;
                    for sid in active_ids {
                        studio.broadcast_all(&WsOutMessage::Done { session_id: Some(sid) }).await;
                    }
                    // Also send a Done with no session_id for legacy clients
                    studio.broadcast_all(&WsOutMessage::Done { session_id: None }).await;
                }
            }
            WsInMessage::GetActiveStreams => {
                let session_ids = studio.active_session_ids().await;
                let _ = out_tx.send(WsOutMessage::ActiveStreams { session_ids }).await;
            }
            WsInMessage::ListSessions => {
                let sessions = super::sessions::list_sessions();
                let _ = out_tx.send(WsOutMessage::Sessions { sessions }).await;
            }
            WsInMessage::GetSession { session_id, limit } => {
                let messages = super::sessions::get_session_messages(&session_id, limit);
                let _ = out_tx
                    .send(WsOutMessage::SessionMessages { messages, session_id: Some(session_id.clone()) })
                    .await;
                // If there's an active stream for this session, replay buffered events
                studio.replay_session_buffer(&session_id, &out_tx).await;
            }
            WsInMessage::DeleteSession { session_id } => {
                super::sessions::delete_session(&session_id);
                // Broadcast updated session list to all clients
                let sessions = super::sessions::list_sessions();
                studio.broadcast_all(&WsOutMessage::Sessions { sessions }).await;
            }
            WsInMessage::ListDirectory { path } => {
                let out_tx = out_tx.clone();
                tokio::spawn(async move {
                    let resolved = match resolve_safe_path(&path) {
                        Some(p) if p.is_dir() => p,
                        _ => {
                            let _ = out_tx
                                .send(WsOutMessage::Error {
                                    message: format!("Invalid or inaccessible directory: {path}"),
                                    session_id: None,
                                })
                                .await;
                            return;
                        }
                    };

                    let mut read_dir = match tokio::fs::read_dir(&resolved).await {
                        Ok(rd) => rd,
                        Err(e) => {
                            let _ = out_tx
                                .send(WsOutMessage::Error {
                                    message: format!("Failed to read directory: {e}"),
                                    session_id: None,
                                })
                                .await;
                            return;
                        }
                    };

                    let mut entries = Vec::new();
                    while let Ok(Some(entry)) = read_dir.next_entry().await {
                        if entries.len() >= MAX_DIR_ENTRIES {
                            break;
                        }
                        let name = entry.file_name().to_string_lossy().to_string();
                        let meta = match entry.metadata().await {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        let kind = if meta.is_dir() {
                            "directory".to_string()
                        } else {
                            "file".to_string()
                        };
                        let size = meta.len();
                        entries.push(FileEntry { name, kind, size });
                    }

                    // Sort: directories first, then alphabetical case-insensitive
                    entries.sort_by(|a, b| {
                        let dir_a = a.kind == "directory";
                        let dir_b = b.kind == "directory";
                        dir_b
                            .cmp(&dir_a)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });

                    let _ = out_tx
                        .send(WsOutMessage::DirectoryListing { path, entries })
                        .await;
                });
            }
            WsInMessage::ReadFile { path } => {
                let out_tx = out_tx.clone();
                tokio::spawn(async move {
                    let resolved = match resolve_safe_path(&path) {
                        Some(p) if p.is_file() => p,
                        _ => {
                            let _ = out_tx
                                .send(WsOutMessage::Error {
                                    message: format!("Invalid or inaccessible file: {path}"),
                                    session_id: None,
                                })
                                .await;
                            return;
                        }
                    };

                    let meta = match tokio::fs::metadata(&resolved).await {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = out_tx
                                .send(WsOutMessage::Error {
                                    message: format!("Failed to read file metadata: {e}"),
                                    session_id: None,
                                })
                                .await;
                            return;
                        }
                    };

                    let size = meta.len();
                    let (content, truncated) = if size > MAX_FILE_PREVIEW_BYTES {
                        // Read only first 512KB
                        let mut file = match tokio::fs::File::open(&resolved).await {
                            Ok(f) => f,
                            Err(e) => {
                                let _ = out_tx
                                    .send(WsOutMessage::Error {
                                        message: format!("Failed to open file: {e}"),
                                        session_id: None,
                                    })
                                    .await;
                                return;
                            }
                        };
                        let mut buf = vec![0u8; MAX_FILE_PREVIEW_BYTES as usize];
                        let n = match file.read(&mut buf).await {
                            Ok(n) => n,
                            Err(e) => {
                                let _ = out_tx
                                    .send(WsOutMessage::Error {
                                        message: format!("Failed to read file: {e}"),
                                        session_id: None,
                                    })
                                    .await;
                                return;
                            }
                        };
                        buf.truncate(n);
                        let text = String::from_utf8(buf)
                            .unwrap_or_else(|_| "(binary file)".to_string());
                        (text, true)
                    } else {
                        match tokio::fs::read_to_string(&resolved).await {
                            Ok(text) => (text, false),
                            Err(_) => ("(binary file)".to_string(), false),
                        }
                    };

                    let _ = out_tx
                        .send(WsOutMessage::FileContent {
                            path,
                            content,
                            size,
                            truncated,
                        })
                        .await;
                });
            }
            WsInMessage::GetAuthStatus => {
                if let Some(ref username) = user {
                    match super::credentials::get_auth_status(username).await {
                        Some(cred) => {
                            let _ = out_tx.send(WsOutMessage::AuthStatus {
                                authenticated: true,
                                method: Some(cred.method),
                                subscription_type: cred.subscription_type,
                                expires_at: None,
                            }).await;
                        }
                        None => {
                            let _ = out_tx.send(WsOutMessage::AuthStatus {
                                authenticated: false,
                                method: None,
                                subscription_type: None,
                                expires_at: None,
                            }).await;
                        }
                    }
                } else {
                    // No user identity — legacy mode, assume authenticated
                    let _ = out_tx.send(WsOutMessage::AuthStatus {
                        authenticated: true,
                        method: Some("legacy".to_string()),
                        subscription_type: None,
                        expires_at: None,
                    }).await;
                }
            }
            WsInMessage::SubmitToken { token } => {
                if let Some(ref username) = user {
                    match super::credentials::save_token(username, &token).await {
                        Ok(()) => {
                            let _ = out_tx.send(WsOutMessage::AuthStatus {
                                authenticated: true,
                                method: Some("token".to_string()),
                                subscription_type: None,
                                expires_at: None,
                            }).await;
                        }
                        Err(e) => {
                            let _ = out_tx.send(WsOutMessage::Error {
                                message: format!("Failed to save token: {e}"),
                                session_id: None,
                            }).await;
                        }
                    }
                } else {
                    let _ = out_tx.send(WsOutMessage::Error {
                        message: "No user identity available.".to_string(),
                        session_id: None,
                    }).await;
                }
            }
            WsInMessage::UnlinkAuth => {
                if let Some(ref username) = user {
                    let _ = super::credentials::remove_credentials(username).await;
                    let _ = out_tx.send(WsOutMessage::AuthStatus {
                        authenticated: false,
                        method: None,
                        subscription_type: None,
                        expires_at: None,
                    }).await;
                } else {
                    let _ = out_tx.send(WsOutMessage::Error {
                        message: "No user identity available.".to_string(),
                        session_id: None,
                    }).await;
                }
            }
            WsInMessage::ConsoleLogs { logs } => {
                tokio::spawn(async move {
                    let log_path = "/tmp/studio-console-logs.json";
                    // Read existing logs
                    let mut existing: Vec<serde_json::Value> = match tokio::fs::read_to_string(log_path).await {
                        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                        Err(_) => Vec::new(),
                    };
                    // Append new logs
                    for entry in logs {
                        existing.push(serde_json::json!({
                            "level": entry.level,
                            "message": entry.message,
                            "timestamp": entry.timestamp,
                        }));
                    }
                    // Keep max 500 entries (FIFO)
                    if existing.len() > 500 {
                        existing = existing.split_off(existing.len() - 500);
                    }
                    // Write updated file
                    if let Ok(content) = serde_json::to_string(&existing) {
                        let _ = tokio::fs::write(log_path, content).await;
                    }
                });
            }
        }
    }

    // Cleanup
    writer_handle.abort();
}

/// Save base64-encoded images to temp files, returning the file paths.
async fn save_image_attachments(images: &[ImageAttachment]) -> Vec<String> {
    let mut paths = Vec::new();
    let upload_dir = "/tmp/studio-uploads";
    let _ = tokio::fs::create_dir_all(upload_dir).await;

    for img in images {
        let ext = match img.media_type.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        };
        let id = uuid::Uuid::new_v4();
        let path = format!("{upload_dir}/{id}.{ext}");

        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(&img.data) {
            Ok(bytes) => {
                if tokio::fs::write(&path, &bytes).await.is_ok() {
                    // Make readable by studio user
                    let _ = tokio::fs::set_permissions(
                        &path,
                        std::os::unix::fs::PermissionsExt::from_mode(0o644),
                    ).await;
                    paths.push(path);
                }
            }
            Err(e) => {
                warn!("Failed to decode base64 image: {e}");
            }
        }
    }
    paths
}

/// Spawn a Claude CLI subprocess and stream its output.
async fn spawn_claude(
    studio: &Arc<StudioBridge>,
    conn_id: &str,
    prompt: &str,
    session_id: Option<String>,
    mode: &str,
    model: Option<&str>,
    images: Option<Vec<ImageAttachment>>,
    out_tx: mpsc::Sender<WsOutMessage>,
    user: Option<&str>,
) {
    // Per-user credential setup (if user identity is available)
    if let Some(username) = user {
        match super::credentials::get_auth_status(username).await {
            Some(cred) if cred.method == "token" && cred.token.is_some() => {
                // Token method: will pass CLAUDE_CODE_OAUTH_TOKEN env var below
                // Ensure hasCompletedOnboarding
                let claude_json = format!("{STUDIO_HOME}/.claude.json");
                if !std::path::Path::new(&claude_json).exists() {
                    let _ = tokio::fs::write(&claude_json, r#"{"hasCompletedOnboarding":true}"#).await;
                }
            }
            Some(_) => {
                // OAuth or native claudeAiOauth: Claude uses its own credentials from HOME
                // No env var needed — claude CLI will find /home/studio/.claude/.credentials.json
            }
            None => {
                let _ = out_tx.send(WsOutMessage::Error {
                    message: "No Claude credentials configured. Use the auth panel to link your Claude account.".to_string(),
                    session_id: session_id.clone(),
                }).await;
                return;
            }
        }
    } else {
        // Legacy fallback: no user identity, use shared credentials
        if CREDENTIALS_SYNCED.get().is_none() {
            sync_studio_credentials().await;
            let _ = CREDENTIALS_SYNCED.set(());
        }
    }

    // Save any attached images to temp files
    let image_paths = if let Some(imgs) = &images {
        save_image_attachments(imgs).await
    } else {
        Vec::new()
    };

    // Build the prompt, prepending image references if any
    let effective_prompt = if image_paths.is_empty() {
        prompt.to_string()
    } else {
        let refs: Vec<String> = image_paths.iter()
            .map(|p| format!("[Attached image: {p} — use the Read tool to view it]"))
            .collect();
        format!("{}\n\n{}", refs.join("\n"), prompt)
    };

    // Build the claude command arguments
    let mut claude_args = vec![
        "-p".to_string(),
        effective_prompt,
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];

    // Model override
    if let Some(m) = model {
        claude_args.push("--model".to_string());
        claude_args.push(m.to_string());
    }

    // Plan mode: restrict to read-only tools. Both modes use bypassPermissions (headless).
    if mode == "plan" {
        claude_args.push("--allowedTools".to_string());
        claude_args.push("Read,Glob,Grep,WebSearch,WebFetch,AskUserQuestion,TodoWrite".to_string());
    }
    claude_args.push("--permission-mode".to_string());
    claude_args.push("bypassPermissions".to_string());

    if let Some(sid) = &session_id {
        claude_args.push("--resume".to_string());
        claude_args.push(sid.clone());
    }

    // Run as non-root "studio" user via runuser
    // This allows --permission-mode bypassPermissions (blocked as root)
    // Build PATH that includes root's nvm/cargo/local bins so Claude's Bash tools work
    let mut extra_paths = vec![
        "/root/.cargo/bin".to_string(),
        "/root/.local/bin".to_string(),
    ];
    // Auto-detect nvm node version
    let nvm_dir = std::path::Path::new("/root/.nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(nvm_dir) {
        if let Some(entry) = entries.filter_map(|e| e.ok()).last() {
            extra_paths.insert(0, format!("{}/bin", entry.path().display()));
        }
    }
    let sys_path = std::env::var("PATH").unwrap_or_default();
    let full_path = format!(
        "{}:{}",
        extra_paths.join(":"),
        sys_path
    );

    let mut cmd = Command::new("runuser");
    cmd.arg("-u")
        .arg(STUDIO_USER)
        .arg("--")
        .arg("env")
        .arg(format!("HOME={STUDIO_HOME}"))
        .arg(format!("PATH={full_path}"));

    // Add token env var if user is using token auth method
    if let Some(username) = user {
        if let Some(cred) = super::credentials::get_auth_status(username).await {
            if cred.method == "token" {
                if let Some(ref token) = cred.token {
                    cmd.arg(format!("CLAUDE_CODE_OAUTH_TOKEN={}", token));
                }
            }
        }
    }

    cmd.arg("claude")
        .args(&claude_args);

    cmd.current_dir("/root/workspace");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn claude: {e}");
            let _ = out_tx
                .send(WsOutMessage::Error {
                    message: format!("Failed to start Claude: {e}"),
                    session_id: session_id.clone(),
                })
                .await;
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            error!("No stdout from claude process");
            let _ = out_tx
                .send(WsOutMessage::Error {
                    message: "No stdout from Claude process".to_string(),
                    session_id: session_id.clone(),
                })
                .await;
            return;
        }
    };

    // Use a temp key for the process until we discover the real session_id from stdout
    let temp_key = session_id.clone().unwrap_or_else(|| format!("_tmp_{}", conn_id));
    let initial_session_id = session_id.clone();

    // Spawn stderr reader to capture and forward errors
    if let Some(stderr) = child.stderr.take() {
        let out_tx_err = out_tx.clone();
        let stderr_session_id = initial_session_id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut stderr_buf = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!("claude stderr: {}", line);
                if !line.is_empty() {
                    stderr_buf.push_str(&line);
                    stderr_buf.push('\n');
                }
            }
            // Always forward stderr to frontend, truncating if necessary
            if !stderr_buf.is_empty() {
                let msg = if stderr_buf.len() > 4000 {
                    format!("{}...\n(truncated)", &stderr_buf[..4000])
                } else {
                    stderr_buf.trim().to_string()
                };
                let _ = out_tx_err
                    .send(WsOutMessage::Error {
                        message: msg,
                        session_id: stderr_session_id,
                    })
                    .await;
            }
        });
    }

    // Spawn reader task to stream stdout (crash detection only, no timeout)
    let studio_reader = Arc::clone(studio);
    let temp_key_reader = temp_key.clone();
    let reader_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut found_session_id: Option<String> = None;
        let mut saw_result = false;
        // The key currently used in active_processes — starts as temp_key, re-keyed when session_id discovered
        let mut current_key = temp_key_reader.clone();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(data) => {
                            // Extract session_id from stream events and re-key if needed
                            if found_session_id.is_none() {
                                if let Some(sid) = data.get("session_id").and_then(|v| v.as_str()) {
                                    found_session_id = Some(sid.to_string());
                                    // Re-key from temp key to real session_id
                                    if current_key != sid {
                                        let mut procs = studio_reader.active_processes.write().await;
                                        if let Some(proc) = procs.remove(&current_key) {
                                            procs.insert(sid.to_string(), proc);
                                        }
                                        drop(procs);
                                        current_key = sid.to_string();
                                    }
                                }
                            }
                            // Track if we see a result event
                            if data.get("type").and_then(|v| v.as_str()) == Some("result") {
                                saw_result = true;
                            }
                            // Buffer + broadcast stream events tagged with session_id
                            let sid = found_session_id.clone();
                            let buffer_key = sid.clone().unwrap_or_else(|| current_key.clone());
                            studio_reader.buffer_and_broadcast_session(
                                &buffer_key,
                                WsOutMessage::Stream { data, session_id: sid },
                            ).await;
                        }
                        Err(_) => {
                            debug!("Non-JSON line from claude ({}): {}", current_key, line);
                        }
                    }
                }
                Ok(None) => {
                    // EOF — stream ended
                    if !saw_result {
                        studio_reader.broadcast_all(&WsOutMessage::Error {
                            message: "Claude process ended unexpectedly".to_string(),
                            session_id: found_session_id.clone(),
                        }).await;
                    }
                    break;
                }
                Err(e) => {
                    warn!("Reader IO error for {}: {e}", current_key);
                    studio_reader.broadcast_all(&WsOutMessage::Error {
                        message: format!("Read error: {e}"),
                        session_id: found_session_id.clone(),
                    }).await;
                    break;
                }
            }
        }

        // Clear session buffer before Done so new connections don't get stale data
        let final_key = found_session_id.clone().unwrap_or(current_key.clone());
        studio_reader.clear_session_buffer(&final_key).await;

        // Remove this process from active_processes (it finished naturally)
        studio_reader.active_processes.write().await.remove(&current_key);
        // Fix race: also clean up by temp key in case re-keying happened before the insert
        studio_reader.active_processes.write().await.remove(&temp_key_reader);

        // Stream ended — broadcast Done and refresh session list
        let done_msg = WsOutMessage::Done {
            session_id: found_session_id,
        };
        studio_reader.broadcast_all(&done_msg).await;

        // Broadcast updated session list so all clients stay in sync
        let sessions = super::sessions::list_sessions();
        studio_reader.broadcast_all(&WsOutMessage::Sessions { sessions }).await;
    });

    let reader_abort = reader_handle.abort_handle();

    // Store the process keyed by session_id (or temp key for new sessions)
    let process = super::ClaudeProcess {
        child,
        reader_abort,
    };
    studio
        .active_processes
        .write()
        .await
        .insert(temp_key, process);

    // No global watchdog — the reader task's progressive timeout handles stalls:
    // 2 min no output → warning, 4 min no output → kill.
    // A global watchdog would kill actively-working Claude processes.
}

