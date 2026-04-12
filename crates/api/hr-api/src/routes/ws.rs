use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new().route("/ws", get(ws_handler))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<ApiState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: ApiState) {
    debug!("WebSocket client connected");

    let mut host_rx = state.events.host_status.subscribe();
    let mut updates_rx = state.events.updates.subscribe();
    let mut agent_rx = state.events.agent_status.subscribe();
    let mut metrics_rx = state.events.agent_metrics.subscribe();
    let mut agent_update_rx = state.events.agent_update.subscribe();
    let mut host_metrics_rx = state.events.host_metrics.subscribe();
    let mut host_power_rx = state.events.host_power.subscribe();
    let mut update_scan_rx = state.events.update_scan.subscribe();
    let mut backup_live_rx = state.events.backup_live.subscribe();
    let mut task_update_rx = state.events.task_update.subscribe();
    let mut energy_metrics_rx = state.events.energy_metrics.subscribe();
    let mut log_rx = state.events.log_entry.subscribe();
    let mut app_state_rx = state.events.app_state.subscribe();

    loop {
        tokio::select! {
            // Host status events (new)
            result = host_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "hosts:status",
                            "data": {
                                "hostId": event.host_id,
                                "online": event.status == "online",
                                "status": event.status,
                                "latency": event.latency_ms.unwrap_or(0),
                                "lastSeen": chrono::Utc::now().to_rfc3339()
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket host_status lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Host metrics events
            result = host_metrics_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "hosts:metrics",
                            "data": {
                                "hostId": event.host_id,
                                "cpuPercent": event.cpu_percent,
                                "memoryUsedBytes": event.memory_used_bytes,
                                "memoryTotalBytes": event.memory_total_bytes,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket host_metrics lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Host power state events (WOL, shutdown, reboot transitions)
            result = host_power_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "hosts:power",
                            "data": {
                                "hostId": event.host_id,
                                "state": event.state,
                                "message": event.message,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket host_power lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Update events
            result = updates_rx.recv() => {
                match result {
                    Ok(event) => {
                        use hr_common::events::UpdateEvent;
                        let msg = match event {
                            UpdateEvent::Started => json!({"type": "updates:started"}),
                            UpdateEvent::Phase { phase, message } => json!({"type": "updates:phase", "data": {"phase": phase, "message": message}}),
                            UpdateEvent::Output { line } => json!({"type": "updates:output", "data": {"line": line}}),
                            UpdateEvent::AptComplete { packages, security_count } => json!({"type": "updates:apt-complete", "data": {"packages": packages, "securityCount": security_count}}),
                            UpdateEvent::SnapComplete { snaps } => json!({"type": "updates:snap-complete", "data": {"snaps": snaps}}),
                            UpdateEvent::NeedrestartComplete(data) => json!({"type": "updates:needrestart-complete", "data": data}),
                            UpdateEvent::Complete { success, summary, duration } => json!({"type": "updates:complete", "data": {"success": success, "summary": summary, "duration": duration}}),
                            UpdateEvent::Cancelled => json!({"type": "updates:cancelled"}),
                            UpdateEvent::Error { error } => json!({"type": "updates:error", "data": {"error": error}}),
                            UpdateEvent::UpgradeStarted { upgrade_type } => json!({"type": "updates:upgrade-started", "data": {"type": upgrade_type}}),
                            UpdateEvent::UpgradeOutput { line } => json!({"type": "updates:upgrade-output", "data": {"line": line}}),
                            UpdateEvent::UpgradeComplete { upgrade_type, success, duration, error } => json!({"type": "updates:upgrade-complete", "data": {"type": upgrade_type, "success": success, "duration": duration, "error": error}}),
                            UpdateEvent::UpgradeCancelled => json!({"type": "updates:upgrade-cancelled"}),
                        };
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket updates lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Agent status events
            result = agent_rx.recv() => {
                match result {
                    Ok(event) => {
                        let mut data = json!({
                            "appId": event.app_id,
                            "slug": event.slug,
                            "status": event.status
                        });
                        if let Some(message) = &event.message {
                            data["message"] = json!(message);
                        }
                        let msg = json!({
                            "type": "agent:status",
                            "data": data
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket agent_status lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Agent metrics events
            result = metrics_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "agent:metrics",
                            "data": {
                                "appId": event.app_id,
                                "memoryBytes": event.memory_bytes,
                                "cpuPercent": event.cpu_percent,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket agent_metrics lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Agent update events
            result = agent_update_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "agent:update",
                            "data": {
                                "appId": event.app_id,
                                "slug": event.slug,
                                "status": format!("{:?}", event.status).to_lowercase(),
                                "version": event.version,
                                "error": event.error,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket agent_update lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Backup live events
            result = backup_live_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "backup:live",
                            "data": {
                                "status": event.status,
                                "progress": event.progress,
                                "repos": event.repos,
                                "latestJob": event.latest_job,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket backup_live lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Unified update scan events
            result = update_scan_rx.recv() => {
                match result {
                    Ok(event) => {
                        use hr_common::events::UpdateScanEvent;
                        let msg = match &event {
                            UpdateScanEvent::ScanStarted { scan_id } => json!({
                                "type": "updates:scan:started",
                                "data": { "scanId": scan_id }
                            }),
                            UpdateScanEvent::TargetScanned { scan_id, target } => json!({
                                "type": "updates:scan:target",
                                "data": { "scanId": scan_id, "target": target }
                            }),
                            UpdateScanEvent::ScanComplete { scan_id } => json!({
                                "type": "updates:scan:complete",
                                "data": { "scanId": scan_id }
                            }),
                            UpdateScanEvent::UpgradeStarted { target_id, category } => json!({
                                "type": "updates:upgrade-target:started",
                                "data": { "targetId": target_id, "category": category }
                            }),
                            UpdateScanEvent::UpgradeOutput { target_id, line } => json!({
                                "type": "updates:upgrade-target:output",
                                "data": { "targetId": target_id, "line": line }
                            }),
                            UpdateScanEvent::UpgradeComplete { target_id, category, success, error } => json!({
                                "type": "updates:upgrade-target:complete",
                                "data": { "targetId": target_id, "category": category, "success": success, "error": error }
                            }),
                        };
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket update_scan lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Task update events
            result = task_update_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "task:update",
                            "data": {
                                "task": event.task,
                                "steps": event.steps,
                            }
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket task_update lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Energy metrics events
            result = energy_metrics_rx.recv() => {
                match result {
                    Ok(event) => {
                        let per_core: Option<Vec<serde_json::Value>> = event.per_core.as_ref().map(|cores| {
                            cores.iter().map(|c| json!({
                                "coreId": c.core_id,
                                "frequencyMhz": c.frequency_mhz,
                                "governor": c.governor,
                                "minFreqMhz": c.min_freq_mhz,
                                "maxFreqMhz": c.max_freq_mhz,
                            })).collect()
                        });
                        let mut data = json!({
                            "hostId": event.host_id,
                            "hostName": event.host_name,
                            "online": event.online,
                            "temperature": event.temperature,
                            "cpuPercent": event.cpu_percent,
                            "frequencyGhz": event.frequency_ghz,
                            "frequencyMinGhz": event.frequency_min_ghz,
                            "frequencyMaxGhz": event.frequency_max_ghz,
                            "governor": event.governor,
                            "mode": event.mode,
                            "cores": event.cores,
                            "model": event.model,
                        });
                        if let Some(pc) = per_core {
                            data["perCore"] = json!(pc);
                        }
                        let msg = json!({
                            "type": "energy:metrics",
                            "data": data
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket energy_metrics lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Log entry events (live log viewer)
            result = log_rx.recv() => {
                match result {
                    Ok(entry) => {
                        let msg = json!({
                            "type": "log:entry",
                            "data": entry
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket log_entry lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // App state change events (real-time from supervisor)
            result = app_state_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = json!({
                            "type": "app:state",
                            "data": event
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket app_state lagged by {}", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Client disconnect
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {} // Ignore other messages
                }
            }
        }
    }

    debug!("WebSocket client disconnected");
}
