//! WebSocket endpoints for hr-orchestrator (port 4001).
//!
//! Manages host-agent WebSocket connections against the local AgentRegistry.

use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use hr_common::events::EventBus;
use hr_registry::AgentRegistry;
use hr_registry::protocol::{HostAgentMessage, HostRegistryMessage};

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
    pub events: Arc<EventBus>,
}

// ── Constants ────────────────────────────────────────────────────────

const HOSTS_FILE: &str = "/data/hosts.json";

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

                    if let Some(ref id) = host_id {
                        if let Some(host) = find_host_mut(&mut data, id) {
                            let mut changed = false;
                            if let Some(ref iface) = lan_interface {
                                if host.get("lan_interface").and_then(|v| v.as_str()) != Some(iface)
                                {
                                    host["lan_interface"] = json!(iface);
                                    changed = true;
                                }
                            }
                            if let Some(ref sp) = container_storage_path {
                                if host.get("container_storage_path").and_then(|v| v.as_str())
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
                            }
                        }
                    }

                    match host_id {
                        Some(id) => (id, host_name, version, role),
                        None => {
                            warn!("Host agent auth failed: unknown host '{}'", host_name);
                            let _ = socket
                                .send(Message::Text(
                                    serde_json::to_string(&HostRegistryMessage::AuthResult {
                                        success: false,
                                        error: Some("Unknown host".to_string()),
                                    })
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

    let (tx, mut rx) = mpsc::channel::<hr_registry::OutgoingHostMessage>(512);
    registry
        .on_host_connected(
            host_id.clone(),
            host_name.clone(),
            tx,
            version.clone(),
            role,
        )
        .await;

    update_host_status(&host_id, "online", &state.events.host_status).await;

    let mut relay_transfers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut active_transfers: std::collections::HashMap<String, ActiveTransfer> =
        std::collections::HashMap::new();
    let mut pending_binary_meta: Option<(String, u32, u32)> = None;

    let heartbeat_timeout = std::time::Duration::from_secs(120);
    let timeout_sleep = tokio::time::sleep(heartbeat_timeout);
    tokio::pin!(timeout_sleep);

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                let ws_msg = match msg {
                    hr_registry::OutgoingHostMessage::Text(m) => {
                        match serde_json::to_string(&m) {
                            Ok(t) => {
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
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                    }
                    Ok(Err(e)) => {
                        warn!(host = %host_name, "WebSocket send error: {e}");
                        break;
                    }
                    Err(_) => {
                        warn!("WebSocket send timeout for host {host_id}, disconnecting");
                        break;
                    }
                }
            }
            _ = &mut timeout_sleep => {
                warn!("Host agent heartbeat timeout: {} ({})", host_name, host_id);
                break;
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                        if let Ok(agent_msg) = serde_json::from_str::<HostAgentMessage>(&text) {
                            handle_host_agent_message(
                                &state, &registry, &host_id, &host_name, &version,
                                agent_msg, &mut relay_transfers, &mut active_transfers,
                                &mut pending_binary_meta,
                            ).await;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        timeout_sleep.as_mut().reset(tokio::time::Instant::now() + heartbeat_timeout);
                        handle_host_binary_frame(
                            &state, &registry, &host_id, data.to_vec(),
                            &mut relay_transfers, &mut active_transfers,
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

    for tid in relay_transfers {
        registry.take_transfer_relay_target(&tid).await;
    }

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
            registry.update_host_metrics(host_id, metrics.clone()).await;
            let _ = state
                .events
                .host_metrics
                .send(hr_common::events::HostMetricsEvent {
                    host_id: host_id.to_string(),
                    cpu_percent: metrics.cpu_percent,
                    memory_used_bytes: metrics.memory_used_bytes,
                    memory_total_bytes: metrics.memory_total_bytes,
                });
        }
        HostAgentMessage::NetworkInterfaces(interfaces) => {
            registry
                .update_host_interfaces(host_id, interfaces.clone())
                .await;
            let mut data = load_hosts().await;
            if let Some(host) = find_host_mut(&mut data, host_id) {
                let ifaces: Vec<serde_json::Value> = interfaces
                    .iter()
                    .map(|i| json!({"ifname": i.name, "address": i.mac, "ipv4": i.ipv4, "is_up": i.is_up}))
                    .collect();
                host["interfaces"] = json!(ifaces);
                let _ = save_hosts(&data).await;
            }
        }
        HostAgentMessage::ContainerList(containers) => {
            registry.update_host_containers(host_id, containers).await;
        }
        HostAgentMessage::ImportComplete {
            transfer_id,
            container_name,
        } => {
            info!(transfer_id = %transfer_id, container = %container_name, "Host import complete");
            registry
                .on_host_import_complete(host_id, &transfer_id, &container_name)
                .await;
        }
        HostAgentMessage::ImportFailed { transfer_id, error } => {
            error!(transfer_id = %transfer_id, %error, "Host import failed");
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
            info!(request_id = %request_id, success, "Host exec result");
            registry
                .on_host_exec_result(host_id, &request_id, success, &stdout, &stderr)
                .await;
        }
        HostAgentMessage::ExportReady {
            transfer_id,
            container_name: _,
            size_bytes,
        } => {
            if let Some((target_host_id, _cname)) =
                registry.get_transfer_relay_target(&transfer_id).await
            {
                info!(transfer_id = %transfer_id, target = %target_host_id, size_bytes, "Relaying ExportReady");
                relay_transfers.insert(transfer_id.clone());
            } else if let Some(cname) = registry.take_transfer_container_name(&transfer_id).await {
                let file_path = format!("/tmp/{}.tar.gz", transfer_id);
                match tokio::fs::File::create(&file_path).await {
                    Ok(file) => {
                        active_transfers.insert(
                            transfer_id.clone(),
                            ActiveTransfer {
                                container_name: cname,
                                storage_path: "/var/lib/machines".to_string(),
                                network_mode: "bridge:br-lan".to_string(),
                                file,
                                phase: TransferPhase::ReceivingContainer,
                                workspace_file: None,
                                total_bytes: size_bytes,
                                bytes_received: 0,
                                chunk_count: 0,
                                app_id: String::new(),
                                transfer_id: transfer_id.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        error!(transfer_id = %transfer_id, %e, "Failed to create local import file");
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
        HostAgentMessage::ExportFailed { transfer_id, error } => {
            error!(transfer_id = %transfer_id, %error, "Host export failed");
            relay_transfers.remove(&transfer_id);
            registry.take_transfer_relay_target(&transfer_id).await;
            active_transfers.remove(&transfer_id);
            let _ = tokio::fs::remove_file(format!("/tmp/{}.tar.gz", transfer_id)).await;
            let _ = tokio::fs::remove_file(format!("/tmp/{}-workspace.tar.gz", transfer_id)).await;
            registry
                .on_host_export_failed(host_id, &transfer_id, &error)
                .await;
        }
        HostAgentMessage::TransferChunkBinary {
            transfer_id,
            sequence,
            size: _,
            checksum,
        } => {
            if relay_transfers.contains(&transfer_id) {
                if let Some((target_host_id, _)) =
                    registry.get_transfer_relay_target(&transfer_id).await
                {
                    let _ = registry
                        .send_host_command(
                            &target_host_id,
                            HostRegistryMessage::ReceiveChunkBinary {
                                transfer_id: transfer_id.clone(),
                                sequence,
                                size: 0,
                                checksum,
                            },
                        )
                        .await;
                }
            }
            *pending_binary_meta = Some((transfer_id, sequence, checksum));
        }
        HostAgentMessage::TransferComplete { transfer_id } => {
            if relay_transfers.remove(&transfer_id) {
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
                registry.take_transfer_relay_target(&transfer_id).await;
            } else if let Some(mut transfer) = active_transfers.remove(&transfer_id) {
                let _ = AsyncWriteExt::flush(&mut transfer.file).await;
                drop(transfer.file);
                let has_workspace = transfer.phase == TransferPhase::ReceivingWorkspace;
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
                let ws_path = format!("/tmp/{}-workspace.tar.gz", transfer_id);
                match tokio::fs::File::create(&ws_path).await {
                    Ok(ws_file) => {
                        transfer.phase = TransferPhase::ReceivingWorkspace;
                        transfer.workspace_file = Some(ws_file);
                        transfer.total_bytes = size_bytes;
                        transfer.bytes_received = 0;
                        transfer.chunk_count = 0;
                    }
                    Err(e) => {
                        error!(transfer_id = %transfer_id, %e, "Failed to create workspace file");
                    }
                }
            }
        }
        HostAgentMessage::TerminalData { session_id, data } => {
            registry.send_terminal_data(&session_id, data).await;
        }
        HostAgentMessage::TerminalOpened { session_id } => {
            debug!(session_id = %session_id, "Remote terminal opened");
        }
        HostAgentMessage::TerminalClosed {
            session_id,
            exit_code,
        } => {
            info!(session_id = %session_id, ?exit_code, "Remote terminal closed");
            registry.send_terminal_data(&session_id, Vec::new()).await;
        }
        HostAgentMessage::Auth { .. } => {}
        HostAgentMessage::NspawnContainerList(_) => {}
        HostAgentMessage::UpdateScanResult {
            os_upgradable,
            os_security,
            scan_error,
        } => {
            info!(host_id = %host_id, os_upgradable, os_security, "Host update scan result");
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
            let _ =
                state
                    .events
                    .update_scan
                    .send(hr_common::events::UpdateScanEvent::TargetScanned {
                        scan_id: String::new(),
                        target,
                    });
        }
        HostAgentMessage::BackupRepoReady { transfer_id } => {
            warn!(transfer_id = %transfer_id, "Received legacy BackupRepoReady (ignored)");
        }
        HostAgentMessage::BackupRepoComplete {
            transfer_id,
            repo_name,
            ..
        } => {
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
            if let Some((target_host_id, _)) =
                registry.get_transfer_relay_target(&transfer_id).await
            {
                if let Err(e) = registry.send_host_binary(&target_host_id, data).await {
                    error!(transfer_id = %transfer_id, %e, "Failed to relay binary chunk");
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
            let data_len = data.len() as u64;
            let target_file = match transfer.phase {
                TransferPhase::ReceivingWorkspace => transfer.workspace_file.as_mut(),
                _ => Some(&mut transfer.file),
            };
            if let Some(file) = target_file {
                if let Err(e) = AsyncWriteExt::write_all(file, &data).await {
                    error!(transfer_id = %transfer_id, %e, "Failed to write binary chunk");
                    active_transfers.remove(&transfer_id);
                    registry
                        .on_host_import_failed(
                            host_id,
                            &transfer_id,
                            &format!("File write error: {e}"),
                        )
                        .await;
                } else {
                    transfer.bytes_received += data_len;
                    transfer.chunk_count += 1;
                }
            }
        }
    } else {
        warn!("Received Binary frame without pending TransferChunkBinary metadata");
    }
}

// ── Local nspawn import ─────────────────────────────────────────────

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

    match tokio::fs::metadata(&import_path).await {
        Ok(m) if m.len() == 0 => {
            registry
                .on_host_import_failed(&source_host_id, &transfer_id, "Transfer file is empty")
                .await;
            let _ = tokio::fs::remove_file(&import_path).await;
            let _ = tokio::fs::remove_file(&ws_import_path).await;
            return;
        }
        Err(e) => {
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
        Ok(_) => {}
    }

    let rootfs_dir = format!("{}/{}", storage_path, container_name);
    let ws_dir = format!("{}/{}-workspace", storage_path, container_name);

    if let Err(e) = tokio::fs::create_dir_all(&rootfs_dir).await {
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
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
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

    if has_workspace {
        let _ = tokio::fs::create_dir_all(&ws_dir).await;
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
        match ws_extract {
            Ok(output) if output.status.success() => {}
            _ => {
                let _ = tokio::fs::create_dir_all(&ws_dir).await;
            }
        }
    }

    let sp = std::path::Path::new(&storage_path);

    if let Err(e) = hr_container::NspawnClient::write_nspawn_unit(
        &container_name,
        sp,
        &network_mode,
        has_workspace,
        &[],
    )
    .await
    {
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

    if let Err(e) =
        hr_container::NspawnClient::write_network_config(&container_name, sp, None).await
    {
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

    match hr_container::NspawnClient::start_container(&container_name).await {
        Ok(()) => {
            registry
                .on_host_import_complete("local", &transfer_id, &container_name)
                .await;
        }
        Err(e) => {
            registry
                .on_host_import_failed(&source_host_id, &transfer_id, &format!("Start failed: {e}"))
                .await;
        }
    }

    let _ = tokio::fs::remove_file(&import_path).await;
    let _ = tokio::fs::remove_file(&ws_import_path).await;
}

// ── hosts.json helpers ──────────────────────────────────────────────

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
