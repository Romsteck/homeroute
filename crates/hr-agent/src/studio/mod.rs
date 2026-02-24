pub mod types;
pub mod static_files;
pub mod handler;
pub mod websocket;
pub mod sessions;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::accept_async;
use tracing::{debug, error, info, warn};

use types::WsOutMessage;

pub const STUDIO_WS_PORT: u16 = 3839;

pub struct StudioBridge {
    pub active_processes: RwLock<HashMap<String, ClaudeProcess>>,
    /// All connected WebSocket clients, keyed by connection ID.
    connections: RwLock<HashMap<String, mpsc::Sender<WsOutMessage>>>,
    /// Buffer of stream events for the current Claude turn.
    /// Replayed to new connections that join mid-stream (e.g. page refresh).
    stream_buffer: RwLock<Vec<WsOutMessage>>,
}

pub struct ClaudeProcess {
    pub child: tokio::process::Child,
    pub reader_abort: tokio::task::AbortHandle,
}

impl StudioBridge {
    pub fn new() -> Self {
        Self {
            active_processes: RwLock::new(HashMap::new()),
            connections: RwLock::new(HashMap::new()),
            stream_buffer: RwLock::new(Vec::new()),
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

    /// Broadcast a message to all connected clients except `exclude`.
    pub async fn broadcast(&self, exclude: &str, msg: &WsOutMessage) {
        let conns = self.connections.read().await;
        for (id, tx) in conns.iter() {
            if id != exclude {
                if tx.send(msg.clone()).await.is_err() {
                    warn!("Failed to broadcast to connection {}", id);
                }
            }
        }
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

    /// Buffer a stream event and broadcast it to all connections.
    /// Used for stream events so new connections can catch up.
    pub async fn buffer_and_broadcast(&self, msg: WsOutMessage) {
        self.stream_buffer.write().await.push(msg.clone());
        self.broadcast_all(&msg).await;
    }

    /// Clear the stream buffer (called on Done or new Prompt).
    pub async fn clear_stream_buffer(&self) {
        self.stream_buffer.write().await.clear();
    }

    /// Replay buffered stream events to a specific connection.
    /// Called after get_session so the client catches up on an ongoing stream.
    pub async fn replay_stream_buffer(&self, tx: &mpsc::Sender<WsOutMessage>) {
        let buf = self.stream_buffer.read().await;
        for msg in buf.iter() {
            if tx.send(msg.clone()).await.is_err() {
                break;
            }
        }
    }

    /// Kill ALL active Claude processes (used on new Prompt or Abort).
    pub async fn kill_all_active(&self) {
        let mut procs = self.active_processes.write().await;
        for (id, mut proc) in procs.drain() {
            proc.reader_abort.abort();
            let _ = proc.child.kill().await;
            debug!("Killed active Claude process for {}", id);
        }
        self.stream_buffer.write().await.clear();
    }

    /// Start the Studio WebSocket server on a local port.
    /// This runs a plain TCP server that accepts WebSocket connections via tokio-tungstenite.
    /// The agent proxy routes WS requests to this port using standard bidirectional copy.
    pub fn start_ws_server(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
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
                    let ws = match accept_async(stream).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            error!("Studio WS handshake failed from {}: {e}", addr);
                            return;
                        }
                    };

                    info!("Studio WebSocket connected: {}", conn_id);
                    websocket::run_ws_session(ws, &studio, &conn_id).await;
                    info!("Studio WebSocket disconnected: {}", conn_id);
                    // Don't kill active process on disconnect — let it finish
                    // so the response is saved and other clients keep receiving the stream.
                    studio.unregister_connection(&conn_id).await;
                });
            }
        })
    }
}
