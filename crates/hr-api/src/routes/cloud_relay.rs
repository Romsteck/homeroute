use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::ApiState;

/// Cloud relay status response.
#[derive(Serialize)]
struct RelayStatusResponse {
    enabled: bool,
    status: String, // "connected", "disconnected", "reconnecting", "error"
    vps_host: Option<String>,
    vps_ipv4: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    latency_ms: Option<u64>,
    active_streams: Option<u32>,
}

/// Cloud relay config update request.
#[derive(Deserialize)]
struct RelayConfigRequest {
    host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
}

/// Bootstrap request.
#[derive(Deserialize)]
struct BootstrapRequest {
    host: String,
    ssh_user: String,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
}

/// Relay config stored at data/cloud-relay/config.json
#[derive(Deserialize)]
struct RelayConfig {
    vps_host: String,
    vps_ipv4: String,
    ssh_user: String,
    ssh_port: u16,
    #[allow(dead_code)]
    quic_port: u16,
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(get_status))
        .route("/enable", post(enable_relay))
        .route("/disable", post(disable_relay))
        .route("/bootstrap", post(bootstrap_vps))
        .route("/config", put(update_config))
        .route("/update", post(push_update))
}

/// GET /api/cloud-relay/status
async fn get_status(State(state): State<ApiState>) -> Json<RelayStatusResponse> {
    let relay_info = state.cloud_relay_status.read().await;
    let env = &state.env;
    let enabled = *state.cloud_relay_enabled.borrow();

    // Read config.json for VPS info (may have been written by bootstrap after service start)
    let disk_config = load_relay_config(&env.data_dir).ok();

    let vps_host = env
        .cloud_relay_host
        .clone()
        .or_else(|| disk_config.as_ref().map(|c| c.vps_host.clone()));
    let vps_ipv4 = relay_info
        .as_ref()
        .and_then(|info| info.vps_ipv4.clone())
        .or_else(|| disk_config.as_ref().map(|c| c.vps_ipv4.clone()));

    Json(RelayStatusResponse {
        enabled,
        status: relay_info
            .as_ref()
            .map(|info| info.status.to_string())
            .unwrap_or_else(|| "disconnected".to_string()),
        vps_host,
        vps_ipv4,
        ssh_user: disk_config.as_ref().map(|c| c.ssh_user.clone()),
        ssh_port: disk_config.as_ref().map(|c| c.ssh_port),
        latency_ms: relay_info.as_ref().and_then(|info| info.latency_ms),
        active_streams: relay_info.as_ref().and_then(|info| info.active_streams),
    })
}

/// POST /api/cloud-relay/enable
async fn enable_relay(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let relay_config = load_relay_config(&state.env.data_dir)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Relay not configured: {}", e)))?;

    // Switch DNS to relay mode
    if let (Some(token), Some(zone_id)) = (&state.env.cf_api_token, &state.env.cf_zone_id) {
        hr_registry::cloudflare::switch_to_relay_dns(
            token,
            zone_id,
            &state.env.base_domain,
            &relay_config.vps_ipv4,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    // Update .env file and in-memory state (watch channel notifies tunnel client)
    update_env_var("CLOUD_RELAY_ENABLED", "true")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let _ = state.cloud_relay_enabled.send(true);

    // Emit event
    let _ = state
        .events
        .cloud_relay
        .send(hr_common::events::CloudRelayEvent {
            status: hr_common::events::CloudRelayStatus::Connected,
            latency_ms: None,
            active_streams: None,
            message: Some("Cloud relay enabled".to_string()),
        });

    Ok(Json(
        serde_json::json!({ "success": true, "message": "Cloud relay enabled" }),
    ))
}

/// POST /api/cloud-relay/disable
async fn disable_relay(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Switch DNS back to direct
    if let (Some(token), Some(zone_id)) = (&state.env.cf_api_token, &state.env.cf_zone_id) {
        let ipv6 = get_public_ipv6(&state.env.cf_interface)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

        hr_registry::cloudflare::switch_to_direct_dns(
            token,
            zone_id,
            &state.env.base_domain,
            &ipv6,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    // Update .env file and in-memory state (watch channel notifies tunnel client)
    update_env_var("CLOUD_RELAY_ENABLED", "false")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let _ = state.cloud_relay_enabled.send(false);

    // Emit event
    let _ = state
        .events
        .cloud_relay
        .send(hr_common::events::CloudRelayEvent {
            status: hr_common::events::CloudRelayStatus::Disconnected,
            latency_ms: None,
            active_streams: None,
            message: Some("Cloud relay disabled".to_string()),
        });

    Ok(Json(
        serde_json::json!({ "success": true, "message": "Cloud relay disabled" }),
    ))
}

/// POST /api/cloud-relay/bootstrap
async fn bootstrap_vps(
    State(state): State<ApiState>,
    Json(req): Json<BootstrapRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Emit bootstrapping event
    let _ = state
        .events
        .cloud_relay
        .send(hr_common::events::CloudRelayEvent {
            status: hr_common::events::CloudRelayStatus::Bootstrapping,
            latency_ms: None,
            active_streams: None,
            message: Some(format!("Bootstrapping VPS at {}", req.host)),
        });

    // 1. Check that hr-cloud-relay binary exists
    let binary_path = "/opt/homeroute/crates/target/release/hr-cloud-relay";
    if tokio::fs::metadata(binary_path).await.is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            "hr-cloud-relay binary not found. Run 'cargo build --release -p hr-cloud-relay' first."
                .to_string(),
        ));
    }

    // 2. Generate mTLS certificates
    let certs = hr_tunnel::crypto::generate_tunnel_certs(&req.host)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cert generation failed: {}", e)))?;

    // 3. Save client certs locally
    let relay_dir = state.env.data_dir.join("cloud-relay");
    tokio::fs::create_dir_all(&relay_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tokio::fs::write(relay_dir.join("ca.pem"), &certs.ca_cert_pem)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    tokio::fs::write(relay_dir.join("client.pem"), &certs.client_cert_pem)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    tokio::fs::write(relay_dir.join("client-key.pem"), &certs.client_key_pem)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let ssh_port = req.ssh_port.unwrap_or(22);
    let ssh_user = &req.ssh_user;
    let host = &req.host;
    let ssh_port_str = ssh_port.to_string();
    let password = req.ssh_password.as_deref();

    // 4. SCP binary + certs to VPS /tmp/
    run_scp(password, &ssh_port_str, binary_path, &format!("{}@{}:/tmp/hr-cloud-relay", ssh_user, host))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("SCP binary failed: {}", e)))?;

    // SCP certs (write to temp files, then SCP)
    let tmp_dir = std::env::temp_dir().join(format!("hr-bootstrap-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _tmp_cleanup = TmpDirCleanup(tmp_dir.clone());

    std::fs::write(tmp_dir.join("ca.pem"), &certs.ca_cert_pem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::write(tmp_dir.join("server.pem"), &certs.server_cert_pem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::write(tmp_dir.join("server-key.pem"), &certs.server_key_pem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for cert_file in ["ca.pem", "server.pem", "server-key.pem"] {
        run_scp(
            password,
            &ssh_port_str,
            tmp_dir.join(cert_file).to_str().unwrap(),
            &format!("{}@{}:/tmp/{}", ssh_user, host, cert_file),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("SCP {} failed: {}", cert_file, e)))?;
    }

    // 5. SSH: install binary, write config, create systemd unit, start
    let config_toml = r#"quic_port = 4443
tcp_listen_port = 443
http_redirect_port = 80

[tls]
ca_cert = "/etc/hr-cloud-relay/ca.pem"
server_cert = "/etc/hr-cloud-relay/server.pem"
server_key = "/etc/hr-cloud-relay/server-key.pem"
"#;

    let service_unit = r#"[Unit]
Description=HomeRoute Cloud Relay
After=network.target

[Service]
ExecStart=/usr/local/bin/hr-cloud-relay
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#;

    let setup_script = format!(
        r#"
mv /tmp/hr-cloud-relay /usr/local/bin/hr-cloud-relay && \
chmod +x /usr/local/bin/hr-cloud-relay && \
mkdir -p /etc/hr-cloud-relay && \
mv /tmp/ca.pem /etc/hr-cloud-relay/ca.pem && \
mv /tmp/server.pem /etc/hr-cloud-relay/server.pem && \
mv /tmp/server-key.pem /etc/hr-cloud-relay/server-key.pem && \
cat > /etc/hr-cloud-relay/config.toml << 'CONF'
{config_toml}CONF
cat > /etc/systemd/system/hr-cloud-relay.service << 'SVC'
{service_unit}SVC
systemctl daemon-reload && \
systemctl enable hr-cloud-relay && \
systemctl restart hr-cloud-relay
"#
    );

    let ssh_cmd = if password.is_some() {
        let escaped = setup_script.replace('\'', "'\\''");
        format!("echo '{}' | sudo -S bash -c '{}'", password.unwrap(), escaped)
    } else {
        format!("bash -c '{}'", setup_script.replace('\'', "'\\''"))
    };

    run_ssh(password, &ssh_port_str, &format!("{}@{}", ssh_user, host), &ssh_cmd)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("VPS setup failed: {}", e)))?;

    // 6. Get VPS public IPv4
    let ip_output = run_ssh_output(password, &ssh_port_str, &format!("{}@{}", ssh_user, host), "curl -4 -s ifconfig.me")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get VPS IP: {}", e)))?;

    let vps_ipv4 = ip_output.trim().to_string();

    // 7. Save relay config locally
    let relay_config = serde_json::json!({
        "vps_host": host,
        "vps_ipv4": vps_ipv4,
        "ssh_user": ssh_user,
        "ssh_port": ssh_port,
        "quic_port": 4443,
    });
    tokio::fs::write(
        relay_dir.join("config.json"),
        serde_json::to_string_pretty(&relay_config).unwrap(),
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 8. Update .env with VPS host
    update_env_var("CLOUD_RELAY_HOST", host)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    update_env_var("CLOUD_RELAY_SSH_USER", ssh_user)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    update_env_var("CLOUD_RELAY_SSH_PORT", &ssh_port.to_string())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "vps_ipv4": vps_ipv4,
        "message": format!("VPS bootstrapped successfully at {}", host),
    })))
}

/// PUT /api/cloud-relay/config
async fn update_config(
    State(_state): State<ApiState>,
    Json(req): Json<RelayConfigRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if let Some(host) = &req.host {
        update_env_var("CLOUD_RELAY_HOST", host)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }
    if let Some(user) = &req.ssh_user {
        update_env_var("CLOUD_RELAY_SSH_USER", user)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }
    if let Some(port) = req.ssh_port {
        update_env_var("CLOUD_RELAY_SSH_PORT", &port.to_string())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/cloud-relay/update — Push the local hr-cloud-relay binary to VPS via QUIC tunnel.
async fn push_update(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use sha2::{Digest, Sha256};

    // 1. Read the binary from disk
    let binary_path = "/opt/homeroute/crates/target/release/hr-cloud-relay";
    let binary_data = tokio::fs::read(binary_path)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!(
                    "hr-cloud-relay binary not found at {}. Run 'cargo build --release -p hr-cloud-relay' first. ({})",
                    binary_path, e
                ),
            )
        })?;

    // 2. Compute SHA256
    let sha256 = format!("{:x}", Sha256::digest(&binary_data));

    // 3. Send via command channel to tunnel client
    let tx = state.cloud_relay_cmd_tx.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Cloud relay command channel not available".to_string(),
        )
    })?;

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    tx.send(hr_common::events::CloudRelayCommand::PushBinaryUpdate {
        binary_data,
        sha256,
        response_tx,
    })
    .await
    .map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Tunnel client not running or channel full".to_string(),
        )
    })?;

    // 4. Wait for response from the tunnel client
    let result = response_rx.await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Tunnel client dropped the response channel".to_string(),
        )
    })?;

    match result {
        Ok(message) => Ok(Json(serde_json::json!({ "success": true, "message": message }))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

// ── Helper functions ──────────────────────────────────────────────────

fn load_relay_config(data_dir: &std::path::Path) -> Result<RelayConfig, String> {
    let path = data_dir.join("cloud-relay/config.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid relay config: {}", e))
}

/// Update a single env var in /opt/homeroute/.env
fn update_env_var(key: &str, value: &str) -> Result<(), String> {
    let env_path = std::path::Path::new("/opt/homeroute/.env");
    let content =
        std::fs::read_to_string(env_path).map_err(|e| format!("Cannot read .env: {}", e))?;

    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if let Some((k, _)) = line.split_once('=') {
                if k.trim() == key {
                    found = true;
                    return format!("{}={}", key, value);
                }
            }
            line.to_string()
        })
        .collect();

    if !found {
        lines.push(format!("{}={}", key, value));
    }

    std::fs::write(env_path, lines.join("\n") + "\n")
        .map_err(|e| format!("Cannot write .env: {}", e))
}

/// Get the public IPv6 address of a network interface.
fn get_public_ipv6(interface: &str) -> Result<String, String> {
    let output = std::process::Command::new("ip")
        .args(["-6", "addr", "show", interface, "scope", "global"])
        .output()
        .map_err(|e| format!("Failed to get IPv6: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("inet6") && !line.contains("deprecated") {
            if let Some(addr) = line.split_whitespace().nth(1) {
                if let Some(ip) = addr.split('/').next() {
                    return Ok(ip.to_string());
                }
            }
        }
    }
    Err(format!("No global IPv6 found on {}", interface))
}

/// Run an SCP command, wrapping with sshpass if a password is provided.
async fn run_scp(password: Option<&str>, port: &str, src: &str, dst: &str) -> Result<(), String> {
    let mut cmd = if let Some(pw) = password {
        let mut c = tokio::process::Command::new("sshpass");
        c.args(["-p", pw, "scp"]);
        c
    } else {
        tokio::process::Command::new("scp")
    };
    cmd.args(["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=15", "-P", port]);
    cmd.arg(src).arg(dst);

    let output = cmd.output().await.map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Run an SSH command, wrapping with sshpass if a password is provided.
async fn run_ssh(password: Option<&str>, port: &str, target: &str, command: &str) -> Result<(), String> {
    let output = run_ssh_raw(password, port, target, command).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Run an SSH command and return stdout as String.
async fn run_ssh_output(password: Option<&str>, port: &str, target: &str, command: &str) -> Result<String, String> {
    let output = run_ssh_raw(password, port, target, command).await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_ssh_raw(password: Option<&str>, port: &str, target: &str, command: &str) -> Result<std::process::Output, String> {
    let mut cmd = if let Some(pw) = password {
        let mut c = tokio::process::Command::new("sshpass");
        c.args(["-p", pw, "ssh"]);
        c
    } else {
        tokio::process::Command::new("ssh")
    };
    cmd.args(["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=15", "-p", port, target, command]);
    cmd.output().await.map_err(|e| e.to_string())
}

/// RAII guard for temp directory cleanup.
struct TmpDirCleanup(std::path::PathBuf);

impl Drop for TmpDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
