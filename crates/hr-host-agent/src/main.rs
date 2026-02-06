use futures_util::{SinkExt, StreamExt};
use hr_registry::protocol::{HostAgentMessage, HostMetrics, HostRegistryMessage};
use std::collections::HashMap;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

mod config;
use config::Config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hr_host_agent=debug".parse().unwrap()),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/hr-host-agent/config.toml".to_string());

    let config = match Config::load(&std::path::PathBuf::from(&config_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    info!(
        host = config.host_name,
        target = config.homeroute_url,
        "hr-host-agent starting"
    );

    let mut backoff = config.reconnect_interval_secs;

    loop {
        match run_connection(&config).await {
            Ok(()) => {
                info!("Connection closed normally");
                backoff = config.reconnect_interval_secs;
            }
            Err(e) => {
                error!("Connection error: {}", e);
                backoff = (backoff * 2).min(60);
            }
        }
        info!(secs = backoff, "Reconnecting...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
    }
}

async fn run_connection(config: &Config) -> Result<(), String> {
    let url = config.ws_url();
    info!(url, "Connecting to HomeRoute");

    let (ws_stream, _) = connect_async(&url)
        .await
        .map_err(|e| format!("WebSocket connect failed: {}", e))?;

    let (mut write, mut read) = ws_stream.split();

    // Send Auth
    let auth = HostAgentMessage::Auth {
        token: config.token.clone(),
        host_name: config.host_name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let auth_json = serde_json::to_string(&auth).map_err(|e| e.to_string())?;
    write
        .send(Message::Text(auth_json.into()))
        .await
        .map_err(|e| e.to_string())?;

    // Wait for AuthResult
    let auth_response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .map_err(|_| "Auth timeout".to_string())?
        .ok_or("Connection closed during auth")?
        .map_err(|e| format!("WebSocket error: {}", e))?;

    match auth_response {
        Message::Text(text) => {
            let msg: HostRegistryMessage =
                serde_json::from_str(&text).map_err(|e| format!("Parse auth response: {}", e))?;
            match msg {
                HostRegistryMessage::AuthResult {
                    success: true, ..
                } => {
                    info!("Authenticated successfully");
                }
                HostRegistryMessage::AuthResult {
                    success: false,
                    error,
                } => {
                    return Err(format!("Auth failed: {}", error.unwrap_or_default()));
                }
                _ => return Err("Unexpected auth response".to_string()),
            }
        }
        _ => return Err("Unexpected message type during auth".to_string()),
    }

    // Channel for outgoing messages
    let (tx, mut rx) = tokio::sync::mpsc::channel::<HostAgentMessage>(32);

    // Track active imports: transfer_id → (container_name, file)
    let mut active_imports: HashMap<String, (String, tokio::fs::File)> = HashMap::new();

    // Heartbeat task
    let tx_hb = tx.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let uptime = {
                std::fs::read_to_string("/proc/uptime")
                    .ok()
                    .and_then(|s| {
                        s.split_whitespace()
                            .next()
                            .and_then(|v| v.parse::<f64>().ok())
                    })
                    .unwrap_or(0.0) as u64
            };
            if tx_hb
                .send(HostAgentMessage::Heartbeat {
                    uptime_secs: uptime,
                    containers_running: 0,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Metrics task
    let tx_metrics = tx.clone();
    let metrics_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let metrics = collect_metrics();
            if tx_metrics
                .send(HostAgentMessage::Metrics(metrics))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Message loop
    loop {
        tokio::select! {
            // Outgoing messages
            Some(msg) = rx.recv() => {
                let text = match serde_json::to_string(&msg) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if write.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            // Incoming messages
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<HostRegistryMessage>(&text) {
                            Ok(HostRegistryMessage::Shutdown { drain }) => {
                                info!(drain, "Shutdown requested");
                                break;
                            }
                            Ok(HostRegistryMessage::StartExport { container_name, transfer_id }) => {
                                info!(container = %container_name, transfer_id = %transfer_id, "Starting export");
                                let tx_export = tx.clone();
                                let tid = transfer_id.clone();
                                let cname = container_name.clone();
                                tokio::spawn(async move {
                                    handle_export(tx_export, tid, cname).await;
                                });
                            }
                            Ok(HostRegistryMessage::StartImport { container_name, transfer_id }) => {
                                info!(container = %container_name, transfer_id = %transfer_id, "Preparing for import");
                                let path = format!("/tmp/{}.tar.gz", transfer_id);
                                match tokio::fs::File::create(&path).await {
                                    Ok(file) => {
                                        active_imports.insert(transfer_id, (container_name, file));
                                    }
                                    Err(e) => {
                                        error!("Failed to create import file {}: {}", path, e);
                                        let _ = tx.send(HostAgentMessage::ImportFailed {
                                            transfer_id,
                                            error: format!("Failed to create temp file: {}", e),
                                        }).await;
                                    }
                                }
                            }
                            Ok(HostRegistryMessage::ReceiveChunk { transfer_id, data }) => {
                                if let Some((_, file)) = active_imports.get_mut(&transfer_id) {
                                    use tokio::io::AsyncWriteExt;
                                    use base64::Engine;
                                    match base64::engine::general_purpose::STANDARD.decode(&data) {
                                        Ok(bytes) => {
                                            if let Err(e) = file.write_all(&bytes).await {
                                                error!("Failed to write chunk for {}: {}", transfer_id, e);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Base64 decode error for {}: {}", transfer_id, e);
                                        }
                                    }
                                }
                            }
                            Ok(HostRegistryMessage::TransferComplete { transfer_id }) => {
                                if let Some((container_name, mut file)) = active_imports.remove(&transfer_id) {
                                    use tokio::io::AsyncWriteExt;
                                    let _ = file.flush().await;
                                    drop(file);
                                    let tx_import = tx.clone();
                                    tokio::spawn(async move {
                                        handle_import(tx_import, transfer_id, container_name).await;
                                    });
                                }
                            }
                            Ok(msg) => {
                                warn!("Unhandled message: {:?}", msg);
                            }
                            Err(e) => {
                                warn!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    heartbeat_handle.abort();
    metrics_handle.abort();
    Ok(())
}

async fn handle_export(
    tx: tokio::sync::mpsc::Sender<HostAgentMessage>,
    transfer_id: String,
    container_name: String,
) {
    use base64::Engine;
    use tokio::io::AsyncReadExt;

    // Stop the container
    info!(container = %container_name, "Stopping container for export");
    let stop = tokio::process::Command::new("lxc")
        .args(["stop", &container_name, "--force"])
        .output()
        .await;

    if let Err(e) = stop {
        let _ = tx.send(HostAgentMessage::ExportFailed {
            transfer_id,
            error: format!("Failed to stop container: {}", e),
        }).await;
        return;
    }

    // Export the container
    let export_path = format!("/tmp/{}.tar.gz", transfer_id);
    info!(path = %export_path, "Exporting container");
    let export = tokio::process::Command::new("lxc")
        .args(["export", &container_name, &export_path])
        .output()
        .await;

    match export {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = tx.send(HostAgentMessage::ExportFailed {
                transfer_id,
                error: format!("lxc export failed: {}", stderr),
            }).await;
            return;
        }
        Err(e) => {
            let _ = tx.send(HostAgentMessage::ExportFailed {
                transfer_id,
                error: format!("Export command failed: {}", e),
            }).await;
            return;
        }
    }

    // Get file size and send ExportReady
    let metadata = match tokio::fs::metadata(&export_path).await {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.send(HostAgentMessage::ExportFailed {
                transfer_id,
                error: format!("Failed to stat export file: {}", e),
            }).await;
            return;
        }
    };

    let size_bytes = metadata.len();
    let _ = tx.send(HostAgentMessage::ExportReady {
        transfer_id: transfer_id.clone(),
        size_bytes,
    }).await;

    // Stream in 64KB chunks
    let mut file = match tokio::fs::File::open(&export_path).await {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(HostAgentMessage::ExportFailed {
                transfer_id: transfer_id.clone(),
                error: format!("Failed to open export: {}", e),
            }).await;
            return;
        }
    };

    let mut buf = vec![0u8; 65536];
    loop {
        let n = match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = tx.send(HostAgentMessage::ExportFailed {
                    transfer_id: transfer_id.clone(),
                    error: format!("Read error: {}", e),
                }).await;
                let _ = tokio::fs::remove_file(&export_path).await;
                return;
            }
        };

        let encoded = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
        if tx.send(HostAgentMessage::TransferChunk {
            transfer_id: transfer_id.clone(),
            data: encoded,
        }).await.is_err() {
            break;
        }

        // Small yield to not overwhelm the connection
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }

    let _ = tx.send(HostAgentMessage::TransferComplete {
        transfer_id: transfer_id.clone(),
    }).await;

    // Cleanup
    let _ = tokio::fs::remove_file(&export_path).await;
    info!(transfer_id = %transfer_id, "Export complete");
}

async fn handle_import(
    tx: tokio::sync::mpsc::Sender<HostAgentMessage>,
    transfer_id: String,
    container_name: String,
) {
    let import_path = format!("/tmp/{}.tar.gz", transfer_id);

    // Import the container
    info!(path = %import_path, container = %container_name, "Importing container");
    let import = tokio::process::Command::new("lxc")
        .args(["import", &import_path])
        .output()
        .await;

    match import {
        Ok(output) if output.status.success() => {
            // Start the container
            let _ = tokio::process::Command::new("lxc")
                .args(["start", &container_name])
                .output()
                .await;

            let _ = tx.send(HostAgentMessage::ImportComplete {
                transfer_id: transfer_id.clone(),
                container_name,
            }).await;
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = tx.send(HostAgentMessage::ImportFailed {
                transfer_id: transfer_id.clone(),
                error: format!("lxc import failed: {}", stderr),
            }).await;
        }
        Err(e) => {
            let _ = tx.send(HostAgentMessage::ImportFailed {
                transfer_id: transfer_id.clone(),
                error: format!("Import command failed: {}", e),
            }).await;
        }
    }

    // Cleanup
    let _ = tokio::fs::remove_file(&import_path).await;
    info!(transfer_id = %transfer_id, "Import handling complete");
}

fn collect_metrics() -> HostMetrics {
    // Read /proc/meminfo
    let (mem_total, mem_available) = {
        let content = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let mut total = 0u64;
        let mut available = 0u64;
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("MemTotal:") {
                total = val
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024;
            }
            if let Some(val) = line.strip_prefix("MemAvailable:") {
                available = val
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024;
            }
        }
        (total, available)
    };

    // Read /proc/loadavg
    let load_avg = {
        let content = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let parts: Vec<f32> = content
            .split_whitespace()
            .take(3)
            .filter_map(|s| s.parse().ok())
            .collect();
        [
            parts.first().copied().unwrap_or(0.0),
            parts.get(1).copied().unwrap_or(0.0),
            parts.get(2).copied().unwrap_or(0.0),
        ]
    };

    // Disk usage for /
    let (disk_total, disk_used) = {
        let output = std::process::Command::new("df")
            .args(["-B1", "/"])
            .output()
            .ok();
        match output {
            Some(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let line = stdout.lines().nth(1).unwrap_or("");
                let parts: Vec<&str> = line.split_whitespace().collect();
                let total = parts
                    .get(1)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                let used = parts
                    .get(2)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                (total, used)
            }
            _ => (0, 0),
        }
    };

    HostMetrics {
        cpu_percent: load_avg[0] * 100.0 / num_cpus().max(1) as f32,
        memory_used_bytes: mem_total.saturating_sub(mem_available),
        memory_total_bytes: mem_total,
        disk_used_bytes: disk_used,
        disk_total_bytes: disk_total,
        load_avg,
    }
}

fn num_cpus() -> usize {
    std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count()
        .max(1)
}
