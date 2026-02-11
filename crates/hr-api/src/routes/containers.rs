//! REST API + WebSocket routes for Containers V2 (systemd-nspawn).

use std::sync::atomic::Ordering;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{error, info};

use hr_common::events::MigrationPhase;

use crate::container_manager::{
    ContainerV2Config, CreateContainerRequest, MigrateContainerRequest, RenameContainerRequest,
    UpdateContainerRequest,
};
use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_containers).post(create_container))
        .route("/{id}", put(update_container).delete(delete_container))
        .route("/{id}/start", post(start_container))
        .route("/{id}/stop", post(stop_container))
        .route("/{id}/terminal", get(terminal_ws))
        .route("/{id}/migrate", post(migrate_container))
        .route("/{id}/migrate/status", get(migration_status))
        .route("/{id}/migrate/cancel", post(cancel_migration))
        .route("/{id}/rename", post(rename_container))
        .route("/{id}/rename/status", get(rename_status))
        .route("/config", get(get_config).put(update_config))
}

// ── CRUD handlers ────────────────────────────────────────────────

async fn list_containers(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };
    let containers = mgr.list_containers().await;
    Json(serde_json::json!({"success": true, "containers": containers})).into_response()
}

async fn create_container(
    State(state): State<ApiState>,
    Json(req): Json<CreateContainerRequest>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.create_container(req).await {
        Ok((record, token)) => {
            info!(slug = record.slug, "Container V2 created via API");
            Json(serde_json::json!({
                "success": true,
                "container": record,
                "token": token
            }))
            .into_response()
        }
        Err(e) => {
            error!("Failed to create container V2: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

async fn delete_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.remove_container(&id).await {
        Ok(true) => Json(serde_json::json!({"success": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Not found"})),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to delete container V2: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

async fn update_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateContainerRequest>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.update_container(&id, req).await {
        Ok(true) => Json(serde_json::json!({"success": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Not found"})),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to update container V2: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

async fn start_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.start_container(&id).await {
        Ok(true) => Json(serde_json::json!({"success": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        )
            .into_response(),
    }
}

async fn stop_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.stop_container(&id).await {
        Ok(true) => Json(serde_json::json!({"success": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        )
            .into_response(),
    }
}

// ── Config handlers ──────────────────────────────────────────────

async fn get_config(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    let config = mgr.get_config().await;
    Json(serde_json::json!({"success": true, "config": config})).into_response()
}

async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<ContainerV2Config>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.update_config(config).await {
        Ok(()) => Json(serde_json::json!({"success": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        )
            .into_response(),
    }
}

// ── Migration handlers ───────────────────────────────────────────

async fn migrate_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<MigrateContainerRequest>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr
        .migrate_container(&id, &req.target_host_id, &state.migrations)
        .await
    {
        Ok(transfer_id) => {
            Json(serde_json::json!({"transfer_id": transfer_id, "status": "started"}))
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn migration_status(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let migrations = state.migrations.read().await;

    let migration = migrations
        .values()
        .filter(|m| m.app_id == id)
        .max_by_key(|m| m.started_at);

    match migration {
        Some(m) => Json(serde_json::json!({
            "transfer_id": m.transfer_id,
            "phase": m.phase,
            "progress_pct": m.progress_pct,
            "bytes_transferred": m.bytes_transferred,
            "total_bytes": m.total_bytes,
            "source_host_id": m.source_host_id,
            "target_host_id": m.target_host_id,
            "error": m.error,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No migration found"})),
        )
            .into_response(),
    }
}

async fn cancel_migration(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let migrations = state.migrations.read().await;

    let migration = migrations.values().find(|m| {
        m.app_id == id
            && !matches!(
                m.phase,
                MigrationPhase::Complete | MigrationPhase::Failed
            )
    });

    match migration {
        Some(m) => {
            if m.cancelled.load(Ordering::SeqCst) {
                return Json(
                    serde_json::json!({"success": true, "message": "Migration already being cancelled"}),
                )
                .into_response();
            }
            m.cancelled.store(true, Ordering::SeqCst);
            info!(app_id = %id, transfer_id = %m.transfer_id, "Container V2 migration cancel requested");
            Json(
                serde_json::json!({"success": true, "message": "Migration cancellation requested"}),
            )
            .into_response()
        }
        None => {
            let has_any = migrations.values().any(|m| m.app_id == id);
            if has_any {
                Json(
                    serde_json::json!({"success": true, "message": "No active migration to cancel"}),
                )
                .into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "No migration found"})),
                )
                    .into_response()
            }
        }
    }
}

// ── Rename handlers ─────────────────────────────────────────────

async fn rename_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<RenameContainerRequest>,
) -> impl IntoResponse {
    let Some(ref mgr) = state.container_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "error": "Container manager not available"})),
        )
            .into_response();
    };

    match mgr.rename_container(&id, req, &state.renames).await {
        Ok(rename_id) => Json(serde_json::json!({
            "success": true,
            "rename_id": rename_id,
            "status": "in_progress"
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": e})),
        )
            .into_response(),
    }
}

async fn rename_status(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let renames = state.renames.read().await;

    // Find a rename that involves this app_id
    let rename = renames
        .values()
        .filter(|r| r.app_ids.contains(&id))
        .max_by_key(|r| r.started_at);

    match rename {
        Some(r) => Json(serde_json::json!({
            "rename_id": r.rename_id,
            "old_slug": r.old_slug,
            "new_slug": r.new_slug,
            "phase": r.phase,
            "started_at": r.started_at,
            "error": r.error,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No rename found"})),
        )
            .into_response(),
    }
}

// ── Terminal WebSocket (machinectl shell) ─────────────────────────

async fn terminal_ws(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal_ws(state, id, socket))
}

async fn handle_terminal_ws(state: ApiState, container_id: String, mut socket: WebSocket) {
    let Some(ref mgr) = state.container_manager else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    // Look up the container record to get the container name and host_id
    let containers = mgr.list_containers().await;
    let container_record = containers
        .iter()
        .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(&container_id));

    let Some(record) = container_record else {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"error": "Container not found"})
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    let container = record
        .get("container_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let host_id = record
        .get("host_id")
        .and_then(|v| v.as_str())
        .unwrap_or("local")
        .to_string();

    if container.is_empty() {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"error": "Container not found"})
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    info!(container, host_id, "Container V2 terminal WebSocket opened");

    if host_id == "local" {
        handle_terminal_local(&container, &mut socket).await;
    } else {
        handle_terminal_remote(&state, &host_id, &container, &mut socket).await;
    }

    let _ = socket.send(Message::Close(None)).await;
    info!(container, "Container V2 terminal WebSocket closed");
}

/// Handle terminal for a local container (machinectl + nsenter).
async fn handle_terminal_local(container: &str, socket: &mut WebSocket) {
    // Get the container's leader PID via machinectl show
    let leader_pid = match Command::new("machinectl")
        .args(["show", container, "--property=Leader", "--value"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(container, "Failed to get leader PID: {stderr}");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"error": format!("Failed to get container PID: {stderr}")})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
        Err(e) => {
            error!(container, "Failed to run machinectl show: {e}");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"error": format!("Failed to get container PID: {e}")})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    // Use script to allocate a PTY for nsenter+bash (interactive shell with echo/prompts)
    let nsenter_cmd = format!(
        "nsenter -t {} -m -u -i -n -p -- /bin/bash -l",
        leader_pid
    );
    let mut child = match Command::new("script")
        .args(["-qfec", &nsenter_cmd, "/dev/null"])
        .env("TERM", "xterm-256color")
        .env("HOME", "/root")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            error!(container, "Failed to spawn nsenter shell: {e}");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"error": format!("Failed to start shell: {e}")})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let mut stdout_buf = vec![0u8; 4096];
    let mut stderr_buf = vec![0u8; 4096];

    loop {
        tokio::select! {
            n = stdout.read(&mut stdout_buf) => {
                match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if socket.send(Message::Binary(stdout_buf[..n].to_vec().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            n = stderr.read(&mut stderr_buf) => {
                match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if socket.send(Message::Binary(stderr_buf[..n].to_vec().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        if stdin.write_all(text.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if stdin.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            status = child.wait() => {
                match status {
                    Ok(s) => info!(container, status = ?s, "Shell process exited"),
                    Err(e) => error!(container, "Shell process error: {e}"),
                }
                break;
            }
        }
    }

    let _ = child.kill().await;
}

/// Handle terminal for a remote container (proxied through host-agent WebSocket).
async fn handle_terminal_remote(
    state: &ApiState,
    host_id: &str,
    container: &str,
    socket: &mut WebSocket,
) {
    let registry = match &state.registry {
        Some(r) => r,
        None => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"error": "Registry not available"})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    if !registry.is_host_connected(host_id).await {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"error": "Host is not connected"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);

    // Register session so data from host-agent is routed to us
    registry.register_terminal_session(&session_id, tx).await;

    // Send TerminalOpen to host-agent
    if let Err(e) = registry
        .send_host_command(
            host_id,
            hr_registry::protocol::HostRegistryMessage::TerminalOpen {
                session_id: session_id.clone(),
                container_name: container.to_string(),
            },
        )
        .await
    {
        error!(container, host_id, "Failed to send TerminalOpen: {e}");
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"error": format!("Failed to open remote terminal: {e}")})
                    .to_string()
                    .into(),
            ))
            .await;
        registry.unregister_terminal_session(&session_id).await;
        return;
    }

    info!(container, host_id, session_id, "Remote terminal session started");

    // Bidirectional relay loop
    loop {
        tokio::select! {
            // Data from host-agent (terminal output) → WebSocket client
            data = rx.recv() => {
                match data {
                    Some(d) if d.is_empty() => {
                        // Empty data signals session closed by host-agent
                        break;
                    }
                    Some(d) => {
                        if socket.send(Message::Binary(d.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break, // Channel closed
                }
            }
            // Data from WebSocket client (user input) → host-agent
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        let _ = registry.send_host_command(
                            host_id,
                            hr_registry::protocol::HostRegistryMessage::TerminalData {
                                session_id: session_id.clone(),
                                data: text.as_bytes().to_vec(),
                            },
                        ).await;
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let _ = registry.send_host_command(
                            host_id,
                            hr_registry::protocol::HostRegistryMessage::TerminalData {
                                session_id: session_id.clone(),
                                data: data.to_vec(),
                            },
                        ).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Cleanup: send TerminalClose and unregister
    let _ = registry
        .send_host_command(
            host_id,
            hr_registry::protocol::HostRegistryMessage::TerminalClose {
                session_id: session_id.clone(),
            },
        )
        .await;
    registry.unregister_terminal_session(&session_id).await;

    info!(container, host_id, session_id, "Remote terminal session ended");
}
