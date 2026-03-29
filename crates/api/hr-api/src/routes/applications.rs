//! REST API + WebSocket routes for application management.
//! Most handlers delegate to hr-orchestrator via IPC (OrchestratorClient).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tracing::{error, info, warn};
use hr_registry::protocol::AgentMessage;
use hr_acme::types::WildcardType;
use hr_ipc::NetcoreClient;
use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/{id}/update/fix", post(fix_agent_update))
        .route("/{id}/exec", post(exec_in_container))
        .route("/{id}/update-rules", post(update_rules_single))
        .route("/update-rules", post(update_rules_all))
        .route("/agents/version", get(agent_version))
        .route("/agents/binary", get(agent_binary))
        .route("/agents/certs", get(agent_certs))
        .route("/agents/update", post(trigger_agent_update))
        .route("/agents/update/status", get(get_update_status))
        .route("/agents/ws", get(agent_ws))
}

// ── IPC helpers ─────────────────────────────────────────────────

fn ipc_err_response(e: anyhow::Error) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"success": false, "error": format!("Orchestrator unavailable: {e}")})),
    )
        .into_response()
}

// ── Rules update handlers ────────────────────────────────────

/// POST /api/applications/{id}/update-rules
/// Push updated .claude/rules/ to a specific agent via IPC.
async fn update_rules_single(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateAgentRules {
            app_ids: Some(vec![id.clone()]),
        })
        .await
    {
        Ok(resp) if resp.ok => {
            info!(app_id = id, "Rules update sent via IPC");
            Json(serde_json::json!({"success": true, "message": "Rules update sent"})).into_response()
        }
        Ok(resp) => {
            warn!(app_id = id, "Failed to send rules update: {:?}", resp.error);
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": resp.error.unwrap_or_else(|| "Agent not connected".into())})),
            )
                .into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

/// POST /api/applications/update-rules
/// Push updated .claude/rules/ to ALL connected agents via IPC.
async fn update_rules_all(
    State(state): State<ApiState>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateAgentRules { app_ids: None })
        .await
    {
        Ok(resp) if resp.ok => {
            let d = resp.data.unwrap_or_default();
            let sent = d.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
            let failed = d.get("failed").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = d.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            info!(sent, failed, total, "Bulk rules update complete via IPC");
            Json(serde_json::json!({
                "success": true,
                "sent": sent,
                "failed": failed,
                "total": total,
            }))
            .into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

// ── Agent update handlers ────────────────────────────────────

/// Trigger update to all connected agents (or specific ones) via IPC.
async fn trigger_agent_update(
    State(state): State<ApiState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_ids = req
        .get("agent_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        });

    match state
        .orchestrator
        .request(&OrchestratorRequest::TriggerAgentUpdate { agent_ids })
        .await
    {
        Ok(resp) if resp.ok => {
            let d = resp.data.unwrap_or_default();
            info!("Agent update triggered via API (IPC)");
            Json(serde_json::json!({
                "success": true,
                "version": d.get("version"),
                "sha256": d.get("sha256"),
                "agents_notified": d.get("agents_notified"),
                "agents_skipped": d.get("agents_skipped"),
            }))
            .into_response()
        }
        Ok(resp) => {
            error!("Failed to trigger agent update: {:?}", resp.error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": resp.error})),
            )
                .into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

/// Get update status for all agents via IPC.
async fn get_update_status(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetAgentUpdateStatus)
        .await
    {
        Ok(resp) if resp.ok => {
            let d = resp.data.unwrap_or_default();
            Json(serde_json::json!({
                "success": true,
                "expected_version": d.get("expected_version"),
                "agents": d.get("agents"),
            }))
            .into_response()
        }
        Ok(resp) => {
            error!("Failed to get update status: {:?}", resp.error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": resp.error})),
            )
                .into_response()
        }
        Err(e) => ipc_err_response(e),
    }
}

/// Fix a failed agent update via IPC.
async fn fix_agent_update(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::FixAgentUpdate { app_id: id.clone() })
        .await
    {
        Ok(resp) if resp.ok => {
            info!(app_id = id, "Agent fixed via IPC");
            let output = resp
                .data
                .as_ref()
                .and_then(|d| d.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Json(serde_json::json!({"success": true, "output": output})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.as_deref().unwrap_or("Unknown error");
            if err.contains("not found") || err.contains("Not found") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"success": false, "error": "Application not found"})),
                )
                    .into_response()
            } else {
                error!(app_id = id, "Failed to fix agent: {err}");
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

/// POST /api/applications/{id}/exec -- execute a command in the container via IPC.
async fn exec_in_container(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let commands: Vec<String> = match body.get("command").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success": false, "error": "command (string array) required"})),
            )
                .into_response();
        }
    };

    match state
        .orchestrator
        .request(&OrchestratorRequest::ExecInContainer {
            app_id: id.clone(),
            commands,
        })
        .await
    {
        Ok(resp) if resp.ok => {
            let d = resp.data.unwrap_or_default();
            let success = d.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
            let stdout = d.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
            let stderr = d.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
            if success {
                Json(serde_json::json!({"success": true, "stdout": stdout})).into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"success": false, "stdout": stdout, "stderr": stderr})),
                )
                    .into_response()
            }
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

// ── Agent binary distribution ────────────────────────────────

const AGENT_BINARY_PATH: &str = "/opt/homeroute/data/agent-binaries/hr-agent";

async fn agent_version() -> impl IntoResponse {
    let binary_path = std::path::Path::new(AGENT_BINARY_PATH);
    if !binary_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Agent binary not found"})),
        )
            .into_response();
    }

    // Read binary and compute SHA256
    let bytes = match tokio::fs::read(binary_path).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": e.to_string()})),
            )
                .into_response();
        }
    };

    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let sha256: String = digest.as_ref().iter().map(|b| format!("{:02x}", b)).collect();

    // Version from version file (written by `make agent`)
    let version_path = std::path::Path::new("/opt/homeroute/data/agent-binaries/hr-agent.version");
    let version = if version_path.exists() {
        tokio::fs::read_to_string(version_path)
            .await
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    } else {
        // Fallback to mtime for backwards compatibility
        match tokio::fs::metadata(binary_path).await {
            Ok(m) => {
                if let Ok(modified) = m.modified() {
                    let dt: chrono::DateTime<chrono::Utc> = modified.into();
                    dt.format("%Y%m%d-%H%M%S").to_string()
                } else {
                    "unknown".to_string()
                }
            }
            Err(_) => "unknown".to_string(),
        }
    };

    Json(serde_json::json!({
        "success": true,
        "version": version,
        "sha256": sha256,
        "size": bytes.len()
    }))
    .into_response()
}

async fn agent_binary() -> impl IntoResponse {
    let binary_path = std::path::Path::new(AGENT_BINARY_PATH);
    match tokio::fs::read(binary_path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"hr-agent\"",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Agent binary not found"})),
        )
            .into_response(),
    }
}

// ── Agent certificate distribution ───────────────────────────

/// GET /api/applications/agents/certs
/// Auth via `Authorization: Bearer {agent_token}` header.
/// Returns cert+key PEM for the app wildcard and global wildcard.
async fn agent_certs(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Extract Bearer token
    let token = match headers.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        Some(t) => t.to_string(),
        None => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Missing or invalid Authorization header"}))).into_response();
        }
    };

    // Authenticate by token via IPC to orchestrator (registry lives there)
    let (app_id, slug) = match state.orchestrator.request(
        &hr_ipc::orchestrator::OrchestratorRequest::AuthenticateAgentToken { token },
    ).await {
        Ok(resp) if resp.ok => {
            let data = resp.data.unwrap_or_default();
            let app_id = data["app_id"].as_str().unwrap_or_default().to_string();
            let slug = data["slug"].as_str().unwrap_or_default().to_string();
            if app_id.is_empty() || slug.is_empty() {
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid token"}))).into_response();
            }
            (app_id, slug)
        }
        Ok(_) => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid token"}))).into_response();
        }
        Err(e) => {
            warn!(error = %e, "Failed to authenticate agent token via IPC");
            return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "Orchestrator not available"}))).into_response();
        }
    };

    info!(app_id, slug, "Agent fetching certificates");

    // Per-app wildcard certs removed — only global wildcard is issued.
    let app_cert: Option<serde_json::Value> = None;

    // Get global wildcard cert
    let global_cert = match state.acme.get_cert_pem(WildcardType::Global).await {
        Ok((cert_pem, key_pem)) => {
            let wildcard_domain = WildcardType::Global.domain_pattern(&state.env.base_domain);
            Some(serde_json::json!({
                "cert_pem": cert_pem,
                "key_pem": key_pem,
                "wildcard_domain": wildcard_domain,
            }))
        }
        Err(_) => None,
    };

    Json(serde_json::json!({
        "app_cert": app_cert,
        "global_cert": global_cert,
    })).into_response()
}

// ── DNS record helpers for agent lifecycle ───────────────────

/// Add local DNS A records for an agent via IPC, based on environment:
/// Add DNS A record: `{slug}.{base}` -> IPv4
async fn add_agent_dns_records(
    netcore: &NetcoreClient,
    slug: &str,
    base_domain: &str,
    ipv4: &str,
    environment: hr_registry::types::Environment,
) {
    let (name, record_type) = (format!("{}.{}", slug, base_domain), "A".to_string());
    if let Err(e) = netcore.dns_add_static_record(name, record_type, ipv4.to_string(), 60).await {
        warn!(slug, ipv4, error = %e, "Failed to add DNS record via IPC");
    } else {
        info!(slug, ipv4, ?environment, "Added local DNS A records for agent");
    }
}

/// Remove all local DNS records pointing to a specific IPv4 address via IPC.
pub(crate) async fn remove_agent_dns_records(netcore: &NetcoreClient, ipv4: &str) {
    if let Err(e) = netcore.dns_remove_static_records_by_value(ipv4).await {
        warn!(ipv4, error = %e, "Failed to remove DNS records via IPC");
    } else {
        info!(ipv4, "Removed local DNS records for agent IP");
    }
}

// ── WebSocket handler for agent connections ─────────────────
// NOTE: This handler remains unchanged during the IPC transition.

async fn agent_ws(
    State(state): State<ApiState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_ws(state, socket))
}

async fn handle_agent_ws(state: ApiState, mut socket: WebSocket) {
    let Some(registry) = &state.registry else {
        // Thin shell mode: proxy to hr-orchestrator
        super::ws_proxy::proxy_ws_to_orchestrator(socket, "/agents/ws").await;
        return;
    };
    let registry = registry.clone();

    // Wait for Auth message with a timeout
    let auth_msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;

    let (token, service_name, version, reported_ipv4) = match auth_msg {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<AgentMessage>(&text) {
                Ok(AgentMessage::Auth { token, service_name, version, ipv4_address }) => {
                    (token, service_name, version, ipv4_address)
                }
                _ => {
                    warn!("Agent WS: expected Auth message, got something else");
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
            }
        }
        _ => {
            warn!("Agent WS: auth timeout or connection error");
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    // Authenticate
    let Some(app_id) = registry.authenticate(&token, &service_name).await else {
        let reject = hr_registry::protocol::RegistryMessage::AuthResult {
            success: false,
            error: Some("Invalid credentials".into()),
            app_id: None,
        };
        let _ = socket.send(Message::Text(serde_json::to_string(&reject).unwrap().into())).await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    info!(app_id = app_id, service = service_name, ipv4 = ?reported_ipv4, "Agent authenticated");

    // Create mpsc channel for registry -> agent messages
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    // Notify registry of connection (pushes config, increments active count)
    if let Err(e) = registry.on_agent_connected(&app_id, tx, version, reported_ipv4).await {
        error!(app_id, "Agent provisioning failed: {e}");
        // Decrement the count that was already incremented
        registry.on_agent_disconnected(&app_id).await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    // Routes are now published by the agent via PublishRoutes message.

    // Send auth success
    let success = hr_registry::protocol::RegistryMessage::AuthResult {
        success: true,
        error: None,
        app_id: Some(app_id.clone()),
    };
    if socket.send(Message::Text(serde_json::to_string(&success).unwrap().into())).await.is_err() {
        registry.on_agent_disconnected(&app_id).await;
        return;
    }

    // Bidirectional message loop
    loop {
        tokio::select! {
            // Registry -> Agent
            Some(msg) = rx.recv() => {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            // Agent -> Registry
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<AgentMessage>(&text) {
                            Ok(AgentMessage::Heartbeat { .. }) => {
                                registry.handle_heartbeat(&app_id).await;
                            }
                            Ok(AgentMessage::ConfigAck { .. }) => {
                                // Acknowledged, nothing to do
                            }
                            Ok(AgentMessage::Error { message }) => {
                                warn!(app_id, message, "Agent reported error");
                            }
                            Ok(AgentMessage::Auth { .. }) => {
                                // Duplicate auth, ignore
                            }
                            Ok(AgentMessage::Metrics(m)) => {
                                // Metrics are proof of liveness -- update heartbeat
                                // (restores Connected status after host recovery)
                                registry.handle_heartbeat(&app_id).await;
                                registry.handle_metrics(&app_id, m).await;
                            }
                            Ok(AgentMessage::IpUpdate { ipv4_address }) => {
                                info!(app_id, ipv4_address, "Agent reported IP update");
                                // Remove old DNS records for previous IP
                                if let Some(app) = registry.get_application(&app_id).await {
                                    if let Some(old_ip) = app.ipv4_address {
                                        remove_agent_dns_records(&state.netcore, &old_ip.to_string()).await;
                                    }
                                }
                                registry.handle_ip_update(&app_id, &ipv4_address).await;
                                // Add new DNS records for updated IP
                                if let Some(app) = registry.get_application(&app_id).await {
                                    add_agent_dns_records(&state.netcore, &app.slug, &state.env.base_domain, &ipv4_address, app.environment).await;
                                }
                            }
                            Ok(AgentMessage::PublishRoutes { routes }) => {
                                info!(app_id, count = routes.len(), "Agent published routes");
                                let apps = registry.list_applications().await;
                                if let Some(app) = apps.iter().find(|a| a.id == app_id) {
                                    if let Some(target_ip) = app.ipv4_address {
                                        // Clear old routes for this app via edge IPC
                                        let base_domain = &state.env.base_domain;
                                        for domain in app.domains(base_domain) {
                                            if let Err(e) = state.edge.remove_app_route(&domain).await {
                                                warn!(domain, error = %e, "Failed to remove app route via edge IPC");
                                            }
                                        }
                                        // Set new routes from agent via edge IPC
                                        for route in &routes {
                                            if let Err(e) = state.edge.set_app_route(
                                                route.domain.clone(),
                                                app.id.clone(),
                                                app.host_id.clone(),
                                                target_ip.to_string(),
                                                route.target_port,
                                                route.auth_required,
                                                route.allowed_groups.clone(),
                                                app.frontend.local_only,
                                            ).await {
                                                warn!(domain = route.domain, error = %e, "Failed to set app route via edge IPC");
                                            }
                                        }
                                        // Add local DNS A records for direct local access
                                        let ip_str = target_ip.to_string();
                                        add_agent_dns_records(&state.netcore, &app.slug, base_domain, &ip_str, app.environment).await;
                                    }
                                }
                            }
                            Ok(AgentMessage::UpdateScanResult {
                                os_upgradable, os_security,
                                claude_cli_installed, claude_cli_latest,
                                code_server_installed, code_server_latest,
                                claude_ext_installed, claude_ext_latest,
                                scan_error,
                            }) => {
                                info!(app_id, os_upgradable, os_security, "Agent update scan result");
                                if let Some(app) = registry.get_application(&app_id).await {
                                    // Read latest agent version from version file
                                    let agent_version_latest = std::fs::read_to_string(
                                        "/opt/homeroute/data/agent-binaries/hr-agent.version"
                                    ).ok().map(|s| s.trim().to_string());

                                    let target = hr_common::events::UpdateTarget {
                                        id: app_id.clone(),
                                        name: app.name.clone(),
                                        target_type: "container".to_string(),
                                        environment: Some(format!("{:?}", app.environment).to_lowercase()),
                                        online: true,
                                        os_upgradable,
                                        os_security,
                                        agent_version: app.agent_version.clone(),
                                        agent_version_latest,
                                        claude_cli_installed,
                                        claude_cli_latest,
                                        code_server_installed,
                                        code_server_latest,
                                        claude_ext_installed,
                                        claude_ext_latest,
                                        scan_error,
                                        scanned_at: chrono::Utc::now().to_rfc3339(),
                                    };
                                    registry.scan_results.write().await.insert(app_id.clone(), target.clone());
                                    let _ = state.events.update_scan.send(
                                        hr_common::events::UpdateScanEvent::TargetScanned {
                                            scan_id: String::new(),
                                            target,
                                        }
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(app_id, "Invalid agent message: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Decrement connection count. Only remove routes when the LAST connection closes.
    let is_last = registry.on_agent_disconnected(&app_id).await;
    if is_last {
        let apps = registry.list_applications().await;
        if let Some(app) = apps.iter().find(|a| a.id == app_id) {
            let base_domain = &state.env.base_domain;
            for domain in app.domains(base_domain) {
                if let Err(e) = state.edge.remove_app_route(&domain).await {
                    warn!(domain, error = %e, "Failed to remove app route via edge IPC on disconnect");
                }
            }
            // Remove local DNS A records for this agent
            if let Some(ip) = app.ipv4_address {
                remove_agent_dns_records(&state.netcore, &ip.to_string()).await;
            }
        }
        info!(app_id, "Agent WebSocket closed (last connection, routes + DNS removed)");
    } else {
        info!(app_id, "Agent WebSocket closed (other connections still active, routes preserved)");
    }
}
