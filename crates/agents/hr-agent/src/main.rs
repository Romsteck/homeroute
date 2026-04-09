mod config;
mod connection;
mod mcp;
mod metrics;
mod proxy;
mod studio;
mod update;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info};

use hr_registry::protocol::{AgentMessage, AgentMetrics, AgentRoute, RegistryMessage};

use crate::metrics::MetricsCollector;

const CONFIG_PATH: &str = "/etc/hr-agent.toml";
const MAX_BACKOFF_SECS: u64 = 60;
const INITIAL_BACKOFF_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    // Check for MCP subcommands
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "mcp" {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .init();

        return mcp::run_mcp_server().await;
    }

    if args.len() > 1 && args[1] == "mcp-store" {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .init();

        return start_store_mcp().await;
    }

    if args.len() > 1 && args[1] == "mcp-studio" {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .init();
        return mcp::run_studio_mcp_server().await;
    }

    if args.len() > 1 && args[1] == "mcp-docs" {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .init();
        return mcp::run_docs_mcp_server().await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hr_agent=debug".parse().unwrap()),
        )
        .init();

    info!("HomeRoute Agent starting...");

    let cfg = config::AgentConfig::load(CONFIG_PATH)?;
    info!(
        service = cfg.service_name,
        homeroute = format!("{}:{}", cfg.homeroute_address, cfg.homeroute_port),
        "Config loaded"
    );

    // Create metrics collector
    let metrics_collector = Arc::new(MetricsCollector::new());

    // Agent HTTPS proxy (created once, survives reconnections)
    let agent_proxy: Arc<proxy::AgentProxy> = Arc::new(proxy::AgentProxy::new(&cfg));
    let mut proxy_started = false;

    // Shared flag for environment (set when Config is received)
    let is_dev_flag = Arc::new(AtomicBool::new(true)); // default to dev

    // Reconnection loop with exponential backoff
    let mut backoff = INITIAL_BACKOFF_SECS;

    loop {
        let (registry_tx, mut registry_rx) = mpsc::channel::<RegistryMessage>(32);
        let (outbound_tx, outbound_rx) = mpsc::channel::<AgentMessage>(64);

        info!(backoff_secs = backoff, "Connecting to HomeRoute...");

        // Ensure network interfaces are UP (fixes macvlan DOWN after container restart)
        connection::ensure_network_up(&cfg.interface).await;

        // Spawn the WebSocket connection in a task so we can process messages concurrently
        let cfg_clone = cfg.clone();
        let mut conn_handle = tokio::spawn(async move {
            connection::run_connection(&cfg_clone, registry_tx, outbound_rx).await
        });

        // Spawn metrics sender task (1 second interval)
        let metrics_tx = outbound_tx.clone();
        let metrics_coll = Arc::clone(&metrics_collector);
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;

                let memory_bytes = metrics_coll.memory_bytes().await;
                let cpu_percent = metrics_coll.cpu_percent().await;

                let metrics = AgentMetrics {
                    memory_bytes,
                    cpu_percent,
                };

                if metrics_tx.send(AgentMessage::Metrics(metrics)).await.is_err() {
                    break;
                }
            }
        });

        // Process messages while the connection is alive
        let mut connected = false;
        loop {
            tokio::select! {
                // Connection task finished
                result = &mut conn_handle => {
                    match result {
                        Ok(Ok(())) => {
                            if connected {
                                info!("Connection closed cleanly, will reconnect");
                            }
                        }
                        Ok(Err(e)) => {
                            if connected {
                                error!("Connection lost: {e}");
                            } else {
                                error!("Connection failed: {e}");
                            }
                        }
                        Err(e) => {
                            error!("Connection task panicked: {e}");
                        }
                    }
                    break;
                }
                // Incoming message from the connection
                msg = registry_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if !connected {
                                connected = true;
                                backoff = INITIAL_BACKOFF_SECS;
                            }
                            handle_registry_message(
                                &outbound_tx,
                                &agent_proxy,
                                &mut proxy_started,
                                &is_dev_flag,
                                msg
                            ).await;
                        }
                        None => {
                            // Channel closed — connection is done
                            break;
                        }
                    }
                }
            }
        }

        // Cancel background tasks
        metrics_handle.abort();

        // Drain any remaining messages
        while let Ok(msg) = registry_rx.try_recv() {
            handle_registry_message(
                &outbound_tx,
                &agent_proxy,
                &mut proxy_started,
                &is_dev_flag,
                msg
            ).await;
        }

        // Wait before reconnecting
        info!(secs = backoff, "Waiting before reconnect...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;

        // Exponential backoff (cap at MAX)
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

/// Start the Deploy MCP server — connects to registry to get app_id and environment.
/// Start the Store MCP server — connects to registry to get app_id.
async fn start_store_mcp() -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let cfg = config::AgentConfig::load(CONFIG_PATH)?;
    let url = cfg.ws_url();

    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Authenticate
    let auth_msg = AgentMessage::Auth {
        token: cfg.token.clone(),
        service_name: cfg.service_name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        ipv4_address: None,
    };
    ws_sink
        .send(Message::Text(serde_json::to_string(&auth_msg)?.into()))
        .await?;

    // Wait for auth result
    let first_msg = ws_stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("Connection closed before auth response"))??;
    let auth_result: RegistryMessage = match first_msg {
        Message::Text(text) => serde_json::from_str(&text)?,
        other => anyhow::bail!("Unexpected message type during auth: {other:?}"),
    };
    let app_id = match auth_result {
        RegistryMessage::AuthResult { success: true, app_id, .. } => {
            app_id.unwrap_or_default()
        }
        RegistryMessage::AuthResult { success: false, error, .. } => {
            anyhow::bail!("Authentication failed: {}", error.unwrap_or_default());
        }
        _ => anyhow::bail!("Unexpected message during auth handshake"),
    };

    // Build store context
    let api_base_url = format!("http://{}:{}", cfg.homeroute_address, cfg.homeroute_port);
    let ctx = mcp::StoreContext {
        app_id,
        api_base_url,
    };

    // Close the WebSocket (store MCP doesn't need ongoing WS connection)
    drop(ws_sink);

    mcp::run_store_mcp_server(ctx).await
}

async fn handle_registry_message(
    outbound_tx: &mpsc::Sender<AgentMessage>,
    agent_proxy: &Arc<proxy::AgentProxy>,
    proxy_started: &mut bool,
    is_dev_flag: &Arc<AtomicBool>,
    msg: RegistryMessage,
) {
    match msg {
        RegistryMessage::Config { base_domain, slug, frontend, environment, code_server_enabled, .. } => {
            info!("Received config from HomeRoute");

            // Write/update .mcp.json for MCP tool discovery
            is_dev_flag.store(false, Ordering::Relaxed);
            let workspace = std::path::Path::new("/root/workspace");
            if workspace.is_dir() {
                // Read the agent token for the orchestrator MCP server
                let mcp_token = config::AgentConfig::load(CONFIG_PATH)
                    .map(|c| c.token)
                    .unwrap_or_default();
                let content = mcp::generate_mcp_json(&mcp_token);
                match std::fs::write(workspace.join(".mcp.json"), &content) {
                    Ok(()) => info!("Updated /root/workspace/.mcp.json"),
                    Err(e) => tracing::debug!("Could not write .mcp.json: {e}"),
                }

                // Write/update settings.json with MCP tool permissions for plan mode
                let settings_content = mcp::generate_settings_json();
                match std::fs::write("/home/studio/.claude/settings.json", &settings_content) {
                    Ok(()) => info!("Updated /home/studio/.claude/settings.json"),
                    Err(e) => tracing::debug!("Could not write settings.json: {e}"),
                }
            }

            // Update agent proxy route table
            agent_proxy.update_routes(
                &base_domain,
                &slug,
                frontend.as_ref(),
                environment,
                code_server_enabled,
            );

            // On first Config: pull certs and start the HTTPS proxy
            if !*proxy_started {
                match agent_proxy.update_certs().await {
                    Ok(()) => {
                        agent_proxy.start();
                        agent_proxy.studio.start_ws_server();
                        *proxy_started = true;
                        info!("Agent HTTPS proxy started");
                    }
                    Err(e) => {
                        error!("Failed to pull initial certs, proxy NOT started: {e}");
                    }
                }
            }

            // Publish production route: {slug}.{base} -> port 443 (agent proxy)
            let mut routes = Vec::new();
            if frontend.is_some() {
                routes.push(AgentRoute {
                    domain: format!("{}.{}", slug, base_domain),
                    target_port: 443,
                    auth_required: false,
                    allowed_groups: vec![],
                });
            }
            if !routes.is_empty() {
                let routes_count = routes.len();
                let _ = outbound_tx.send(AgentMessage::PublishRoutes { routes }).await;
                info!("Published {} routes to HomeRoute (all port 443)", routes_count);
            }
        }

        RegistryMessage::Shutdown => {
            info!("Shutdown requested by HomeRoute");
            std::process::exit(0);
        }

        RegistryMessage::AuthResult { .. } => {
            // Handled in connection.rs
        }

        RegistryMessage::UpdateAvailable { version, download_url, sha256 } => {
            info!(version, download_url, "Update available, starting auto-update");
            if let Err(e) = update::apply_update(&download_url, &sha256, &version).await {
                error!("Auto-update failed: {e}");
            }
        }

        RegistryMessage::CertRenewal { slug } => {
            info!(slug, "Certificate renewal notification, re-pulling certs");
            let proxy = Arc::clone(agent_proxy);
            tokio::spawn(async move {
                if let Err(e) = proxy.update_certs().await {
                    error!("Failed to update certs after renewal: {e}");
                }
            });
        }

        RegistryMessage::UpdateRules { rules } => {
            info!("Received UpdateRules ({} files)", rules.len());
            let rules_dir = std::path::Path::new("/root/workspace/.claude/rules");
            if let Err(e) = std::fs::create_dir_all(rules_dir) {
                error!("Failed to create rules directory: {e}");
                return;
            }
            for (filename, content) in rules {
                match std::fs::write(rules_dir.join(&filename), &content) {
                    Ok(()) => info!(filename = %filename, "Updated rule file"),
                    Err(e) => error!(filename = %filename, error = %e, "Failed to write rule file"),
                }
            }
        }

    }
}

