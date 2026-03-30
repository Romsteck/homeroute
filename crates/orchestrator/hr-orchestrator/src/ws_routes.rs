//! WebSocket endpoints for hr-orchestrator (port 4001).
//!
//! These handlers manage agent, host-agent, and env-agent WebSocket connections
//! directly against the local AgentRegistry, without going through hr-api.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_ipc::{EdgeClient, NetcoreClient};
use hr_environment::{EnvAgentMessage, EnvOrchestratorMessage};
use hr_registry::protocol::{AgentMessage, HostAgentMessage, HostRegistryMessage, RegistryMessage};
use hr_registry::AgentRegistry;

use crate::env_manager::EnvironmentManager;

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
    pub env_manager: Arc<EnvironmentManager>,
    pub env: Arc<EnvConfig>,
    pub events: Arc<EventBus>,
    pub edge: Arc<EdgeClient>,
    pub netcore: Arc<NetcoreClient>,
    pub pipeline_engine: Arc<hr_pipeline::PipelineEngine>,
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
            // (restores Connected status after host recovery)
            registry.handle_heartbeat(app_id).await;
            registry.handle_metrics(app_id, m).await;
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

    // Track which transfer_ids are being relayed (remote->remote)
    let mut relay_transfers: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Track local nspawn imports (remote->local)
    let mut active_transfers: std::collections::HashMap<String, ActiveTransfer> =
        std::collections::HashMap::new();

    // Pending binary chunk metadata: (transfer_id, sequence, checksum)
    let mut pending_binary_meta: Option<(String, u32, u32)> = None;

    // Heartbeat timeout: agent sends every 5s. During backup finalization,
    // the host-agent may be busy with I/O (snapshot creation, manifest writes)
    // for large repos (420K+ files), so we use a generous 120s timeout.
    let heartbeat_timeout = std::time::Duration::from_secs(120);
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
                            Ok(t) => {
                                // Log non-heartbeat outgoing messages for debugging
                                if !t.contains("Heartbeat") {
                                    info!(host = %host_name, "WS→host: {}", &t[..t.len().min(200)]);
                                }
                                Message::Text(t.into())
                            }
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
                    Ok(Err(e)) => {
                        warn!(host = %host_name, "WebSocket send error: {e}");
                        break;
                    }
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
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
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
                        let sp = "/var/lib/machines".to_string();
                        let nm = "bridge:br-lan".to_string();

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
        HostAgentMessage::BackupRepoReady { transfer_id } => {
            // Legacy: backup now runs locally via borg, no host-agent needed
            warn!(transfer_id = %transfer_id, "Received legacy BackupRepoReady (ignored)");
        }
        HostAgentMessage::BackupRepoComplete {
            transfer_id,
            repo_name,
            success: _,
            message: _,
            snapshot_name: _,
        } => {
            // Legacy: backup now runs locally via borg, no host-agent needed
            warn!(transfer_id = %transfer_id, repo = %repo_name, "Received legacy BackupRepoComplete (ignored)");
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
        &[],
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

// (Legacy terminal WebSocket for containers removed)

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
    let _ = environment; // kept for signature compat
    let (name, record_type) = (format!("{}.{}", slug, base_domain), "A".to_string());
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

// ══════════════════════════════════════════════════════════════════════
// Env-Agent WebSocket (/envs/ws)
// ══════════════════════════════════════════════════════════════════════

pub async fn env_agent_ws(
    State(state): State<WsState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_env_agent_ws(state, socket))
}

async fn handle_env_agent_ws(state: WsState, mut socket: WebSocket) {
    let env_manager = &state.env_manager;

    // Wait for Auth message with a timeout
    let auth_msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;

    let (token, env_slug, version, reported_ipv4) = match auth_msg {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<EnvAgentMessage>(&text) {
                Ok(EnvAgentMessage::Auth {
                    token,
                    env_slug,
                    version,
                    ipv4_address,
                }) => (token, env_slug, version, ipv4_address),
                _ => {
                    warn!("Env-Agent WS: expected Auth message, got something else");
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
            }
        }
        _ => {
            warn!("Env-Agent WS: auth timeout or connection error");
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    // Authenticate
    let Some(env_id) = env_manager.verify_token(&env_slug, &token).await else {
        let reject = EnvOrchestratorMessage::AuthResult {
            success: false,
            error: Some("Invalid credentials".into()),
            env_id: None,
        };
        let _ = socket
            .send(Message::Text(
                serde_json::to_string(&reject).unwrap().into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    let ipv4: Option<std::net::Ipv4Addr> = reported_ipv4
        .as_deref()
        .and_then(|s| s.parse().ok());

    info!(
        env_id = env_id,
        env_slug = env_slug,
        ipv4 = ?ipv4,
        "Env-agent authenticated"
    );

    // Mark agent as connected
    env_manager
        .update_agent_connected(&env_slug, true, Some(version), ipv4)
        .await;

    // Send auth success
    let success = EnvOrchestratorMessage::AuthResult {
        success: true,
        error: None,
        env_id: Some(env_id.clone()),
    };
    if socket
        .send(Message::Text(
            serde_json::to_string(&success).unwrap().into(),
        ))
        .await
        .is_err()
    {
        env_manager
            .update_agent_connected(&env_slug, false, None, None)
            .await;
        return;
    }

    // Add DNS wildcard for this environment — points to hr-edge IP (not the container)
    // so that TLS termination happens at the proxy level.
    {
        let env_domain = format!("*.{}.{}", env_slug, state.env.base_domain);
        // hr-edge listens on the secondary IP (10.0.0.254 typically). Use the same IP
        // that the base domain resolves to — this is the machine's proxy address.
        let proxy_ip = "10.0.0.254".to_string();
        if let Err(e) = state.netcore.dns_add_static_record(env_domain, "A".into(), proxy_ip.clone(), 60).await {
            warn!(env_slug, error = %e, "Failed to add env DNS wildcard");
        } else {
            info!(env_slug, proxy_ip, "Added DNS wildcard for environment (pointing to hr-edge)");
        }
    }

    // Request TLS wildcard certificate for this environment (*.{env}.mynetwk.biz)
    // if not already available. This is async and non-blocking — runs in background.
    {
        let edge = state.edge.clone();
        let slug = env_slug.clone();
        tokio::spawn(async move {
            match edge.request_env_wildcard_cert(&slug).await {
                Ok(resp) if resp.ok => {
                    info!(env_slug = slug, "Env wildcard TLS certificate ready");
                }
                Ok(resp) => {
                    warn!(env_slug = slug, error = ?resp.error, "Env wildcard cert request returned error");
                }
                Err(e) => {
                    warn!(env_slug = slug, error = %e, "Failed to request env wildcard cert via edge IPC");
                }
            }
        });
    }

    // Create mpsc channel for orchestrator -> env-agent messages
    let (tx, mut rx) = mpsc::channel::<EnvOrchestratorMessage>(32);

    // Register the connection with the env manager
    if let Err(e) = env_manager.register_connection(&env_slug, tx).await {
        warn!(env_slug, error = %e, "Failed to register env-agent connection");
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    // Bidirectional message loop
    info!(env_slug, "Starting bidirectional WS loop");
    loop {
        tokio::select! {
            // Orchestrator -> Env-Agent
            Some(msg) = rx.recv() => {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    warn!(env_slug, "Failed to forward message to env-agent WebSocket");
                    break;
                }
            }
            // Env-Agent -> Orchestrator
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_env_agent_message(&state, &env_slug, &text).await;
                    }
                    Some(Ok(Message::Close(reason))) => {
                        info!(env_slug, ?reason, "Env WS closed by client");
                        break;
                    }
                    None => {
                        info!(env_slug, "Env WS stream ended (None)");
                        break;
                    }
                    other => {
                        tracing::debug!(env_slug, ?other, "Env WS non-text message");
                    }
                }
            }
        }
    }

    info!(env_slug, "Env WS loop exited, cleaning up");

    // Remove edge app routes and DNS records for this environment
    if let Some(env_record) = env_manager.get_by_slug(&env_slug).await {
        let base_domain = &state.env.base_domain;

        // Remove app routes
        for app in &env_record.apps {
            let domain = format!("{}.{}.{}", app.slug, env_slug, base_domain);
            if let Err(e) = state.edge.remove_app_route(&domain).await {
                warn!(domain, error = %e, "Failed to remove env app route on disconnect");
            }
        }

        // Remove studio route
        let studio_domain = format!("studio.{}.{}", env_slug, base_domain);
        if let Err(e) = state.edge.remove_app_route(&studio_domain).await {
            warn!(domain = studio_domain, error = %e, "Failed to remove env studio route on disconnect");
        }

        // Remove DNS wildcard records for this env
        if let Some(ip) = env_record.ipv4_address {
            remove_agent_dns_records(&state.netcore, &ip.to_string()).await;
        }
    }

    // Unregister connection and mark as disconnected
    env_manager.unregister_connection(&env_slug).await;
    info!(env_slug, "Env-agent WebSocket closed (routes + DNS removed)");
}

/// Process a single env-agent message.
async fn handle_env_agent_message(
    state: &WsState,
    env_slug: &str,
    text: &str,
) {
    let env_manager = &state.env_manager;
    let pipeline_engine = &state.pipeline_engine;

    match serde_json::from_str::<EnvAgentMessage>(text) {
        Ok(EnvAgentMessage::Heartbeat { apps_running, apps_total, .. }) => {
            env_manager.update_heartbeat(env_slug, apps_running, apps_total).await;
        }
        Ok(EnvAgentMessage::AppDiscovery { apps }) => {
            info!(env_slug, count = apps.len(), "Env-agent app discovery");

            // Create edge app routes for each discovered app
            if let Some(env_record) = env_manager.get_by_slug(env_slug).await {
                if let Some(env_ip) = env_record.ipv4_address {
                    let base_domain = &state.env.base_domain;
                    let ip_str = env_ip.to_string();

                    for app in &apps {
                        let domain = format!("{}.{}.{}", app.slug, env_slug, base_domain);
                        if let Err(e) = state.edge.set_app_route(
                            domain.clone(),
                            format!("env-{}-{}", env_slug, app.slug),
                            env_record.host_id.clone(),
                            ip_str.clone(),
                            app.port,
                            false,
                            vec![],
                            false,
                        ).await {
                            warn!(
                                domain,
                                error = %e,
                                "Failed to set env app route via edge IPC"
                            );
                        } else {
                            info!(domain, env_slug, app_slug = app.slug, "Set env app route");
                        }
                    }

                    // Studio route → homeroute port 4003 (Rust SPA with code-server iframe)
                    let studio_domain = format!("studio.{}.{}", env_slug, base_domain);
                    if let Err(e) = state.edge.set_app_route(
                        studio_domain.clone(),
                        format!("env-{}-studio", env_slug),
                        "local".to_string(),
                        "10.0.0.20".to_string(),
                        4003,
                        false,
                        vec![],
                        false,
                    ).await {
                        warn!(
                            domain = studio_domain,
                            error = %e,
                            "Failed to set env studio route via edge IPC"
                        );
                    } else {
                        info!(domain = studio_domain, env_slug, "Set env studio route");
                    }

                    // code-server route → env container:8443 (direct proxy for iframe)
                    let code_domain = format!("code.{}.{}", env_slug, base_domain);
                    if let Err(e) = state.edge.set_app_route(
                        code_domain.clone(),
                        format!("env-{}-code", env_slug),
                        env_record.host_id.clone(),
                        env_ip.to_string(),
                        8443,
                        false,
                        vec![],
                        false,
                    ).await {
                        warn!(domain = code_domain, error = %e, "Failed to set code-server route");
                    } else {
                        info!(domain = code_domain, env_slug, "Set code-server route");
                    }
                }
            }

            env_manager.update_apps(env_slug, apps).await;
        }
        Ok(EnvAgentMessage::AppStatus {
            app_slug, running, ..
        }) => {
            info!(env_slug, app_slug, running, "Env-agent app status change");
        }
        Ok(EnvAgentMessage::Metrics { data: metrics }) => {
            env_manager.update_metrics(env_slug, metrics).await;
        }
        Ok(EnvAgentMessage::Error { message }) => {
            warn!(env_slug, message, "Env-agent reported error");
        }
        Ok(EnvAgentMessage::Auth { .. }) => {
            // Duplicate auth, ignore
        }
        Ok(EnvAgentMessage::PipelineProgress { pipeline_id, step, success, message }) => {
            info!(env_slug, pipeline_id, step, success, ?message, "Pipeline progress");
            pipeline_engine.on_pipeline_progress(&pipeline_id, &step, success, message).await;
        }
        Ok(EnvAgentMessage::MigrationResult { pipeline_id, app_slug, success, migrations_applied, error }) => {
            info!(env_slug, pipeline_id, app_slug, success, "Migration result");
            pipeline_engine.on_migration_result(&pipeline_id, &app_slug, success, migrations_applied, error).await;
        }
        Ok(EnvAgentMessage::HostMetrics { hostname, total_memory_mb, available_memory_mb, cpu_usage_percent }) => {
            debug!(env_slug, hostname, total_memory_mb, available_memory_mb, cpu_usage_percent, "Env-agent host metrics");
            // Store total memory for the environment (convert MB to bytes)
            env_manager.update_host_memory(env_slug, total_memory_mb * 1024 * 1024).await;
        }
        Err(e) => {
            warn!(env_slug, error = %e, "Failed to parse env-agent message");
        }
    }
}
