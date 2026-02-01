mod adblock;
mod config;
mod dhcp;
mod dns;
mod ipv6;
mod logging;
mod shared;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};
use signal_hook::consts::SIGHUP;
use signal_hook_tokio::Signals;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

use crate::adblock::AdblockEngine;
use crate::config::Config;
use crate::dhcp::lease_store::LeaseStore;
use crate::dns::cache::DnsCache;
use crate::dns::upstream::UpstreamForwarder;
use crate::logging::QueryLogger;
use crate::shared::{ServerState, SharedState};

fn config_path() -> PathBuf {
    PathBuf::from(
        std::env::var("DNS_DHCP_CONFIG_PATH")
            .unwrap_or_else(|_| "/var/lib/server-dashboard/dns-dhcp-config.json".to_string()),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_dns_dhcp=info".parse().unwrap()),
        )
        .init();

    info!("rust-dns-dhcp starting...");

    // Load config
    let path = config_path();
    let config = if path.exists() {
        Config::load_from_file(&path)
            .with_context(|| format!("Failed to load config from {}", path.display()))?
    } else {
        info!("No config file found at {}, using defaults", path.display());
        Config::default()
    };

    info!("Config loaded: DNS port {}, DHCP {}, Adblock {}, IPv6 {}",
        config.dns.port,
        if config.dhcp.enabled { "enabled" } else { "disabled" },
        if config.adblock.enabled { "enabled" } else { "disabled" },
        if config.ipv6.enabled { "enabled" } else { "disabled" },
    );

    // Initialize components
    let dns_cache = DnsCache::new(config.dns.cache_size);

    let mut lease_store = LeaseStore::new(&config.dhcp.lease_file);
    if let Err(e) = lease_store.load_from_file() {
        warn!("Failed to load lease file: {}", e);
    }

    let upstream = UpstreamForwarder::new(
        config.dns.upstream_servers.clone(),
        config.dns.upstream_timeout_ms,
    );

    let mut adblock_engine = AdblockEngine::new();
    adblock_engine.set_whitelist(config.adblock.whitelist.clone());

    // Load adblock cache if available
    if config.adblock.enabled {
        let cache_path = PathBuf::from(&config.adblock.data_dir).join("domains.json");
        match adblock::sources::load_cache(&cache_path) {
            Ok(domains) => {
                info!("Loaded {} blocked domains from cache", domains.len());
                adblock_engine.set_blocked(domains);
            }
            Err(_) => {
                info!("No adblock cache found, will download on startup");
            }
        }
    }

    let query_logger = if !config.dns.query_log_path.is_empty() {
        Some(QueryLogger::new(&config.dns.query_log_path))
    } else {
        None
    };

    // Build shared state
    let state: SharedState = Arc::new(RwLock::new(ServerState {
        config: config.clone(),
        dns_cache,
        lease_store,
        adblock: adblock_engine,
        query_logger,
        upstream,
    }));

    // Spawn SIGHUP handler for hot-reload
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = handle_sighup(state_clone).await {
            error!("SIGHUP handler error: {}", e);
        }
    });

    // Spawn DNS UDP servers
    for addr_str in &config.dns.listen_addresses {
        let addr: SocketAddr = format!("{}:{}", addr_str, config.dns.port)
            .parse()
            .with_context(|| format!("Invalid listen address: {}:{}", addr_str, config.dns.port))?;

        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = dns::server::run_udp_server(addr, state_clone).await {
                error!("DNS UDP server on {} failed: {}", addr, e);
            }
        });

        // Also spawn TCP server on the same address
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = dns::server::run_tcp_server(addr, state_clone).await {
                error!("DNS TCP server on {} failed: {}", addr, e);
            }
        });
    }

    // Spawn DHCP server
    if config.dhcp.enabled {
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = dhcp::server::run_dhcp_server(state_clone).await {
                error!("DHCP server failed: {}", e);
            }
        });
    }

    // Spawn IPv6 RA sender
    if config.ipv6.enabled && config.ipv6.ra_enabled {
        let ipv6_config = config.ipv6.clone();
        tokio::spawn(async move {
            if let Err(e) = ipv6::ra::run_ra_sender(ipv6_config).await {
                error!("RA sender failed: {}", e);
            }
        });
    }

    // Spawn DHCPv6 server
    if config.ipv6.enabled && config.ipv6.dhcpv6_enabled {
        let ipv6_config = config.ipv6.clone();
        tokio::spawn(async move {
            if let Err(e) = ipv6::dhcpv6::run_dhcpv6_server(ipv6_config).await {
                error!("DHCPv6 server failed: {}", e);
            }
        });
    }

    // Spawn adblock API server
    if config.adblock.enabled {
        let api_port = config.adblock.api_port;
        let state_clone = state.clone();
        tokio::spawn(async move {
            let router = adblock::api::build_router(state_clone);
            let addr: SocketAddr = format!("127.0.0.1:{}", api_port).parse().unwrap();
            info!("Adblock API listening on {}", addr);
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            if let Err(e) = axum::serve(listener, router).await {
                error!("Adblock API server failed: {}", e);
            }
        });

        // Trigger initial adblock download in background
        let state_clone = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            info!("Starting initial adblock list download...");
            do_adblock_update(&state_clone).await;
        });

        // Spawn auto-update timer
        if config.adblock.auto_update_hours > 0 {
            let hours = config.adblock.auto_update_hours;
            let state_clone = state.clone();
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(hours * 3600);
                loop {
                    tokio::time::sleep(interval).await;
                    info!("Running scheduled adblock update...");
                    do_adblock_update(&state_clone).await;
                }
            });
        }
    }

    // Spawn lease persistence timer (save every 60s)
    {
        let state_clone = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let s = state_clone.read().await;
                if let Err(e) = s.lease_store.save_to_file() {
                    warn!("Failed to save lease file: {}", e);
                }
            }
        });
    }

    // Spawn DNS cache purge timer (every 30s)
    {
        let state_clone = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let s = state_clone.read().await;
                let purged = s.dns_cache.purge_expired().await;
                if purged > 0 {
                    info!("Purged {} expired DNS cache entries", purged);
                }
            }
        });
    }

    info!("rust-dns-dhcp started successfully");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Save leases on shutdown
    {
        let s = state.read().await;
        if let Err(e) = s.lease_store.save_to_file() {
            error!("Failed to save leases on shutdown: {}", e);
        } else {
            info!("Leases saved successfully");
        }
    }

    Ok(())
}

async fn handle_sighup(state: SharedState) -> Result<()> {
    let mut signals = Signals::new([SIGHUP])?;

    while let Some(signal) = signals.next().await {
        if signal == SIGHUP {
            info!("Received SIGHUP, reloading config...");

            let path = config_path();
            match Config::load_from_file(&path) {
                Ok(new_config) => {
                    let mut s = state.write().await;
                    s.upstream = UpstreamForwarder::new(
                        new_config.dns.upstream_servers.clone(),
                        new_config.dns.upstream_timeout_ms,
                    );
                    s.adblock.set_whitelist(new_config.adblock.whitelist.clone());
                    s.dns_cache.clear().await;
                    s.config = new_config;
                    info!("Config reloaded successfully");
                }
                Err(e) => {
                    error!("Failed to reload config: {}", e);
                }
            }
        }
    }

    Ok(())
}

async fn do_adblock_update(state: &SharedState) {
    let sources = {
        let s = state.read().await;
        s.config.adblock.sources.clone()
    };

    let (domains, _results) = adblock::sources::download_all(&sources).await;
    let count = domains.len();

    {
        let mut s = state.write().await;
        s.adblock.set_blocked(domains.clone());

        let cache_path = PathBuf::from(&s.config.adblock.data_dir).join("domains.json");
        if let Err(e) = adblock::sources::save_cache(&domains, &cache_path) {
            warn!("Failed to save adblock cache: {}", e);
        }
    }

    info!("Adblock update complete: {} unique domains blocked", count);
}
