pub mod types;
pub mod static_files;
pub mod handler;
pub mod websocket;
pub mod sessions;
pub mod credentials;
pub mod todo_watcher;
pub mod terminal;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response, ErrorResponse};
use tracing::{debug, error, info, warn};

use types::WsOutMessage;

pub const STUDIO_WS_PORT: u16 = 3839;

pub struct StudioBridge {
    /// Active Claude processes, keyed by session_id.
    pub active_processes: RwLock<HashMap<String, ClaudeProcess>>,
    /// Active terminal (PTY/tmux) sessions, keyed by session_id.
    pub terminal_sessions: RwLock<HashMap<String, TerminalSession>>,
    /// All connected WebSocket clients, keyed by connection ID.
    connections: RwLock<HashMap<String, mpsc::Sender<WsOutMessage>>>,
    /// Per-session stream buffers for replaying events to late joiners.
    stream_buffers: RwLock<HashMap<String, Vec<WsOutMessage>>>,
    /// Maximum number of concurrent Claude sessions.
    pub max_concurrent: usize,
}

pub struct ClaudeProcess {
    pub child: tokio::process::Child,
    pub reader_abort: tokio::task::AbortHandle,
}

pub struct TerminalSession {
    pub tmux_name: String,
    pub reader_abort: tokio::task::AbortHandle,
    pub created_at: u64,
}

impl StudioBridge {
    pub fn new() -> Self {
        Self {
            active_processes: RwLock::new(HashMap::new()),
            terminal_sessions: RwLock::new(HashMap::new()),
            connections: RwLock::new(HashMap::new()),
            stream_buffers: RwLock::new(HashMap::new()),
            max_concurrent: 3,
        }
    }

    /// Register a WebSocket connection for broadcast.
    pub async fn register_connection(&self, conn_id: &str, tx: mpsc::Sender<WsOutMessage>) {
        self.connections
            .write()
            .await
            .insert(conn_id.to_string(), tx);
    }

    /// Unregister a WebSocket connection.
    pub async fn unregister_connection(&self, conn_id: &str) {
        self.connections.write().await.remove(conn_id);
    }

    /// Broadcast a message to ALL connected clients.
    pub async fn broadcast_all(&self, msg: &WsOutMessage) {
        let conns = self.connections.read().await;
        for (id, tx) in conns.iter() {
            if tx.send(msg.clone()).await.is_err() {
                warn!("Failed to broadcast to connection {}", id);
            }
        }
    }

    /// Buffer a stream event for a specific session and broadcast it to all connections.
    pub async fn buffer_and_broadcast_session(&self, session_id: &str, msg: WsOutMessage) {
        self.stream_buffers
            .write()
            .await
            .entry(session_id.to_string())
            .or_default()
            .push(msg.clone());
        self.broadcast_all(&msg).await;
    }

    /// Clear the stream buffer for a specific session.
    pub async fn clear_session_buffer(&self, session_id: &str) {
        self.stream_buffers.write().await.remove(session_id);
    }

    /// Replay buffered stream events for a specific session to a connection.
    pub async fn replay_session_buffer(&self, session_id: &str, tx: &mpsc::Sender<WsOutMessage>) {
        let bufs = self.stream_buffers.read().await;
        if let Some(buf) = bufs.get(session_id) {
            for msg in buf.iter() {
                if tx.send(msg.clone()).await.is_err() {
                    break;
                }
            }
        }
    }

    /// Kill a specific session's Claude process and clear its buffer.
    pub async fn kill_session(&self, session_id: &str) {
        if let Some(mut proc) = self.active_processes.write().await.remove(session_id) {
            proc.reader_abort.abort();
            let _ = proc.child.kill().await;
            debug!("Killed active Claude process for session {}", session_id);
        }
        self.clear_session_buffer(session_id).await;
    }

    /// Kill ALL active Claude processes (backward compat for abort-all).
    pub async fn kill_all_active(&self) {
        let mut procs = self.active_processes.write().await;
        for (id, mut proc) in procs.drain() {
            proc.reader_abort.abort();
            let _ = proc.child.kill().await;
            debug!("Killed active Claude process for session {}", id);
        }
        self.stream_buffers.write().await.clear();
    }

    /// Number of currently active sessions (agent + terminal).
    pub async fn active_count(&self) -> usize {
        self.active_processes.read().await.len() + self.terminal_sessions.read().await.len()
    }

    /// Check if a specific session is currently active.
    pub async fn is_session_active(&self, session_id: &str) -> bool {
        self.active_processes.read().await.contains_key(session_id)
    }

    /// Get all active session IDs.
    pub async fn active_session_ids(&self) -> Vec<String> {
        self.active_processes.read().await.keys().cloned().collect()
    }

    /// Kill a terminal session by session_id.
    pub async fn kill_terminal(&self, session_id: &str) {
        if let Some(ts) = self.terminal_sessions.write().await.remove(session_id) {
            ts.reader_abort.abort();
            // Must run tmux as studio user to access studio's tmux server
            let _ = tokio::process::Command::new("runuser")
                .args(["-u", "studio", "--", "tmux", "kill-session", "-t", &ts.tmux_name])
                .output()
                .await;
            debug!("Killed terminal session {} (tmux: {})", session_id, ts.tmux_name);
        }
    }

    /// Check if a terminal session is active.
    pub async fn is_terminal_active(&self, session_id: &str) -> bool {
        self.terminal_sessions.read().await.contains_key(session_id)
    }

    /// Start the Studio WebSocket server on a local port.
    /// This runs a plain TCP server that accepts WebSocket connections via tokio-tungstenite.
    /// The agent proxy routes WS requests to this port using standard bidirectional copy.
    pub fn start_ws_server(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        // Start the JSONL todo watcher
        todo_watcher::start(Arc::clone(self));

        let studio = Arc::clone(self);
        tokio::spawn(async move {
            let listener = match TcpListener::bind(format!("127.0.0.1:{}", STUDIO_WS_PORT)).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind Studio WS server on port {}: {e}", STUDIO_WS_PORT);
                    return;
                }
            };
            info!("Studio WebSocket server listening on 127.0.0.1:{}", STUDIO_WS_PORT);

            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Studio WS accept error: {e}");
                        continue;
                    }
                };

                let studio = Arc::clone(&studio);
                let conn_id = uuid::Uuid::new_v4().to_string();

                tokio::spawn(async move {
                    let user = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
                    let user_clone = user.clone();
                    let callback = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                        if let Some(val) = req.headers().get("x-forwarded-user") {
                            if let Ok(s) = val.to_str() {
                                *user_clone.lock().unwrap() = Some(s.to_string());
                            }
                        }
                        Ok(resp)
                    };
                    let ws = match tokio_tungstenite::accept_hdr_async(stream, callback).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            error!("Studio WS handshake failed from {}: {e}", addr);
                            return;
                        }
                    };
                    let user = user.lock().unwrap().take();

                    info!("Studio WebSocket connected: {} (user: {:?})", conn_id, user);
                    websocket::run_ws_session(ws, &studio, &conn_id, user).await;
                    info!("Studio WebSocket disconnected: {}", conn_id);
                    // Don't kill active process on disconnect — let it finish
                    // so the response is saved and other clients keep receiving the stream.
                    studio.unregister_connection(&conn_id).await;
                });
            }
        })
    }
}
