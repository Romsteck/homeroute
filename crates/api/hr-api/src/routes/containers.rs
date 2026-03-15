//! REST API + WebSocket routes for Containers V2 (systemd-nspawn).
//! Handlers delegate to hr-orchestrator via IPC (OrchestratorClient).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{error, info};

use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_containers).post(create_container))
        .route("/{id}", put(update_container).delete(delete_container))
        .route("/{id}/start", post(start_container))
        .route("/{id}/stop", post(stop_container))
        .route("/{id}/volumes", get(list_volumes).post(attach_volume))
        .route("/{id}/volumes/{vol_id}", put(update_volume).delete(detach_volume))
        .route("/{id}/terminal", get(terminal_ws))
        .route("/{id}/migrate", post(migrate_container))
        .route("/{id}/migrate/status", get(migration_status))
        .route("/{id}/migrate/cancel", post(cancel_migration))
        .route("/{id}/rename", post(rename_container))
        .route("/{id}/rename/status", get(rename_status))
        .route("/config", get(get_config).put(update_config))
}

// ── Helpers ─────────────────────────────────────────────────────

/// Map an IPC Ok response to an axum response. If resp.ok, return the success JSON;
/// otherwise return 500 with the error.
fn ipc_ok_response(resp: hr_ipc::types::IpcResponse) -> axum::response::Response {
    if resp.ok {
        Json(serde_json::json!({"success": true, "data": resp.data})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response()
    }
}

fn ipc_err_response(e: anyhow::Error) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"success": false, "error": format!("Orchestrator unavailable: {e}")})),
    )
        .into_response()
}

// ── CRUD handlers ────────────────────────────────────────────────

async fn list_containers(State(state): State<ApiState>) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::ListContainers).await {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "containers": resp.data})).into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

async fn create_container(
    State(state): State<ApiState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::CreateContainer { request: req })
        .await
    {
        Ok(resp) if resp.ok => {
            info!("Container V2 created via API (IPC)");
            Json(serde_json::json!({
                "success": true,
                "container": resp.data.as_ref().and_then(|d| d.get("container")),
                "token": resp.data.as_ref().and_then(|d| d.get("token")),
            }))
            .into_response()
        }
        Ok(resp) => {
            error!("Failed to create container V2: {:?}", resp.error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": resp.error})),
            )
                .into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn delete_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Clean up proxy routes and local DNS records BEFORE deleting the container.
    // Use IPC ListApplications to find the app info instead of state.registry.
    if let Ok(resp) = state
        .orchestrator
        .request(&OrchestratorRequest::ListApplications)
        .await
    {
        if resp.ok {
            if let Some(apps) = resp.data.as_ref().and_then(|d| d.as_array()) {
                if let Some(app) = apps.iter().find(|a| a.get("id").and_then(|v| v.as_str()) == Some(&id)) {
                    let base_domain = &state.env.base_domain;
                    // Collect domains from the app object
                    let slug = app.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                    let env_str = app.get("environment").and_then(|v| v.as_str()).unwrap_or("");
                    let domains = match env_str {
                        "development" => vec![
                            format!("dev.{}.{}", slug, base_domain),
                            format!("code.{}.{}", slug, base_domain),
                        ],
                        "production" => vec![format!("{}.{}", slug, base_domain)],
                        _ => vec![],
                    };
                    for domain in &domains {
                        if let Err(e) = state.edge.remove_app_route(domain).await {
                            tracing::warn!(domain, error = %e, "Failed to remove app route via edge IPC on container delete");
                        }
                    }
                    if let Some(ip) = app.get("ipv4_address").and_then(|v| v.as_str()) {
                        super::applications::remove_agent_dns_records(&state.netcore, ip).await;
                    }
                }
            }
        }
    }

    match state
        .orchestrator
        .request(&OrchestratorRequest::DeleteContainer { id: id.clone() })
        .await
    {
        Ok(resp) if resp.ok => Json(serde_json::json!({"success": true})).into_response(),
        Ok(resp) => {
            // Check if orchestrator reported "not found"
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") || err.contains("Not found") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"success": false, "error": "Not found"})),
                )
                    .into_response()
            } else {
                error!("Failed to delete container V2: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"success": false, "error": err})),
                )
                    .into_response()
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn update_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateContainer {
            id: id.clone(),
            request: req,
        })
        .await
    {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

async fn start_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::StartContainer { id: id.clone() })
        .await
    {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

async fn stop_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::StopContainer { id: id.clone() })
        .await
    {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

// ── Config handlers ──────────────────────────────────────────────

async fn get_config(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetContainerConfig)
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "config": resp.data})).into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateContainerConfig { config })
        .await
    {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

// ── Volume handlers ─────────────────────────────────────────────

async fn list_volumes(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::ListVolumes { container_id: id })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "volumes": resp.data})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") {
                (StatusCode::NOT_FOUND, Json(serde_json::json!({"success": false, "error": err}))).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "error": err}))).into_response()
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn attach_volume(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(volume): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::AttachVolume {
            container_id: id,
            volume,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "volume": resp.data})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            let status = if err.contains("not found") { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            (status, Json(serde_json::json!({"success": false, "error": err}))).into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn update_volume(
    State(state): State<ApiState>,
    Path((id, vol_id)): Path<(String, String)>,
    Json(updates): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateVolume {
            container_id: id,
            volume_id: vol_id,
            updates,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "volume": resp.data})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            let status = if err.contains("not found") { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            (status, Json(serde_json::json!({"success": false, "error": err}))).into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn detach_volume(
    State(state): State<ApiState>,
    Path((id, vol_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::DetachVolume {
            container_id: id,
            volume_id: vol_id,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            let status = if err.contains("not found") { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            (status, Json(serde_json::json!({"success": false, "error": err}))).into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Migration handlers ───────────────────────────────────────────

async fn migrate_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let target_host_id = match req.get("target_host_id").and_then(|v| v.as_str()) {
        Some(h) => h.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "target_host_id required"})),
            )
                .into_response()
        }
    };

    match state
        .orchestrator
        .request_long(&OrchestratorRequest::MigrateContainer {
            id: id.clone(),
            target_host_id,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({
                "transfer_id": resp.data.as_ref().and_then(|d| d.get("transfer_id")),
                "status": "started"
            }))
            .into_response()
        }
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

async fn migration_status(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetMigrationStatus { app_id: id.clone() })
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or_default()).into_response(),
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") || err.contains("No migration") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "No migration found"})),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": err})),
                )
                    .into_response()
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

async fn cancel_migration(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::CancelMigration { app_id: id.clone() })
        .await
    {
        Ok(resp) if resp.ok => {
            let msg = resp
                .data
                .as_ref()
                .and_then(|d| d.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Migration cancellation requested");
            Json(serde_json::json!({"success": true, "message": msg})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") || err.contains("No migration") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "No migration found"})),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": err})),
                )
                    .into_response()
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Rename handlers ─────────────────────────────────────────────

async fn rename_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::RenameContainer {
            id: id.clone(),
            request: req,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({
                "success": true,
                "rename_id": resp.data.as_ref().and_then(|d| d.get("rename_id")),
                "status": "in_progress"
            }))
            .into_response()
        }
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

async fn rename_status(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetRenameStatus { app_id: id.clone() })
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or_default()).into_response(),
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") || err.contains("No rename") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "No rename found"})),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": err})),
                )
                    .into_response()
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Terminal WebSocket (machinectl shell) ─────────────────────────
// NOTE: This handler remains unchanged during the IPC transition.
// It still uses state.container_manager and state.registry directly.

async fn terminal_ws(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal_ws(state, id, socket))
}

async fn handle_terminal_ws(state: ApiState, container_id: String, mut socket: WebSocket) {
    let Some(ref mgr) = state.container_manager else {
        // Thin shell mode: proxy to hr-orchestrator
        let path = format!("/containers/{container_id}/terminal");
        super::ws_proxy::proxy_ws_to_orchestrator(socket, &path).await;
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
            // Data from host-agent (terminal output) -> WebSocket client
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
            // Data from WebSocket client (user input) -> host-agent
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
