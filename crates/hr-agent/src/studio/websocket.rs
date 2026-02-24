use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, error, warn};

use super::types::{FileEntry, WsInMessage, WsOutMessage};
use super::StudioBridge;

const WORKSPACE_ROOT: &str = "/root/workspace";
const STUDIO_USER: &str = "studio";
const STUDIO_HOME: &str = "/home/studio";
const MAX_FILE_PREVIEW_BYTES: u64 = 512_000;
const MAX_DIR_ENTRIES: usize = 1000;

/// Sync Claude credentials from root to studio user's home.
/// Studio user is pre-created during container provisioning.
async fn sync_studio_credentials() {
    let studio_claude = format!("{STUDIO_HOME}/.claude");
    let _ = tokio::fs::create_dir_all(&studio_claude).await;

    // Copy credentials file (may be refreshed by agent)
    let src = "/root/.claude/.credentials.json";
    let dst = format!("{studio_claude}/.credentials.json");
    if std::path::Path::new(src).exists() {
        let _ = tokio::fs::copy(src, &dst).await;
    }

    // Copy settings
    let src = "/root/.claude/settings.json";
    let dst = format!("{studio_claude}/settings.json");
    if std::path::Path::new(src).exists() {
        let _ = tokio::fs::copy(src, &dst).await;
    }

    // Ensure studio user owns its home
    let _ = tokio::process::Command::new("chown")
        .args(["-R", &format!("{STUDIO_USER}:{STUDIO_USER}"), STUDIO_HOME])
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
/// Called from StudioBridge::start_ws_server() after tokio-tungstenite accept_async().
pub async fn run_ws_session<S>(ws: WebSocketStream<S>, studio: &Arc<StudioBridge>, conn_id: &str)
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
                    })
                    .await;
                continue;
            }
        };

        match parsed {
            WsInMessage::Prompt { prompt, session_id, mode, model } => {
                // Kill any active subprocess first
                kill_active(studio, conn_id).await;
                // Spawn new claude process
                spawn_claude(studio, conn_id, &prompt, session_id, &mode, model.as_deref(), out_tx.clone()).await;
            }
            WsInMessage::Abort => {
                kill_active(studio, conn_id).await;
                studio.broadcast_all(&WsOutMessage::Done { session_id: None }).await;
            }
            WsInMessage::ListSessions => {
                let sessions = super::sessions::list_sessions();
                let _ = out_tx.send(WsOutMessage::Sessions { sessions }).await;
            }
            WsInMessage::GetSession { session_id, limit } => {
                let messages = super::sessions::get_session_messages(&session_id, limit);
                let _ = out_tx
                    .send(WsOutMessage::SessionMessages { messages })
                    .await;
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
        }
    }

    // Cleanup
    writer_handle.abort();
}

/// Spawn a Claude CLI subprocess and stream its output.
async fn spawn_claude(
    studio: &Arc<StudioBridge>,
    conn_id: &str,
    prompt: &str,
    session_id: Option<String>,
    mode: &str,
    model: Option<&str>,
    out_tx: mpsc::Sender<WsOutMessage>,
) {
    // Sync credentials to studio user (pre-created during provisioning)
    sync_studio_credentials().await;

    // Build the claude command arguments
    let mut claude_args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];

    // Model override
    if let Some(m) = model {
        claude_args.push("--model".to_string());
        claude_args.push(m.to_string());
    }

    // Plan mode: restrict to read-only tools. Execute mode: bypass permissions
    if mode == "plan" {
        claude_args.push("--allowedTools".to_string());
        claude_args.push("Read,Glob,Grep,WebSearch,WebFetch".to_string());
    } else {
        claude_args.push("--permission-mode".to_string());
        claude_args.push("bypassPermissions".to_string());
    }

    if let Some(sid) = &session_id {
        claude_args.push("--resume".to_string());
        claude_args.push(sid.clone());
    }

    // Run as non-root "studio" user via runuser
    // This allows --permission-mode bypassPermissions (blocked as root)
    let mut cmd = Command::new("runuser");
    cmd.arg("-u")
        .arg(STUDIO_USER)
        .arg("--")
        .arg("env")
        .arg(format!("HOME={STUDIO_HOME}"))
        .arg("claude")
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
                })
                .await;
            return;
        }
    };

    // Spawn stderr reader to capture and forward errors
    if let Some(stderr) = child.stderr.take() {
        let out_tx_err = out_tx.clone();
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
                    .send(WsOutMessage::Error { message: msg })
                    .await;
            }
        });
    }

    // Spawn reader task to stream stdout (with progressive timeout + crash detection)
    let studio_reader = Arc::clone(studio);
    let conn_id_owned = conn_id.to_string();
    let reader_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut found_session_id: Option<String> = None;
        let mut saw_result = false;
        let mut consecutive_timeouts: u32 = 0;

        loop {
            match tokio::time::timeout(Duration::from_secs(120), lines.next_line()).await {
                Ok(Ok(Some(line))) => {
                    consecutive_timeouts = 0;
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(data) => {
                            // Extract session_id from stream events
                            if found_session_id.is_none() {
                                if let Some(sid) = data.get("session_id").and_then(|v| v.as_str()) {
                                    found_session_id = Some(sid.to_string());
                                }
                            }
                            // Track if we see a result event
                            if data.get("type").and_then(|v| v.as_str()) == Some("result") {
                                saw_result = true;
                            }
                            // Broadcast stream events to all connected clients
                            studio_reader.broadcast_all(&WsOutMessage::Stream { data }).await;
                        }
                        Err(_) => {
                            debug!("Non-JSON line from claude ({}): {}", conn_id_owned, line);
                        }
                    }
                }
                Ok(Ok(None)) => {
                    // EOF — stream ended
                    if !saw_result {
                        studio_reader.broadcast_all(&WsOutMessage::Error {
                            message: "Claude process ended unexpectedly".to_string(),
                        }).await;
                    }
                    break;
                }
                Ok(Err(e)) => {
                    warn!("Reader IO error for {}: {e}", conn_id_owned);
                    studio_reader.broadcast_all(&WsOutMessage::Error {
                        message: format!("Read error: {e}"),
                    }).await;
                    break;
                }
                Err(_) => {
                    // Timeout — no output for 120s
                    consecutive_timeouts += 1;
                    if consecutive_timeouts == 1 {
                        warn!("Claude process inactive for 2 min ({})", conn_id_owned);
                        studio_reader.broadcast_all(&WsOutMessage::Error {
                            message: "Warning: Claude has been inactive for 2 minutes".to_string(),
                        }).await;
                    } else {
                        // 2nd consecutive timeout (4 min total) — kill the process
                        warn!("Claude process inactive for 4 min, killing ({})", conn_id_owned);
                        studio_reader.broadcast_all(&WsOutMessage::Error {
                            message: "Claude process killed after 4 minutes of inactivity".to_string(),
                        }).await;
                        // Kill the process from active_processes
                        let mut procs = studio_reader.active_processes.write().await;
                        if let Some(mut proc) = procs.remove(&conn_id_owned) {
                            let _ = proc.child.kill().await;
                        }
                        break;
                    }
                }
            }
        }

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

    // Store the process
    let process = super::ClaudeProcess {
        child,
        reader_abort,
    };
    studio
        .active_processes
        .write()
        .await
        .insert(conn_id.to_string(), process);

    // Spawn a timeout watchdog (5 minutes — safety net)
    let studio_watchdog = Arc::clone(studio);
    let conn_id_owned = conn_id.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        // If process is still running after timeout, kill it
        let mut procs = studio_watchdog.active_processes.write().await;
        if let Some(mut proc) = procs.remove(&conn_id_owned) {
            warn!("Claude process timed out for {}", conn_id_owned);
            proc.reader_abort.abort();
            let _ = proc.child.kill().await;
            studio_watchdog.broadcast_all(&WsOutMessage::Error {
                message: "Claude process timed out (5 minutes)".to_string(),
            }).await;
        }
    });
}

/// Kill any active Claude subprocess for a connection.
pub async fn kill_active(studio: &Arc<StudioBridge>, conn_id: &str) {
    let mut procs = studio.active_processes.write().await;
    if let Some(mut proc) = procs.remove(conn_id) {
        proc.reader_abort.abort();
        let _ = proc.child.kill().await;
        debug!("Killed active Claude process for {}", conn_id);
    }
}
