use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, error, warn};

use super::types::{WsInMessage, WsOutMessage};
use super::StudioBridge;

/// Main WebSocket session loop.
/// Called from StudioBridge::start_ws_server() after tokio-tungstenite accept_async().
pub async fn run_ws_session<S>(ws: WebSocketStream<S>, studio: &Arc<StudioBridge>, conn_id: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (ws_tx, mut ws_rx) = ws.split();

    // Channel for outgoing messages
    let (out_tx, mut out_rx) = mpsc::channel::<WsOutMessage>(256);

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
                let _ = out_tx
                    .send(WsOutMessage::Done { session_id: None })
                    .await;
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
                // Send updated session list
                let sessions = super::sessions::list_sessions();
                let _ = out_tx.send(WsOutMessage::Sessions { sessions }).await;
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
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");

    // Model override
    if let Some(m) = model {
        cmd.arg("--model").arg(m);
    }

    // Permission mode: plan = read-only analysis, default = full execution
    match mode {
        "plan" => {
            cmd.arg("--permission-mode").arg("plan");
            cmd.arg("--allowedTools").arg("Read,Glob,Grep,WebSearch,WebFetch");
        }
        _ => {
            cmd.arg("--allowedTools")
                .arg("Read,Write,Edit,Bash,Glob,Grep,WebSearch,WebFetch,TodoWrite,Task,NotebookEdit");
        }
    }

    if let Some(sid) = &session_id {
        cmd.arg("--resume").arg(sid);
    }

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
            // If there's stderr output and it looks like an error, send to frontend
            if !stderr_buf.is_empty() && stderr_buf.len() < 2000 {
                let _ = out_tx_err
                    .send(WsOutMessage::Error {
                        message: stderr_buf.trim().to_string(),
                    })
                    .await;
            }
        });
    }

    // Spawn reader task to stream stdout
    let out_tx_clone = out_tx.clone();
    let conn_id_owned = conn_id.to_string();
    let reader_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut found_session_id: Option<String> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(data) => {
                    // Extract session_id from stream events
                    if found_session_id.is_none() {
                        if let Some(sid) = data.get("session_id").and_then(|v| v.as_str()) {
                            found_session_id = Some(sid.to_string());
                        }
                    }
                    if out_tx_clone
                        .send(WsOutMessage::Stream { data })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => {
                    debug!("Non-JSON line from claude ({}): {}", conn_id_owned, line);
                }
            }
        }

        // Stream ended
        let _ = out_tx_clone
            .send(WsOutMessage::Done {
                session_id: found_session_id,
            })
            .await;
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

    // Spawn a timeout watchdog (10 minutes)
    let studio_clone = Arc::clone(studio);
    let conn_id_owned = conn_id.to_string();
    let out_tx_clone = out_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(600)).await;
        // If process is still running after timeout, kill it
        let mut procs = studio_clone.active_processes.write().await;
        if let Some(mut proc) = procs.remove(&conn_id_owned) {
            warn!("Claude process timed out for {}", conn_id_owned);
            proc.reader_abort.abort();
            let _ = proc.child.kill().await;
            let _ = out_tx_clone
                .send(WsOutMessage::Error {
                    message: "Claude process timed out (10 minutes)".to_string(),
                })
                .await;
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
