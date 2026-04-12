mod handler;

use hr_adblock::AdblockEngine;
use hr_common::config::EnvConfig;
use hr_common::service_registry::{
    ServicePriorityLevel, ServiceState, ServiceStatus, new_service_registry, now_millis,
};
use hr_common::supervisor::{ServicePriority, spawn_supervised};
use hr_dns::DnsState;
use signal_hook::consts::{SIGHUP, SIGTERM};
use signal_hook_tokio::Signals;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};
use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Combined config from dns-dhcp-config.json (matches the original file layout)
#[derive(serde::Deserialize, Default)]
struct DnsDhcpConfig {
    #[serde(default)]
    dns: hr_dns::DnsConfig,
    #[serde(default)]
    dhcp: hr_dhcp::DhcpConfig,
    #[serde(default)]
    ipv6: hr_ipv6::Ipv6Config,
    #[serde(default)]
    adblock: hr_adblock::config::AdblockConfig,
}

impl DnsDhcpConfig {
    fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            info!("No config file at {}, using defaults", path.display());
            Ok(Self::default())
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize centralized log store
    let log_store = std::sync::Arc::new(
        hr_common::logging::LogStore::new(std::path::Path::new("/opt/homeroute/data/logs.db"))
            .expect("Failed to init log store"),
    );

    // Initialize logging with custom layer
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,hr_netcore=debug".parse().unwrap()),
            ),
        )
        .with(hr_common::logging::LoggingLayer::new(
            log_store.clone(),
            hr_common::logging::LogService::Netcore,
        ))
        .init();

    info!("hr-netcore starting...");

    // Spawn log flush task
    {
        let flush_store = log_store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if let Err(e) = flush_store.flush_to_db().await {
                    eprintln!("Log flush error: {e}");
                }
            }
        });
    }
    // Spawn log compaction task
    {
        let compact_store = log_store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                if let Err(e) = compact_store.compact().await {
                    eprintln!("Log compaction error: {e}");
                }
            }
        });
    }

    // Load environment config
    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    // Initialize service registry
    let service_registry = new_service_registry();

    // ── Load DNS/DHCP/IPv6/Adblock config ──────────────────────────────

    let dns_dhcp_config = DnsDhcpConfig::load(&env.dns_dhcp_config_path)?;

    info!(
        "Config loaded: DNS port {}, DHCP {}, Adblock {}, IPv6 {}",
        dns_dhcp_config.dns.port,
        if dns_dhcp_config.dhcp.enabled {
            "enabled"
        } else {
            "disabled"
        },
        if dns_dhcp_config.adblock.enabled {
            "enabled"
        } else {
            "disabled"
        },
        if dns_dhcp_config.ipv6.enabled {
            "enabled"
        } else {
            "disabled"
        },
    );

    // ── Initialize adblock engine ──────────────────────────────────────

    let mut adblock_engine = AdblockEngine::new();
    adblock_engine.set_whitelist(dns_dhcp_config.adblock.whitelist.clone());

    if dns_dhcp_config.adblock.enabled {
        let cache_path = PathBuf::from(&dns_dhcp_config.adblock.data_dir).join("domains.json");
        match hr_adblock::sources::load_cache(&cache_path) {
            Ok(domains) => {
                info!("Loaded {} blocked domains from cache", domains.len());
                adblock_engine.set_blocked(domains);
            }
            Err(_) => {
                info!("No adblock cache found, will download on startup");
            }
        }
    }

    let adblock = Arc::new(RwLock::new(adblock_engine));

    // ── Initialize DHCP state ──────────────────────────────────────────

    let server_ip: Ipv4Addr = dns_dhcp_config
        .dhcp
        .gateway
        .parse()
        .unwrap_or(Ipv4Addr::UNSPECIFIED);

    let mut lease_store = hr_dhcp::LeaseStore::new(&dns_dhcp_config.dhcp.lease_file);
    if let Err(e) = lease_store.load_from_file() {
        warn!("Failed to load lease file: {}", e);
    }

    let dhcp_state: hr_dhcp::SharedDhcpState = Arc::new(RwLock::new(hr_dhcp::DhcpState {
        config: dns_dhcp_config.dhcp.clone(),
        lease_store,
        server_ip,
    }));

    // Separate LeaseStore for DNS resolver (synced from DHCP state every 10s).
    // DhcpState owns its LeaseStore directly; DnsState needs Arc<RwLock<LeaseStore>>.
    // A background sync task keeps them in sync.
    let lease_store_for_dns: Arc<RwLock<hr_dhcp::LeaseStore>> = {
        let mut shared_lease_store = hr_dhcp::LeaseStore::new(&dns_dhcp_config.dhcp.lease_file);
        if let Err(e) = shared_lease_store.load_from_file() {
            warn!("Failed to load lease file for DNS: {}", e);
        }
        Arc::new(RwLock::new(shared_lease_store))
    };

    // ── Initialize DNS state ───────────────────────────────────────────

    let dns_cache = hr_dns::cache::DnsCache::new(dns_dhcp_config.dns.cache_size);

    let upstream = hr_dns::upstream::UpstreamForwarder::new(
        dns_dhcp_config.dns.upstream_servers.clone(),
        dns_dhcp_config.dns.upstream_timeout_ms,
    );

    let query_logger = if !dns_dhcp_config.dns.query_log_path.is_empty() {
        Some(hr_dns::logging::QueryLogger::new(
            &dns_dhcp_config.dns.query_log_path,
        ))
    } else {
        None
    };

    let dns_state: hr_dns::SharedDnsState = Arc::new(RwLock::new(DnsState {
        config: dns_dhcp_config.dns.clone(),
        dns_cache,
        upstream,
        query_logger,
        adblock: adblock.clone(),
        lease_store: lease_store_for_dns.clone(),
        adblock_enabled: dns_dhcp_config.adblock.enabled,
        adblock_block_response: dns_dhcp_config.adblock.block_response.clone(),
    }));

    // ── Spawn supervised services ──────────────────────────────────────

    info!("Starting supervised network services...");

    // DNS UDP + TCP server (Critical)
    // Each listen address gets a unique service name to avoid status overwrites
    for addr_str in &dns_dhcp_config.dns.listen_addresses {
        // IPv6 addresses need brackets: [addr]:port
        let addr_formatted = if addr_str.contains(':') {
            format!("[{}]:{}", addr_str, dns_dhcp_config.dns.port)
        } else {
            format!("{}:{}", addr_str, dns_dhcp_config.dns.port)
        };
        let addr: SocketAddr = match addr_formatted.parse() {
            Ok(a) => a,
            Err(e) => {
                warn!("Skipping invalid listen address '{}': {}", addr_str, e);
                continue;
            }
        };

        // Quick bind check: skip addresses not available on this host (EADDRNOTAVAIL)
        // to avoid the supervisor endlessly retrying a permanently unavailable address.
        match tokio::net::UdpSocket::bind(addr).await {
            Ok(_sock) => { /* address is available, drop test socket */ }
            Err(e) if e.raw_os_error() == Some(99) => {
                warn!(
                    "Skipping DNS listen address {} (not available on this host)",
                    addr_str
                );
                continue;
            }
            Err(_) => { /* other errors (e.g. port in use) — let supervisor handle */ }
        }

        let udp_name = format!("dns-udp:{}", addr_str);
        let tcp_name = format!("dns-tcp:{}", addr_str);

        let dns_state_c = dns_state.clone();
        let reg = service_registry.clone();
        spawn_supervised(&udp_name, ServicePriority::Critical, reg, move || {
            let state = dns_state_c.clone();
            let addr = addr;
            async move { hr_dns::server::run_udp_server(addr, state).await }
        });

        let dns_state_c = dns_state.clone();
        let reg = service_registry.clone();
        spawn_supervised(&tcp_name, ServicePriority::Critical, reg, move || {
            let state = dns_state_c.clone();
            let addr = addr;
            async move { hr_dns::server::run_tcp_server(addr, state).await }
        });
    }

    // DHCP server (Critical)
    if dns_dhcp_config.dhcp.enabled {
        let dhcp_state_c = dhcp_state.clone();
        let reg = service_registry.clone();
        spawn_supervised("dhcp", ServicePriority::Critical, reg, move || {
            let state = dhcp_state_c.clone();
            async move { hr_dhcp::server::run_dhcp_server(state).await }
        });
    } else {
        let mut reg = service_registry.write().await;
        reg.insert(
            "dhcp".into(),
            ServiceStatus {
                name: "dhcp".into(),
                state: ServiceState::Disabled,
                priority: ServicePriorityLevel::Critical,
                restart_count: 0,
                last_state_change: now_millis(),
                error: None,
            },
        );
        drop(reg);
    }

    // ── IPv6 Prefix Delegation + RA ─────────────────────────────────

    // Watch channel: PD client -> RA sender
    let (prefix_tx, prefix_rx) = tokio::sync::watch::channel::<Option<hr_ipv6::PrefixInfo>>(None);

    // 1) DHCPv6-PD client (obtains /56 from upstream, publishes /64 on channel)
    if dns_dhcp_config.ipv6.enabled && dns_dhcp_config.ipv6.pd_enabled {
        let ipv6_config = dns_dhcp_config.ipv6.clone();
        let tx = prefix_tx.clone();
        let reg = service_registry.clone();
        spawn_supervised("ipv6-pd", ServicePriority::Important, reg, move || {
            let config = ipv6_config.clone();
            let tx = tx.clone();
            async move { hr_ipv6::pd_client::run_pd_client(config, tx).await }
        });
    } else {
        let mut reg = service_registry.write().await;
        reg.insert(
            "ipv6-pd".into(),
            ServiceStatus {
                name: "ipv6-pd".into(),
                state: ServiceState::Disabled,
                priority: ServicePriorityLevel::Important,
                restart_count: 0,
                last_state_change: now_millis(),
                error: None,
            },
        );
        drop(reg);
    }

    // 2) IPv6 RA sender (announces ULA + GUA prefixes)
    if dns_dhcp_config.ipv6.enabled && dns_dhcp_config.ipv6.ra_enabled {
        let ipv6_config = dns_dhcp_config.ipv6.clone();
        let rx = prefix_rx.clone();
        let reg = service_registry.clone();
        spawn_supervised("ipv6-ra", ServicePriority::Important, reg, move || {
            let config = ipv6_config.clone();
            let rx = rx.clone();
            async move { hr_ipv6::ra::run_ra_sender(config, rx).await }
        });
    } else {
        let mut reg = service_registry.write().await;
        reg.insert(
            "ipv6-ra".into(),
            ServiceStatus {
                name: "ipv6-ra".into(),
                state: ServiceState::Disabled,
                priority: ServicePriorityLevel::Important,
                restart_count: 0,
                last_state_change: now_millis(),
                error: None,
            },
        );
        drop(reg);
    }

    // ── Background tasks ───────────────────────────────────────────────

    // Lease persistence + expired lease purge (every 60s)
    {
        let dhcp_state_c = dhcp_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let mut s = dhcp_state_c.write().await;
                let purged = s.lease_store.purge_expired();
                if purged > 0 {
                    info!("Purged {} expired DHCP leases", purged);
                }
                if let Err(e) = s.lease_store.save_to_file() {
                    warn!("Failed to save lease file: {}", e);
                }
            }
        });
    }

    // Sync DHCP leases -> DNS lease store (every 10s)
    {
        let dhcp_state_c = dhcp_state.clone();
        let lease_store_dns = lease_store_for_dns.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                // Copy all leases from DHCP state to DNS lease store
                let dhcp_read = dhcp_state_c.read().await;
                let all_leases: Vec<_> = dhcp_read
                    .lease_store
                    .all_leases()
                    .into_iter()
                    .cloned()
                    .collect();
                drop(dhcp_read);

                let mut dns_ls = lease_store_dns.write().await;
                // Rebuild: clear and re-add all (cheap, ~100 entries max)
                for lease in all_leases {
                    dns_ls.add_lease(lease);
                }
            }
        });
    }

    // DNS cache purge (every 30s)
    {
        let dns_state_c = dns_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let s = dns_state_c.read().await;
                let purged = s.dns_cache.purge_expired().await;
                if purged > 0 {
                    info!("Purged {} expired DNS cache entries", purged);
                }
            }
        });
    }

    // Adblock initial download + auto-update
    if dns_dhcp_config.adblock.enabled {
        let adblock_c = adblock.clone();
        let sources = dns_dhcp_config.adblock.sources.clone();
        let data_dir = dns_dhcp_config.adblock.data_dir.clone();
        let dns_state_c = dns_state.clone();
        tokio::spawn(async move {
            // Initial download after 5s delay
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            info!("Starting initial adblock list download...");
            do_adblock_update(&adblock_c, &sources, &data_dir, &dns_state_c).await;
        });

        if dns_dhcp_config.adblock.auto_update_hours > 0 {
            let adblock_c = adblock.clone();
            let sources = dns_dhcp_config.adblock.sources.clone();
            let data_dir = dns_dhcp_config.adblock.data_dir.clone();
            let dns_state_c = dns_state.clone();
            let hours = dns_dhcp_config.adblock.auto_update_hours;
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(hours * 3600);
                loop {
                    tokio::time::sleep(interval).await;
                    info!("Running scheduled adblock update...");
                    do_adblock_update(&adblock_c, &sources, &data_dir, &dns_state_c).await;
                }
            });
        }
    }

    // ── SIGHUP handler (DNS/DHCP/Adblock reload only) ──────────────────

    let dns_state_reload = dns_state.clone();
    let adblock_reload = adblock.clone();
    let dns_dhcp_config_path = env.dns_dhcp_config_path.clone();

    tokio::spawn(async move {
        if let Err(e) = handle_sighup(dns_dhcp_config_path, dns_state_reload, adblock_reload).await
        {
            error!("SIGHUP handler error: {}", e);
        }
    });

    // ── IPC server ─────────────────────────────────────────────────────

    let ipc_handler = Arc::new(handler::NetcoreHandler {
        dns_state: dns_state.clone(),
        dhcp_state: dhcp_state.clone(),
        adblock: adblock.clone(),
        service_registry: service_registry.clone(),
        dns_dhcp_config_path: env.dns_dhcp_config_path.clone(),
    });

    let socket_path = PathBuf::from("/run/hr-netcore.sock");
    let socket_path_c = socket_path.clone();
    tokio::spawn(async move {
        if let Err(e) = hr_ipc::server::run_ipc_server(&socket_path_c, ipc_handler).await {
            error!("IPC server error: {}", e);
        }
    });

    // ── Ready ──────────────────────────────────────────────────────────

    info!("hr-netcore started successfully");
    info!("  DNS: listening on port {}", dns_dhcp_config.dns.port);
    info!(
        "  DHCP: {}",
        if dns_dhcp_config.dhcp.enabled {
            "listening on port 67"
        } else {
            "disabled"
        }
    );
    info!(
        "  IPv6: {}",
        if dns_dhcp_config.ipv6.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    info!(
        "  Adblock: {} domains blocked",
        adblock.read().await.domain_count()
    );
    info!("  IPC: {}", socket_path.display());

    // Wait for shutdown signal (SIGINT or SIGTERM from systemd)
    let mut shutdown_signals = Signals::new([SIGTERM])?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        Some(_) = shutdown_signals.next() => {},
    }
    info!("Shutting down...");

    // Save leases on shutdown
    {
        let s = dhcp_state.read().await;
        if let Err(e) = s.lease_store.save_to_file() {
            error!("Failed to save leases on shutdown: {}", e);
        } else {
            info!("Leases saved successfully");
        }
    }

    // Clean up IPC socket
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

// ── SIGHUP handler (DNS/DHCP/Adblock only, no proxy) ───────────────────

async fn handle_sighup(
    dns_dhcp_config_path: PathBuf,
    dns_state: hr_dns::SharedDnsState,
    adblock: Arc<RwLock<AdblockEngine>>,
) -> anyhow::Result<()> {
    let mut signals = Signals::new([SIGHUP])?;

    while let Some(signal) = signals.next().await {
        if signal == SIGHUP {
            info!("Received SIGHUP, reloading DNS/DHCP/Adblock config...");

            // Reload DNS/DHCP config
            match DnsDhcpConfig::load(&dns_dhcp_config_path) {
                Ok(new_config) => {
                    let mut s = dns_state.write().await;
                    s.upstream = hr_dns::upstream::UpstreamForwarder::new(
                        new_config.dns.upstream_servers.clone(),
                        new_config.dns.upstream_timeout_ms,
                    );
                    s.config = new_config.dns;
                    s.adblock_enabled = new_config.adblock.enabled;
                    s.adblock_block_response = new_config.adblock.block_response;
                    s.dns_cache.clear().await;

                    let mut ab = adblock.write().await;
                    ab.set_whitelist(new_config.adblock.whitelist);

                    info!("DNS/DHCP/Adblock config reloaded");
                }
                Err(e) => {
                    error!("Failed to reload DNS/DHCP config: {}", e);
                }
            }
        }
    }

    Ok(())
}

// ── Adblock update ─────────────────────────────────────────────────────

async fn do_adblock_update(
    adblock: &Arc<RwLock<AdblockEngine>>,
    sources: &[hr_adblock::config::AdblockSource],
    data_dir: &str,
    _dns_state: &hr_dns::SharedDnsState,
) {
    let (domains, _results) = hr_adblock::sources::download_all(sources).await;
    let count = domains.len();

    {
        let mut ab = adblock.write().await;
        ab.set_blocked(domains.clone());
    }

    let cache_path = PathBuf::from(data_dir).join("domains.json");
    if let Err(e) = hr_adblock::sources::save_cache(&domains, &cache_path) {
        warn!("Failed to save adblock cache: {}", e);
    }

    info!("Adblock update complete: {} unique domains blocked", count);
}
