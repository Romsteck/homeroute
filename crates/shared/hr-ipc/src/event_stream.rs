//! Push-based event stream between services via Unix socket.
//!
//! The orchestrator runs `serve_event_stream` which accepts connections and
//! pushes `IpcEvent` lines as they happen (no polling).
//! Clients (homeroute) connect with `connect_event_stream` and receive events.

use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::events::IpcEvent;

/// Default socket path for the orchestrator event stream.
pub const EVENT_STREAM_SOCKET: &str = "/run/hr-orchestrator-events.sock";

/// Serve an event stream on a Unix socket.
/// For each connected client, subscribes to the broadcast channels and pushes JSON lines.
pub async fn serve_event_stream(
    socket_path: &Path,
    app_state_rx: broadcast::Sender<hr_common::events::AppStateEvent>,
    log_rx: broadcast::Sender<hr_common::logging::LogEntry>,
    app_build_rx: broadcast::Sender<hr_common::events::AppBuildEvent>,
) -> anyhow::Result<()> {
    // Remove stale socket
    let _ = tokio::fs::remove_file(socket_path).await;
    let listener = UnixListener::bind(socket_path)?;
    info!(path = %socket_path.display(), "Event stream server listening");

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let app_tx = app_state_rx.clone();
                let log_tx = log_rx.clone();
                let build_tx = app_build_rx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_event_client(stream, app_tx, log_tx, build_tx).await {
                        warn!(error = %e, "Event stream client disconnected");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "Event stream accept error");
            }
        }
    }
}

async fn handle_event_client(
    stream: UnixStream,
    app_state_tx: broadcast::Sender<hr_common::events::AppStateEvent>,
    log_tx: broadcast::Sender<hr_common::logging::LogEntry>,
    app_build_tx: broadcast::Sender<hr_common::events::AppBuildEvent>,
) -> anyhow::Result<()> {
    let (_, mut writer) = stream.into_split();
    let mut app_rx = app_state_tx.subscribe();
    let mut log_rx = log_tx.subscribe();
    let mut build_rx = app_build_tx.subscribe();

    info!("Event stream client connected");

    loop {
        let event: IpcEvent = tokio::select! {
            Ok(ev) = app_rx.recv() => {
                IpcEvent {
                    channel: "app:state".to_string(),
                    payload: serde_json::to_value(&ev).unwrap_or_default(),
                }
            }
            Ok(ev) = build_rx.recv() => {
                IpcEvent {
                    channel: "app:build".to_string(),
                    payload: serde_json::to_value(&ev).unwrap_or_default(),
                }
            }
            Ok(entry) = log_rx.recv() => {
                // Only forward app-tagged log entries (not system logs)
                let data = entry.data.as_ref().and_then(|d| d.as_object());
                let has_app = data.map(|d| d.contains_key("app_slug")).unwrap_or(false);
                if !has_app { continue; }
                IpcEvent {
                    channel: "app:log".to_string(),
                    payload: serde_json::json!({
                        "slug": data.and_then(|d| d.get("app_slug")).and_then(|v| v.as_str()).unwrap_or(""),
                        "level": format!("{:?}", entry.level).to_lowercase(),
                        "message": entry.message,
                        "timestamp": entry.timestamp.to_rfc3339(),
                    }),
                }
            }
            else => break,
        };

        let mut line = serde_json::to_string(&event)?;
        line.push('\n');
        if writer.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }

    Ok(())
}

/// Connect to an event stream and re-emit events on the local EventBus.
pub async fn connect_event_stream(
    socket_path: &Path,
    events: std::sync::Arc<hr_common::events::EventBus>,
) {
    loop {
        match UnixStream::connect(socket_path).await {
            Ok(stream) => {
                info!(path = %socket_path.display(), "Connected to orchestrator event stream");
                let reader = BufReader::new(stream);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(event) = serde_json::from_str::<IpcEvent>(&line) {
                        match event.channel.as_str() {
                            "app:state" => {
                                if let Ok(ev) = serde_json::from_value::<hr_common::events::AppStateEvent>(event.payload) {
                                    let _ = events.app_state.send(ev);
                                }
                            }
                            "app:build" => {
                                if let Ok(ev) = serde_json::from_value::<hr_common::events::AppBuildEvent>(event.payload) {
                                    let _ = events.app_build.send(ev);
                                }
                            }
                            "app:log" => {
                                // Re-emit as a log_entry on the local bus for the WS handler
                                // The WS handler already listens to log_entry
                                // We create a minimal LogEntry from the event
                                if let Ok(log_data) = serde_json::from_value::<AppLogEvent>(event.payload) {
                                    let entry = hr_common::logging::LogEntry {
                                        id: 0,
                                        timestamp: chrono::Utc::now(),
                                        service: hr_common::logging::LogService::Orchestrator,
                                        level: hr_common::logging::LogLevel::Info,
                                        category: hr_common::logging::LogCategory::System,
                                        message: log_data.message,
                                        data: Some(serde_json::json!({"app_slug": log_data.slug})),
                                        request_id: None,
                                        user_id: None,
                                        source: hr_common::logging::LogSource {
                                            crate_name: "hr-apps".to_string(),
                                            module: "supervisor".to_string(),
                                            function: None,
                                            file: None,
                                            line: None,
                                        },
                                    };
                                    let _ = events.log_entry.send(entry);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                warn!("Event stream disconnected, reconnecting in 3s...");
            }
            Err(e) => {
                warn!(error = %e, "Cannot connect to event stream, retrying in 3s...");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

#[derive(serde::Deserialize)]
struct AppLogEvent {
    slug: String,
    message: String,
}
