pub mod types;
pub mod static_files;
pub mod handler;
pub mod websocket;
pub mod sessions;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::accept_async;
use tracing::{error, info};

pub const STUDIO_WS_PORT: u16 = 3839;

pub struct StudioBridge {
    pub active_processes: RwLock<HashMap<String, ClaudeProcess>>,
}

pub struct ClaudeProcess {
    pub child: tokio::process::Child,
    pub reader_abort: tokio::task::AbortHandle,
}

impl StudioBridge {
    pub fn new() -> Self {
        Self {
            active_processes: RwLock::new(HashMap::new()),
        }
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

                    websocket::kill_active(&studio, &conn_id).await;
                });
            }
        })
    }
}
