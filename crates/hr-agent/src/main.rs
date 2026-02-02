mod config;
mod connection;
mod ipv6;
mod proxy;
mod update;

use std::net::Ipv6Addr;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use hr_registry::protocol::RegistryMessage;

const CONFIG_PATH: &str = "/etc/hr-agent.toml";
const MAX_BACKOFF_SECS: u64 = 60;
const INITIAL_BACKOFF_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hr_agent=debug".parse().unwrap()),
        )
        .init();

    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("HomeRoute Agent starting...");

    let cfg = config::AgentConfig::load(CONFIG_PATH)?;
    info!(
        service = cfg.service_name,
        homeroute = format!("[{}]:{}", cfg.homeroute_address, cfg.homeroute_port),
        "Config loaded"
    );

    // Create the proxy (not yet listening — needs routes from HomeRoute)
    let mut agent_proxy = proxy::AgentProxy::new()?;

    // Current assigned IPv6 address (if any)
    let mut current_ipv6: Option<String> = None;
    // Proxy task handle
    let mut proxy_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Reconnection loop with exponential backoff
    let mut backoff = INITIAL_BACKOFF_SECS;

    loop {
        let (registry_tx, mut registry_rx) = mpsc::channel::<RegistryMessage>(32);

        info!(backoff_secs = backoff, "Connecting to HomeRoute...");

        // Spawn the WebSocket connection in a task so we can process messages concurrently
        let cfg_clone = cfg.clone();
        let mut conn_handle = tokio::spawn(async move {
            connection::run_connection(&cfg_clone, registry_tx).await
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
                            handle_registry_message(&cfg, &mut agent_proxy, &mut current_ipv6, &mut proxy_handle, msg).await;
                        }
                        None => {
                            // Channel closed — connection is done
                            break;
                        }
                    }
                }
            }
        }

        // Drain any remaining messages
        while let Ok(msg) = registry_rx.try_recv() {
            handle_registry_message(&cfg, &mut agent_proxy, &mut current_ipv6, &mut proxy_handle, msg).await;
        }

        // Wait before reconnecting
        info!(secs = backoff, "Waiting before reconnect...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;

        // Exponential backoff (cap at MAX)
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

async fn handle_registry_message(
    cfg: &config::AgentConfig,
    proxy: &mut proxy::AgentProxy,
    current_ipv6: &mut Option<String>,
    proxy_handle: &mut Option<tokio::task::JoinHandle<()>>,
    msg: RegistryMessage,
) {
    match msg {
        RegistryMessage::Config {
            ipv6_address,
            routes,
            homeroute_auth_url,
            ..
        } => {
            info!(
                routes = routes.len(),
                ipv6 = ipv6_address,
                "Received full config from HomeRoute"
            );

            // Apply IPv6 address
            if !ipv6_address.is_empty() {
                apply_ipv6(cfg, current_ipv6, &ipv6_address).await;
            }

            // Apply routes to proxy
            if let Err(e) = proxy.apply_routes(&routes, &homeroute_auth_url) {
                error!("Failed to apply routes: {e}");
                return;
            }

            // Start or restart the proxy if we have an IPv6 address
            if let Some(addr_str) = current_ipv6.as_deref() {
                // Wait briefly for the IPv6 address to pass DAD and become available
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                start_proxy(proxy, proxy_handle, addr_str).await;
            }
        }

        RegistryMessage::Ipv6Update { ipv6_address } => {
            info!(ipv6 = ipv6_address, "IPv6 address updated");
            apply_ipv6(cfg, current_ipv6, &ipv6_address).await;

            // Restart proxy on new address
            if let Some(addr_str) = current_ipv6.as_deref() {
                start_proxy(proxy, proxy_handle, addr_str).await;
            }
        }

        RegistryMessage::CertUpdate { routes } => {
            info!(routes = routes.len(), "Certificate update received");
            let auth_url = String::new(); // Keep existing auth_url
            if let Err(e) = proxy.apply_routes(&routes, &auth_url) {
                error!("Failed to apply cert update: {e}");
            }
        }

        RegistryMessage::Shutdown => {
            info!("Shutdown requested by HomeRoute");
            proxy.shutdown();
            if let Some(handle) = proxy_handle.take() {
                handle.abort();
            }
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
    }
}

async fn apply_ipv6(
    cfg: &config::AgentConfig,
    current_ipv6: &mut Option<String>,
    new_addr: &str,
) {
    // Remove old address if different
    if let Some(old) = current_ipv6.as_ref() {
        if old != new_addr {
            if let Err(e) = ipv6::remove_address(&cfg.interface, old).await {
                warn!("Failed to remove old IPv6: {e}");
            }
        }
    }

    // Add new address
    if let Err(e) = ipv6::add_address(&cfg.interface, new_addr).await {
        error!("Failed to add IPv6 {new_addr}: {e}");
        return;
    }

    *current_ipv6 = Some(new_addr.to_string());
}

async fn start_proxy(
    proxy: &mut proxy::AgentProxy,
    proxy_handle: &mut Option<tokio::task::JoinHandle<()>>,
    addr_str: &str,
) {
    // Stop existing proxy
    proxy.shutdown();
    if let Some(handle) = proxy_handle.take() {
        handle.abort();
        // Give the old listener a moment to release the port
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let bind_addr: Ipv6Addr = match addr_str.parse() {
        Ok(a) => a,
        Err(e) => {
            error!("Invalid IPv6 address {addr_str}: {e}");
            return;
        }
    };

    info!(addr = addr_str, "Starting HTTPS proxy on [{}]:443", addr_str);

    // Get the shared components needed for the spawned task
    let listener_handle = proxy.spawn_listener(bind_addr);
    match listener_handle {
        Ok(handle) => {
            *proxy_handle = Some(handle);
        }
        Err(e) => {
            error!("Failed to start proxy listener: {e}");
        }
    }
}
