//! WebSocket endpoints for hr-orchestrator (port 4001).
//!
//! These handlers manage agent, host-agent, and terminal WebSocket connections
//! directly against the local AgentRegistry, without going through hr-api.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_ipc::{EdgeClient, NetcoreClient};
use hr_registry::protocol::{AgentMessage, HostAgentMessage, HostRegistryMessage, RegistryMessage};
use hr_registry::AgentRegistry;

use crate::container_manager::ContainerManager;

// ── Transfer types for host-agent migration relay ────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum TransferPhase {
    ReceivingContainer,
    ReceivingWorkspace,
}

struct ActiveTransfer {
    container_name: String,
    storage_path: String,
    network_mode: String,
    file: tokio::fs::File,
    phase: TransferPhase,
    workspace_file: Option<tokio::fs::File>,
    total_bytes: u64,
    bytes_received: u64,
    chunk_count: u32,
    #[allow(dead_code)]
    app_id: String,
    #[allow(dead_code)]
    transfer_id: String,
}

// ── Shared state for WS routes ──────────────────────────────────────

#[derive(Clone)]
pub struct WsState {
    pub registry: Arc<AgentRegistry>,
    pub container_manager: Arc<ContainerManager>,
    pub env: Arc<EnvConfig>,
    pub events: Arc<EventBus>,
    pub edge: Arc<EdgeClient>,
    pub netcore: Arc<NetcoreClient>,
}

// ── Constants ────────────────────────────────────────────────────────

const HOSTS_FILE: &str = "/data/hosts.json";

// ══════════════════════════════════════════════════════════════════════
// Agent WebSocket (/agents/ws)
// ══════════════════════════════════════════════════════════════════════

pub async fn agent_ws(
    State(state): State<WsState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_ws(state, socket))
}

async fn handle_agent_ws(state: WsState, mut socket: WebSocket) {
    let registry = state.registry.clone();

    // Wait for Auth message with a timeout
    let auth_msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;

    let (token, service_name, version, reported_ipv4) = match auth_msg {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<AgentMessage>(&text) {
                Ok(AgentMessage::Auth {
                    token,
                    service_name,
                    version,
                    ipv4_address,
                }) => (token, service_name, version, ipv4_address),
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
        let reject = RegistryMessage::AuthResult {
            success: false,
            error: Some("Invalid credentials".into()),
            app_id: None,
        };
        let _ = socket
            .send(Message::Text(
                serde_json::to_string(&reject).unwrap().into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    info!(
        app_id = app_id,
        service = service_name,
        ipv4 = ?reported_ipv4,
        "Agent authenticated"
    );

    // Create mpsc channel for registry -> agent messages
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    // Notify registry of connection (pushes config, increments active count)
    if let Err(e) = registry
        .on_agent_connected(&app_id, tx, version, reported_ipv4)
        .await
    {
        error!(app_id, "Agent provisioning failed: {e}");
        registry.on_agent_disconnected(&app_id).await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    // Send auth success
    let success = RegistryMessage::AuthResult {
        success: true,
        error: None,
        app_id: Some(app_id.clone()),
    };
    if socket
        .send(Message::Text(
            serde_json::to_string(&success).unwrap().into(),
        ))
        .await
        .is_err()
    {
        registry.on_agent_disconnected(&app_id).await;
        return;
    }

    // Bidirectional message loop
    loop {
        tokio::select! {
            // Registry -> Agent
            Some(msg) = rx.recv() => {
                // Log non-trivial messages being forwarded to agent
                match &msg {
                    RegistryMessage::RunUpgrade { category } => {
                        info!(app_id, category, "Forwarding RunUpgrade to agent WebSocket");
                    }
                    RegistryMessage::RunUpdateScan => {
                        info!(app_id, "Forwarding RunUpdateScan to agent WebSocket");
                    }
                    _ => {}
                }
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    warn!(app_id, "Failed to forward message to agent WebSocket (send error)");
                    break;
                }
            }
            // Agent -> Registry
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_agent_message(&state, &registry, &app_id, &text).await;
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
                    warn!(
                        domain,
                        error = %e,
                        "Failed to remove app route via edge IPC on disconnect"
                    );
                }
            }
            // Remove local DNS A records for this agent
            if let Some(ip) = app.ipv4_address {
                remove_agent_dns_records(&state.netcore, &ip.to_string()).await;
            }
        }
        info!(
            app_id,
            "Agent WebSocket closed (last connection, routes + DNS removed)"
        );
    } else {
        info!(
            app_id,
            "Agent WebSocket closed (other connections still active, routes preserved)"
        );
    }
}

/// Process a single agent message (extracted for readability).
async fn handle_agent_message(
    state: &WsState,
    registry: &Arc<AgentRegistry>,
    app_id: &str,
    text: &str,
) {
    match serde_json::from_str::<AgentMessage>(text) {
        Ok(AgentMessage::Heartbeat { .. }) => {
            registry.handle_heartbeat(app_id).await;
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
            // (restores Connected status after host suspend/resume)
            registry.handle_heartbeat(app_id).await;
            registry.handle_metrics(app_id, m).await;
        }
        Ok(AgentMessage::SchemaMetadata {
            tables,
            relations,
            version,
            db_size_bytes,
        }) => {
            info!(
                app_id,
                tables = tables.len(),
                version,
                "Agent reported schema metadata"
            );
            registry
                .handle_schema_metadata(app_id, tables, relations, version, db_size_bytes)
                .await;
        }
        Ok(AgentMessage::DataverseQueryResult {
            request_id,
            data,
            error,
        }) => {
            registry
                .on_dataverse_query_result(&request_id, data, error)
                .await;
        }
        Ok(AgentMessage::GetDataverseSchemas { request_id }) => {
            // Schema cache lives in hr-api's ApiState. In hr-orchestrator we
            // return an empty list; agents will still get schemas via hr-api
            // until the migration is complete.
            let _ = registry
                .send_to_agent(
                    app_id,
                    RegistryMessage::DataverseSchemas {
                        request_id,
                        schemas: vec![],
                    },
                )
                .await;
        }
        Ok(AgentMessage::IpUpdate { ipv4_address }) => {
            info!(app_id, ipv4_address, "Agent reported IP update");
            // Remove old DNS records for previous IP
            if let Some(app) = registry.get_application(app_id).await {
                if let Some(old_ip) = app.ipv4_address {
                    remove_agent_dns_records(&state.netcore, &old_ip.to_string()).await;
                }
            }
            registry.handle_ip_update(app_id, &ipv4_address).await;
            // Add new DNS records for updated IP
            if let Some(app) = registry.get_application(app_id).await {
                add_agent_dns_records(
                    &state.netcore,
                    &app.slug,
                    &state.env.base_domain,
                    &ipv4_address,
                    app.environment,
                )
                .await;
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
                            warn!(
                                domain,
                                error = %e,
                                "Failed to remove app route via edge IPC"
                            );
                        }
                    }
                    // Set new routes from agent via edge IPC
                    for route in &routes {
                        if let Err(e) = state
                            .edge
                            .set_app_route(
                                route.domain.clone(),
                                app.id.clone(),
                                app.host_id.clone(),
                                target_ip.to_string(),
                                route.target_port,
                                route.auth_required,
                                route.allowed_groups.clone(),
                                app.frontend.local_only,
                            )
                            .await
                        {
                            warn!(
                                domain = route.domain,
                                error = %e,
                                "Failed to set app route via edge IPC"
                            );
                        }
                    }
                    // Add local DNS A records for direct local access
                    let ip_str = target_ip.to_string();
                    add_agent_dns_records(
                        &state.netcore,
                        &app.slug,
                        base_domain,
                        &ip_str,
                        app.environment,
                    )
                    .await;
                }
            }
        }
        Ok(AgentMessage::UpdateScanResult {
            os_upgradable,
            os_security,
            claude_cli_installed,
            claude_cli_latest,
            code_server_installed,
            code_server_latest,
            claude_ext_installed,
            claude_ext_latest,
            scan_error,
        }) => {
            info!(
                app_id,
                os_upgradable,
                os_security,
                "Agent update scan result"
            );
            if let Some(app) = registry.get_application(app_id).await {
                // Read latest agent version from version file
                let agent_version_latest = std::fs::read_to_string(
                    "/opt/homeroute/data/agent-binaries/hr-agent.version",
                )
                .ok()
                .map(|s| s.trim().to_string());

                let mut target = hr_common::events::UpdateTarget {
                    id: app_id.to_string(),
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
                // Fill missing *_latest from server-side cache (avoids GitHub rate limits)
                registry.fill_latest_versions(&mut target).await;
                registry
                    .scan_results
                    .write()
                    .await
                    .insert(app_id.to_string(), target.clone());
                let _ = state.events.update_scan.send(
                    hr_common::events::UpdateScanEvent::TargetScanned {
                        scan_id: String::new(),
                        target,
                    },
                );
            }
        }
        Err(e) => {
            warn!(app_id, "Invalid agent message: {e}");
        }
    }
}

// ══════════════════════════════════════════════════════════════════════
// Host-agent WebSocket (/host-agents/ws)
// ══════════════════════════════════════════════════════════════════════

pub async fn host_agent_ws(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_host_agent_socket(socket, state))
}

async fn handle_host_agent_socket(mut socket: WebSocket, state: WsState) {
    let registry = state.registry.clone();

    // Wait for Auth message (5s timeout)
    let auth_msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;
    let (host_id, host_name, version, role) = match auth_msg {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<HostAgentMessage>(&text) {
                Ok(HostAgentMessage::Auth {
                    token: _,
                    host_name,
                    version,
                    lan_interface,
                    container_storage_path,
                    role,
                }) => {
                    let mut data = load_hosts().await;
                    let host_id = data
                        .get("hosts")
                        .and_then(|h| h.as_array())
                        .and_then(|hosts| {
                            hosts.iter().find(|h| {
                                h.get("name").and_then(|n| n.as_str()) == Some(&host_name)
                            })
                        })
                        .and_then(|h| h.get("id").and_then(|i| i.as_str()))
                        .map(|s| s.to_string());

                    // Store lan_interface and container_storage_path from host agent
                    if let Some(ref id) = host_id {
                        if let Some(host) = find_host_mut(&mut data, id) {
                            let mut changed = false;
                            if let Some(ref iface) = lan_interface {
                                if host.get("lan_interface").and_then(|v| v.as_str())
                                    != Some(iface)
                                {
                                    host["lan_interface"] = json!(iface);
                                    changed = true;
                                }
                            }
                            if let Some(ref sp) = container_storage_path {
                                if host
                                    .get("container_storage_path")
                                    .and_then(|v| v.as_str())
                                    != Some(sp)
                                {
                                    host["container_storage_path"] = json!(sp);
                                    changed = true;
                                }
                            }
                            if let Some(ref r) = role {
                                host["role"] = serde_json::json!(r);
                                changed = true;
                            }
                            if changed {
                                let _ = save_hosts(&data).await;
                                info!(
                                    host = %host_name,
                                    "Updated host config from agent: lan_interface={:?}, storage_path={:?}",
                                    lan_interface,
                                    container_storage_path
                                );
                            }
                        }
                    }

                    match host_id {
                        Some(id) => (id, host_name, version, role),
                        None => {
                            warn!("Host agent auth failed: unknown host '{}'", host_name);
                            let _ = socket
                                .send(Message::Text(
                                    serde_json::to_string(
                                        &HostRegistryMessage::AuthResult {
                                            success: false,
                                            error: Some("Unknown host".to_string()),
                                        },
                                    )
                                    .unwrap()
                                    .into(),
                                ))
                                .await;
                            return;
                        }
                    }
                }
                _ => {
                    warn!("Host agent: expected Auth message");
                    return;
                }
            }
        }
        _ => {
            warn!("Host agent: auth timeout or error");
            return;
        }
    };

    // Send auth success
    if socket
        .send(Message::Text(
            serde_json::to_string(&HostRegistryMessage::AuthResult {
                success: true,
                error: None,
            })
            .unwrap()
            .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    info!("Host agent authenticated: {} ({})", host_name, host_id);

    // Register connection
    let (tx, mut rx) = mpsc::channel::<hr_registry::OutgoingHostMessage>(512);
    registry
        .on_host_connected(host_id.clone(), host_name.clone(), tx, version.clone(), role)
        .await;

    // Mark host online
    update_host_status(&host_id, "online", &state.events.host_status).await;

    // Push auto-off config to agent on connect
    {
        let data = load_hosts().await;
        if let Some(host) = find_host(&data, &host_id) {
            let mode_str = host
                .get("auto_off_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("off");
            let minutes = host
                .get("auto_off_minutes")
                .and_then(|v| v.as_u64())
                // Backward compat: fallback to old field
                .or_else(|| host.get("sleep_timeout_minutes").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u32;
            if mode_str != "off" && minutes > 0 {
                let mode = match mode_str {
                    "shutdown" => hr_registry::protocol::AutoOffMode::Shutdown,
                    _ => hr_registry::protocol::AutoOffMode::Sleep,
                };
                let _ = registry
                    .send_host_command(
                        &host_id,
                        HostRegistryMessage::SetAutoOff { mode, minutes },
                    )
                    .await;
            }
        }
    }

    // Restore containers that should be running on this host
    state
        .container_manager
        .restore_host_containers(&host_id)
        .await;

    // Track which transfer_ids are being relayed (remote->remote)
    let mut relay_transfers: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Track local nspawn imports (remote->local)
    let mut active_transfers: std::collections::HashMap<String, ActiveTransfer> =
        std::collections::HashMap::new();

    // Pending binary chunk metadata: (transfer_id, sequence, checksum)
    let mut pending_binary_meta: Option<(String, u32, u32)> = None;

    // Heartbeat timeout: agent sends every 5s, detect offline within 30s.
    // During backup, outgoing data can starve the recv arm, so we also reset
    // the timeout whenever we successfully send data (proves connection is alive).
    let heartbeat_timeout = std::time::Duration::from_secs(30);
    let timeout_sleep = tokio::time::sleep(heartbeat_timeout);
    tokio::pin!(timeout_sleep);

    // Bidirectional message loop
    loop {
        tokio::select! {
            // Messages from registry -> host-agent
            Some(msg) = rx.recv() => {
                let ws_msg = match msg {
                    hr_registry::OutgoingHostMessage::Text(m) => {
                        match serde_json::to_string(&m) {
                            Ok(t) => Message::Text(t.into()),
                            Err(_) => continue,
                        }
                    }
                    hr_registry::OutgoingHostMessage::Binary(data) => {
                        Message::Binary(data.into())
                    }
                };
                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    socket.send(ws_msg),
                ).await {
                    Ok(Ok(())) => {
                        // Successful send proves connection is alive — reset heartbeat
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                    }
                    Ok(Err(_)) => break,    // WebSocket send error
                    Err(_) => {             // 30s timeout
                        warn!("WebSocket send timeout for host {host_id}, disconnecting");
                        break;
                    }
                }
            }
            // Heartbeat timeout -- host likely asleep or unreachable
            _ = &mut timeout_sleep => {
                warn!("Host agent heartbeat timeout: {} ({})", host_name, host_id);
                break;
            }
            // Messages from host-agent -> registry
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // Any message from the agent resets the heartbeat deadline
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                        if let Ok(agent_msg) = serde_json::from_str::<HostAgentMessage>(&text) {
                            handle_host_agent_message(
                                &state,
                                &registry,
                                &host_id,
                                &host_name,
                                &version,
                                agent_msg,
                                &mut relay_transfers,
                                &mut active_transfers,
                                &mut pending_binary_meta,
                            ).await;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // Binary frame following a TransferChunkBinary metadata message
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                        handle_host_binary_frame(
                            &state,
                            &registry,
                            &host_id,
                            data.to_vec(),
                            &mut relay_transfers,
                            &mut active_transfers,
                            &mut pending_binary_meta,
                        ).await;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Clean up any pending relay transfers
    for tid in relay_transfers {
        registry.take_transfer_relay_target(&tid).await;
    }

    // Mark host offline
    update_host_status(&host_id, "offline", &state.events.host_status).await;

    registry.on_host_disconnected(&host_id).await;
    info!("Host agent disconnected: {} ({})", host_name, host_id);
}

// ── Host-agent text message handler ──────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_host_agent_message(
    state: &WsState,
    registry: &Arc<AgentRegistry>,
    host_id: &str,
    host_name: &str,
    version: &str,
    agent_msg: HostAgentMessage,
    relay_transfers: &mut std::collections::HashSet<String>,
    active_transfers: &mut std::collections::HashMap<String, ActiveTransfer>,
    pending_binary_meta: &mut Option<(String, u32, u32)>,
) {
    match agent_msg {
        HostAgentMessage::Heartbeat { .. } => {
            registry.update_host_heartbeat(host_id).await;
            update_host_last_seen(host_id).await;
        }
        HostAgentMessage::Metrics(metrics) => {
            registry
                .update_host_metrics(host_id, metrics.clone())
                .await;
            let _ = state.events.host_metrics.send(
                hr_common::events::HostMetricsEvent {
                    host_id: host_id.to_string(),
                    cpu_percent: metrics.cpu_percent,
                    memory_used_bytes: metrics.memory_used_bytes,
                    memory_total_bytes: metrics.memory_total_bytes,
                },
            );
        }
        HostAgentMessage::NetworkInterfaces(interfaces) => {
            registry
                .update_host_interfaces(host_id, interfaces.clone())
                .await;
            // Persist to hosts.json
            let mut data = load_hosts().await;
            if let Some(host) = find_host_mut(&mut data, host_id) {
                let ifaces: Vec<serde_json::Value> = interfaces
                    .iter()
                    .map(|i| {
                        json!({
                            "ifname": i.name,
                            "address": i.mac,
                            "ipv4": i.ipv4,
                            "is_up": i.is_up,
                        })
                    })
                    .collect();
                host["interfaces"] = json!(ifaces);
                let _ = save_hosts(&data).await;
            }
        }
        HostAgentMessage::ContainerList(containers) => {
            registry
                .update_host_containers(host_id, containers)
                .await;
        }
        HostAgentMessage::ImportComplete {
            transfer_id,
            container_name,
        } => {
            info!(
                transfer_id = %transfer_id,
                container = %container_name,
                "Host import complete"
            );
            registry
                .on_host_import_complete(host_id, &transfer_id, &container_name)
                .await;
        }
        HostAgentMessage::ImportFailed {
            transfer_id,
            error,
        } => {
            error!(
                transfer_id = %transfer_id,
                %error,
                "Host import failed"
            );
            registry
                .on_host_import_failed(host_id, &transfer_id, &error)
                .await;
        }
        HostAgentMessage::ExecResult {
            request_id,
            success,
            stdout,
            stderr,
        } => {
            info!(
                request_id = %request_id,
                success,
                "Host exec result"
            );
            registry
                .on_host_exec_result(host_id, &request_id, success, &stdout, &stderr)
                .await;
        }
        HostAgentMessage::ExportReady {
            transfer_id,
            container_name: _,
            size_bytes,
        } => {
            // Check if this is a remote->remote relay
            if let Some((target_host_id, _cname)) =
                registry.get_transfer_relay_target(&transfer_id).await
            {
                info!(
                    transfer_id = %transfer_id,
                    target = %target_host_id,
                    size_bytes,
                    "Relaying ExportReady to target host"
                );
                relay_transfers.insert(transfer_id.clone());
            } else if let Some(cname) =
                registry.take_transfer_container_name(&transfer_id).await
            {
                // Remote->Local nspawn import: set up file receiver
                let file_path = format!("/tmp/{}.tar.gz", transfer_id);
                match tokio::fs::File::create(&file_path).await {
                    Ok(file) => {
                        // Resolve storage path and network mode for local
                        let sp = state
                            .container_manager
                            .resolve_storage_path("local")
                            .await;
                        let nm = state
                            .container_manager
                            .resolve_network_mode("local")
                            .await
                            .unwrap_or_else(|_| "bridge:br-lan".to_string());

                        // app_id is used for progress tracking; in hr-orchestrator
                        // the migration state is not replicated, so we use empty.
                        let app_id = String::new();

                        info!(
                            transfer_id = %transfer_id,
                            container = %cname,
                            size_bytes,
                            "Setting up local nspawn import receiver"
                        );
                        active_transfers.insert(
                            transfer_id.clone(),
                            ActiveTransfer {
                                container_name: cname,
                                storage_path: sp,
                                network_mode: nm,
                                file,
                                phase: TransferPhase::ReceivingContainer,
                                workspace_file: None,
                                total_bytes: size_bytes,
                                bytes_received: 0,
                                chunk_count: 0,
                                app_id,
                                transfer_id: transfer_id.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        error!(
                            transfer_id = %transfer_id,
                            %e,
                            "Failed to create local import file"
                        );
                        registry
                            .on_host_import_failed(
                                host_id,
                                &transfer_id,
                                &format!("File creation error: {e}"),
                            )
                            .await;
                    }
                }
            }
        }
        HostAgentMessage::ExportFailed {
            transfer_id,
            error,
        } => {
            error!(
                transfer_id = %transfer_id,
                %error,
                "Host export failed"
            );
            relay_transfers.remove(&transfer_id);
            registry.take_transfer_relay_target(&transfer_id).await;
            active_transfers.remove(&transfer_id);
            let _ = tokio::fs::remove_file(format!("/tmp/{}.tar.gz", transfer_id)).await;
            let _ = tokio::fs::remove_file(format!("/tmp/{}-workspace.tar.gz", transfer_id))
                .await;
            registry
                .on_host_export_failed(host_id, &transfer_id, &error)
                .await;
        }
        HostAgentMessage::TransferChunkBinary {
            transfer_id,
            sequence,
            size,
            checksum,
        } => {
            if relay_transfers.contains(&transfer_id) {
                // Relay mode: forward metadata to target host
                if let Some((target_host_id, _)) =
                    registry.get_transfer_relay_target(&transfer_id).await
                {
                    let _ = registry
                        .send_host_command(
                            &target_host_id,
                            HostRegistryMessage::ReceiveChunkBinary {
                                transfer_id: transfer_id.clone(),
                                sequence,
                                size,
                                checksum,
                            },
                        )
                        .await;
                }
            }
            // Store metadata; the next Binary frame carries the actual data
            *pending_binary_meta = Some((transfer_id, sequence, checksum));
        }
        HostAgentMessage::TransferComplete { transfer_id } => {
            if relay_transfers.remove(&transfer_id) {
                // Relay mode: forward TransferComplete to target host
                info!(
                    transfer_id = %transfer_id,
                    "Relaying TransferComplete to target host"
                );
                if let Some((target_host_id, _)) =
                    registry.get_transfer_relay_target(&transfer_id).await
                {
                    let _ = registry
                        .send_host_command(
                            &target_host_id,
                            HostRegistryMessage::TransferComplete {
                                transfer_id: transfer_id.to_string(),
                            },
                        )
                        .await;
                }
                // Clean up relay target (import result will come from target host)
                registry
                    .take_transfer_relay_target(&transfer_id)
                    .await;
            } else if let Some(mut transfer) = active_transfers.remove(&transfer_id) {
                // Local nspawn import: finalize
                let _ = AsyncWriteExt::flush(&mut transfer.file).await;
                drop(transfer.file);
                let has_workspace =
                    transfer.phase == TransferPhase::ReceivingWorkspace;
                if let Some(mut ws_file) = transfer.workspace_file.take() {
                    let _ = AsyncWriteExt::flush(&mut ws_file).await;
                    drop(ws_file);
                }
                let tid = transfer_id.clone();
                let reg = registry.clone();
                let hid = host_id.to_string();
                tokio::spawn(async move {
                    handle_local_nspawn_import(
                        reg,
                        hid,
                        tid,
                        transfer.container_name,
                        transfer.storage_path,
                        transfer.network_mode,
                        has_workspace,
                    )
                    .await;
                });
            }
        }
        HostAgentMessage::AutoOffNotify { mode } => {
            let action = match mode {
                hr_registry::protocol::AutoOffMode::Sleep => {
                    info!(
                        "Host agent auto-sleep: {} ({})",
                        host_name, host_id
                    );
                    hr_common::events::PowerAction::Suspend
                }
                hr_registry::protocol::AutoOffMode::Shutdown => {
                    info!(
                        "Host agent auto-shutdown: {} ({})",
                        host_name, host_id
                    );
                    hr_common::events::PowerAction::Shutdown
                }
            };
            let _ = registry.request_power_action(host_id, action).await;
        }
        HostAgentMessage::WorkspaceReady {
            transfer_id,
            size_bytes,
        } => {
            if relay_transfers.contains(&transfer_id) {
                // Relay mode: forward WorkspaceReady to target host
                info!(
                    transfer_id = %transfer_id,
                    size_bytes,
                    "Relaying WorkspaceReady to target host"
                );
                if let Some((target_host_id, _)) =
                    registry.get_transfer_relay_target(&transfer_id).await
                {
                    let _ = registry
                        .send_host_command(
                            &target_host_id,
                            HostRegistryMessage::WorkspaceReady {
                                transfer_id: transfer_id.to_string(),
                                size_bytes,
                            },
                        )
                        .await;
                }
            } else if let Some(transfer) = active_transfers.get_mut(&transfer_id) {
                // Local import: transition to workspace phase
                info!(
                    transfer_id = %transfer_id,
                    size_bytes,
                    "Receiving workspace for local import"
                );
                let ws_path =
                    format!("/tmp/{}-workspace.tar.gz", transfer_id);
                match tokio::fs::File::create(&ws_path).await {
                    Ok(ws_file) => {
                        transfer.phase = TransferPhase::ReceivingWorkspace;
                        transfer.workspace_file = Some(ws_file);
                        // Reset byte counters for workspace phase
                        transfer.total_bytes = size_bytes;
                        transfer.bytes_received = 0;
                        transfer.chunk_count = 0;
                    }
                    Err(e) => {
                        error!(
                            transfer_id = %transfer_id,
                            %e,
                            "Failed to create workspace file for local import"
                        );
                    }
                }
            }
        }
        HostAgentMessage::TerminalData { session_id, data } => {
            registry.send_terminal_data(&session_id, data).await;
        }
        HostAgentMessage::TerminalOpened { session_id } => {
            tracing::debug!(
                session_id = %session_id,
                "Remote terminal opened"
            );
        }
        HostAgentMessage::TerminalClosed {
            session_id,
            exit_code,
        } => {
            info!(
                session_id = %session_id,
                ?exit_code,
                "Remote terminal closed"
            );
            // Send empty data to signal close to the API WS handler
            registry
                .send_terminal_data(&session_id, Vec::new())
                .await;
        }
        HostAgentMessage::Auth { .. } => {}
        HostAgentMessage::NspawnContainerList(_) => {
            // TODO: track nspawn containers separately if needed
        }
        HostAgentMessage::UpdateScanResult {
            os_upgradable,
            os_security,
            scan_error,
        } => {
            info!(
                host_id = %host_id,
                os_upgradable,
                os_security,
                "Host update scan result"
            );
            let target = hr_common::events::UpdateTarget {
                id: host_id.to_string(),
                name: host_name.to_string(),
                target_type: "remote_host".to_string(),
                environment: None,
                online: true,
                os_upgradable,
                os_security,
                agent_version: Some(version.to_string()),
                agent_version_latest: None,
                claude_cli_installed: None,
                claude_cli_latest: None,
                code_server_installed: None,
                code_server_latest: None,
                claude_ext_installed: None,
                claude_ext_latest: None,
                scan_error,
                scanned_at: chrono::Utc::now().to_rfc3339(),
            };
            registry
                .scan_results
                .write()
                .await
                .insert(host_id.to_string(), target.clone());
            let _ = state.events.update_scan.send(
                hr_common::events::UpdateScanEvent::TargetScanned {
                    scan_id: String::new(),
                    target,
                },
            );
        }
        HostAgentMessage::BackupRepoReady {
            transfer_id,
        } => {
            info!(transfer_id = %transfer_id, "Backup repo ready");
            registry.on_backup_repo_ready(&transfer_id).await;
        }
        HostAgentMessage::BackupRepoComplete {
            transfer_id,
            repo_name,
            success,
            message,
            snapshot_name,
        } => {
            info!(
                transfer_id = %transfer_id,
                repo = %repo_name,
                success,
                "Backup repo complete"
            );
            registry
                .on_backup_repo_complete(&transfer_id, success, &message, snapshot_name)
                .await;
        }
    }
}

// ── Host-agent binary frame handler ──────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_host_binary_frame(
    _state: &WsState,
    registry: &Arc<AgentRegistry>,
    host_id: &str,
    data: Vec<u8>,
    relay_transfers: &mut std::collections::HashSet<String>,
    active_transfers: &mut std::collections::HashMap<String, ActiveTransfer>,
    pending_binary_meta: &mut Option<(String, u32, u32)>,
) {
    if let Some((transfer_id, _sequence, _checksum)) = pending_binary_meta.take() {
        if relay_transfers.contains(&transfer_id) {
            // Relay mode: forward binary data to target host
            if let Some((target_host_id, _)) =
                registry.get_transfer_relay_target(&transfer_id).await
            {
                if let Err(e) = registry
                    .send_host_binary(&target_host_id, data)
                    .await
                {
                    error!(
                        transfer_id = %transfer_id,
                        %e,
                        "Failed to relay binary chunk to target"
                    );
                    relay_transfers.remove(&transfer_id);
                    registry.take_transfer_relay_target(&transfer_id).await;
                    registry
                        .on_host_import_failed(
                            host_id,
                            &transfer_id,
                            &format!("Relay binary send failed: {e}"),
                        )
                        .await;
                }
            }
        } else if registry.backup_manifest_data.read().await.contains_key(&transfer_id) {
            // Backup manifest receive mode: accumulate data
            registry.append_backup_manifest_data(&transfer_id, &data).await;
        } else if let Some(transfer) = active_transfers.get_mut(&transfer_id) {
            // Local import mode: write binary data to file
            let data_len = data.len() as u64;
            let target_file = match transfer.phase {
                TransferPhase::ReceivingWorkspace => transfer.workspace_file.as_mut(),
                _ => Some(&mut transfer.file),
            };
            if let Some(file) = target_file {
                if let Err(e) = AsyncWriteExt::write_all(file, &data).await {
                    error!(
                        transfer_id = %transfer_id,
                        %e,
                        "Failed to write binary chunk to local file"
                    );
                    active_transfers.remove(&transfer_id);
                    registry
                        .on_host_import_failed(
                            host_id,
                            &transfer_id,
                            &format!("File write error: {e}"),
                        )
                        .await;
                } else {
                    // Update progress tracking
                    transfer.bytes_received += data_len;
                    transfer.chunk_count += 1;
                }
            }
        }
    } else {
        warn!("Received Binary frame without pending TransferChunkBinary metadata");
    }
}

// ── Local nspawn import for remote->local migration ─────────────────

async fn handle_local_nspawn_import(
    registry: Arc<AgentRegistry>,
    source_host_id: String,
    transfer_id: String,
    container_name: String,
    storage_path: String,
    network_mode: String,
    has_workspace: bool,
) {
    let import_path = format!("/tmp/{}.tar.gz", transfer_id);
    let ws_import_path = format!("/tmp/{}-workspace.tar.gz", transfer_id);

    // Verify the file exists and is non-empty
    match tokio::fs::metadata(&import_path).await {
        Ok(m) if m.len() == 0 => {
            error!(
                transfer_id = %transfer_id,
                "Transfer file is empty"
            );
            registry
                .on_host_import_failed(&source_host_id, &transfer_id, "Transfer file is empty")
                .await;
            let _ = tokio::fs::remove_file(&import_path).await;
            let _ = tokio::fs::remove_file(&ws_import_path).await;
            return;
        }
        Err(e) => {
            error!(
                transfer_id = %transfer_id,
                %e,
                "Transfer file missing"
            );
            registry
                .on_host_import_failed(
                    &source_host_id,
                    &transfer_id,
                    &format!("Transfer file missing: {e}"),
                )
                .await;
            let _ = tokio::fs::remove_file(&ws_import_path).await;
            return;
        }
        Ok(m) => {
            info!(
                transfer_id = %transfer_id,
                size_bytes = m.len(),
                "Starting local nspawn import"
            );
        }
    }

    let rootfs_dir = format!("{}/{}", storage_path, container_name);
    let ws_dir = format!("{}/{}-workspace", storage_path, container_name);

    // Create rootfs directory
    if let Err(e) = tokio::fs::create_dir_all(&rootfs_dir).await {
        error!(
            transfer_id = %transfer_id,
            %e,
            "Failed to create rootfs directory"
        );
        registry
            .on_host_import_failed(
                &source_host_id,
                &transfer_id,
                &format!("Failed to create rootfs dir: {e}"),
            )
            .await;
        let _ = tokio::fs::remove_file(&import_path).await;
        let _ = tokio::fs::remove_file(&ws_import_path).await;
        return;
    }

    // Extract container tar
    info!(
        transfer_id = %transfer_id,
        container = %container_name,
        dir = %rootfs_dir,
        "Extracting container tar"
    );
    let extract = Command::new("tar")
        .args([
            "xf",
            &import_path,
            "--numeric-owner",
            "--xattrs",
            "--xattrs-include=*",
            "-C",
            &rootfs_dir,
        ])
        .output()
        .await;

    match &extract {
        Ok(output) if output.status.success() => {
            info!(
                transfer_id = %transfer_id,
                "Container tar extracted successfully"
            );
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(
                transfer_id = %transfer_id,
                %stderr,
                "Container tar extraction failed"
            );
            registry
                .on_host_import_failed(
                    &source_host_id,
                    &transfer_id,
                    &format!("tar extract failed: {stderr}"),
                )
                .await;
            let _ = tokio::fs::remove_dir_all(&rootfs_dir).await;
            let _ = tokio::fs::remove_file(&import_path).await;
            let _ = tokio::fs::remove_file(&ws_import_path).await;
            return;
        }
        Err(e) => {
            error!(
                transfer_id = %transfer_id,
                %e,
                "tar command error"
            );
            registry
                .on_host_import_failed(
                    &source_host_id,
                    &transfer_id,
                    &format!("tar command error: {e}"),
                )
                .await;
            let _ = tokio::fs::remove_dir_all(&rootfs_dir).await;
            let _ = tokio::fs::remove_file(&import_path).await;
            let _ = tokio::fs::remove_file(&ws_import_path).await;
            return;
        }
    }

    // Handle workspace
    if has_workspace {
        if let Err(e) = tokio::fs::create_dir_all(&ws_dir).await {
            warn!(
                transfer_id = %transfer_id,
                %e,
                "Failed to create workspace dir"
            );
        }
        let ws_extract = Command::new("tar")
            .args([
                "xf",
                &ws_import_path,
                "--numeric-owner",
                "--xattrs",
                "--xattrs-include=*",
                "-C",
                &ws_dir,
            ])
            .output()
            .await;
        match &ws_extract {
            Ok(output) if output.status.success() => {
                info!(
                    transfer_id = %transfer_id,
                    "Workspace tar extracted successfully"
                );
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    transfer_id = %transfer_id,
                    %stderr,
                    "Workspace tar extraction failed, creating empty workspace"
                );
                let _ = tokio::fs::create_dir_all(&ws_dir).await;
            }
            Err(e) => {
                warn!(
                    transfer_id = %transfer_id,
                    %e,
                    "Workspace tar error, creating empty workspace"
                );
                let _ = tokio::fs::create_dir_all(&ws_dir).await;
            }
        }
    }

    let sp = std::path::Path::new(&storage_path);

    // Write .nspawn unit (dev containers get workspace bind, prod don't)
    if let Err(e) = hr_container::NspawnClient::write_nspawn_unit(
        &container_name,
        sp,
        &network_mode,
        has_workspace,
    )
    .await
    {
        error!(
            transfer_id = %transfer_id,
            %e,
            "Failed to write nspawn unit"
        );
        registry
            .on_host_import_failed(
                &source_host_id,
                &transfer_id,
                &format!("Failed to write nspawn unit: {e}"),
            )
            .await;
        let _ = tokio::fs::remove_dir_all(&rootfs_dir).await;
        if has_workspace {
            let _ = tokio::fs::remove_dir_all(&ws_dir).await;
        }
        let _ = tokio::fs::remove_file(&import_path).await;
        let _ = tokio::fs::remove_file(&ws_import_path).await;
        return;
    }

    // Write network config in rootfs
    if let Err(e) =
        hr_container::NspawnClient::write_network_config(&container_name, sp).await
    {
        error!(
            transfer_id = %transfer_id,
            %e,
            "Failed to write network config"
        );
        registry
            .on_host_import_failed(
                &source_host_id,
                &transfer_id,
                &format!("Failed to write network config: {e}"),
            )
            .await;
        let _ = tokio::fs::remove_dir_all(&rootfs_dir).await;
        if has_workspace {
            let _ = tokio::fs::remove_dir_all(&ws_dir).await;
        }
        let _ = tokio::fs::remove_file(&import_path).await;
        let _ = tokio::fs::remove_file(&ws_import_path).await;
        return;
    }

    // Start the container
    match hr_container::NspawnClient::start_container(&container_name).await {
        Ok(()) => {
            info!(
                transfer_id = %transfer_id,
                container = %container_name,
                "Nspawn container started after local import"
            );
            registry
                .on_host_import_complete("local", &transfer_id, &container_name)
                .await;
        }
        Err(e) => {
            error!(
                transfer_id = %transfer_id,
                %e,
                "Container start failed after import"
            );
            registry
                .on_host_import_failed(
                    &source_host_id,
                    &transfer_id,
                    &format!("Start failed: {e}"),
                )
                .await;
        }
    }

    // Cleanup transfer files
    let _ = tokio::fs::remove_file(&import_path).await;
    let _ = tokio::fs::remove_file(&ws_import_path).await;
}

// ══════════════════════════════════════════════════════════════════════
// Terminal WebSocket (/containers/{id}/terminal)
// ══════════════════════════════════════════════════════════════════════

pub async fn terminal_ws(
    State(state): State<WsState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal_ws(state, id, socket))
}

async fn handle_terminal_ws(state: WsState, container_id: String, mut socket: WebSocket) {
    let mgr = &state.container_manager;

    // Look up the container record to get the container name and host_id
    let containers = mgr.list_containers().await;
    let container_record = containers
        .iter()
        .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(&container_id));

    let Some(record) = container_record else {
        let _ = socket
            .send(Message::Text(
                json!({"error": "Container not found"}).to_string().into(),
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
                json!({"error": "Container not found"}).to_string().into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    info!(container, host_id, "Container V2 terminal WebSocket opened");

    if host_id == "local" {
        handle_terminal_local(&container, &mut socket).await;
    } else {
        handle_terminal_remote(&state.registry, &host_id, &container, &mut socket).await;
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
                    json!({"error": format!("Failed to get container PID: {stderr}")})
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
                    json!({"error": format!("Failed to get container PID: {e}")})
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
                    json!({"error": format!("Failed to start shell: {e}")})
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
    registry: &Arc<AgentRegistry>,
    host_id: &str,
    container: &str,
    socket: &mut WebSocket,
) {
    if !registry.is_host_connected(host_id).await {
        let _ = socket
            .send(Message::Text(
                json!({"error": "Host is not connected"}).to_string().into(),
            ))
            .await;
        return;
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);

    // Register session so data from host-agent is routed to us
    registry
        .register_terminal_session(&session_id, tx)
        .await;

    // Send TerminalOpen to host-agent
    if let Err(e) = registry
        .send_host_command(
            host_id,
            HostRegistryMessage::TerminalOpen {
                session_id: session_id.clone(),
                container_name: container.to_string(),
            },
        )
        .await
    {
        error!(container, host_id, "Failed to send TerminalOpen: {e}");
        let _ = socket
            .send(Message::Text(
                json!({"error": format!("Failed to open remote terminal: {e}")})
                    .to_string()
                    .into(),
            ))
            .await;
        registry
            .unregister_terminal_session(&session_id)
            .await;
        return;
    }

    info!(
        container,
        host_id,
        session_id,
        "Remote terminal session started"
    );

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
                            HostRegistryMessage::TerminalData {
                                session_id: session_id.clone(),
                                data: text.as_bytes().to_vec(),
                            },
                        ).await;
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let _ = registry.send_host_command(
                            host_id,
                            HostRegistryMessage::TerminalData {
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
            HostRegistryMessage::TerminalClose {
                session_id: session_id.clone(),
            },
        )
        .await;
    registry
        .unregister_terminal_session(&session_id)
        .await;

    info!(
        container,
        host_id,
        session_id,
        "Remote terminal session ended"
    );
}

// ══════════════════════════════════════════════════════════════════════
// DNS record helpers for agent lifecycle
// ══════════════════════════════════════════════════════════════════════

/// Add local DNS A records for an agent via IPC, based on environment:
/// - Development: `*.{slug}.{base}` -> IPv4 (covers dev.{slug} and code.{slug})
/// - Production: `{slug}.{base}` -> IPv4
async fn add_agent_dns_records(
    netcore: &NetcoreClient,
    slug: &str,
    base_domain: &str,
    ipv4: &str,
    environment: hr_registry::types::Environment,
) {
    let (name, record_type) = match environment {
        hr_registry::types::Environment::Development => {
            (format!("*.{}.{}", slug, base_domain), "A".to_string())
        }
        hr_registry::types::Environment::Production => {
            (format!("{}.{}", slug, base_domain), "A".to_string())
        }
    };
    if let Err(e) = netcore
        .dns_add_static_record(name, record_type, ipv4.to_string(), 60)
        .await
    {
        warn!(slug, ipv4, error = %e, "Failed to add DNS record via IPC");
    } else {
        info!(
            slug,
            ipv4,
            ?environment,
            "Added local DNS A records for agent"
        );
    }
}

/// Remove all local DNS records pointing to a specific IPv4 address via IPC.
async fn remove_agent_dns_records(netcore: &NetcoreClient, ipv4: &str) {
    if let Err(e) = netcore
        .dns_remove_static_records_by_value(ipv4)
        .await
    {
        warn!(ipv4, error = %e, "Failed to remove DNS records via IPC");
    } else {
        info!(ipv4, "Removed local DNS records for agent IP");
    }
}

// ══════════════════════════════════════════════════════════════════════
// hosts.json access helpers (same as in hr-api/routes/hosts.rs)
// ══════════════════════════════════════════════════════════════════════

async fn load_hosts() -> serde_json::Value {
    match tokio::fs::read_to_string(HOSTS_FILE).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or(json!({"hosts": []})),
        Err(_) => json!({"hosts": []}),
    }
}

async fn save_hosts(data: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    let tmp = format!("{}.tmp", HOSTS_FILE);
    tokio::fs::write(&tmp, &content)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, HOSTS_FILE)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn find_host<'a>(data: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
    data.get("hosts")?
        .as_array()?
        .iter()
        .find(|h| h.get("id").and_then(|i| i.as_str()) == Some(id))
}

fn find_host_mut<'a>(
    data: &'a mut serde_json::Value,
    id: &str,
) -> Option<&'a mut serde_json::Value> {
    data.get_mut("hosts")?
        .as_array_mut()?
        .iter_mut()
        .find(|h| h.get("id").and_then(|i| i.as_str()) == Some(id))
}

async fn update_host_status(
    host_id: &str,
    status: &str,
    host_events: &tokio::sync::broadcast::Sender<hr_common::events::HostStatusEvent>,
) {
    let mut data = load_hosts().await;
    if let Some(host) = find_host_mut(&mut data, host_id) {
        let now = chrono::Utc::now().to_rfc3339();
        host["status"] = json!(status);
        host["lastSeen"] = json!(&now);
        let _ = save_hosts(&data).await;
    }
    let _ = host_events.send(hr_common::events::HostStatusEvent {
        host_id: host_id.to_string(),
        status: status.to_string(),
        latency_ms: None,
    });
}

async fn update_host_last_seen(host_id: &str) {
    let mut data = load_hosts().await;
    if let Some(host) = find_host_mut(&mut data, host_id) {
        host["lastSeen"] = json!(chrono::Utc::now().to_rfc3339());
        let _ = save_hosts(&data).await;
    }
}
