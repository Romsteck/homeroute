use hr_auth::AuthService;
use hr_acme::{AcmeConfig, AcmeManager};
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_common::service_registry::new_service_registry;
use hr_common::supervisor::{spawn_supervised, ServicePriority};
use hr_ipc::{NetcoreClient, EdgeClient};
use hr_ipc::orchestrator::OrchestratorClient;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,homeroute=debug".parse().unwrap()),
        )
        .init();

    info!("HomeRoute starting...");

    // Install rustls crypto provider (needed by ACME/auth)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Load environment config
    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    // Initialize event bus (local, for WebSocket events to frontend)
    let events = Arc::new(EventBus::new());

    // Initialize service registry
    let service_registry = new_service_registry();

    // Initialize auth service (needed for session cookies in hr-api)
    let auth = AuthService::new(&env.auth_data_dir, &env.base_domain)?;
    auth.start_cleanup_task();
    info!("Auth service initialized");

    // Initialize ACME (read-only, for list_certificates in API routes)
    let acme_config = AcmeConfig {
        storage_path: env.acme_storage_path.to_string_lossy().to_string(),
        cf_api_token: env.cf_api_token.clone().unwrap_or_default(),
        cf_zone_id: env.cf_zone_id.clone().unwrap_or_default(),
        base_domain: env.base_domain.clone(),
        directory_url: if env.acme_staging {
            "https://acme-staging-v02.api.letsencrypt.org/directory".to_string()
        } else {
            "https://acme-v02.api.letsencrypt.org/directory".to_string()
        },
        account_email: env.acme_email.clone()
            .unwrap_or_else(|| format!("admin@{}", env.base_domain)),
        renewal_threshold_days: 30,
    };
    let acme = Arc::new(AcmeManager::new(acme_config));
    acme.init().await?;
    info!(
        "ACME manager initialized ({})",
        if acme.is_initialized() { "account loaded" } else { "new account created" }
    );

    // ── IPC clients ───────────────────────────────────────────────────

    let netcore = Arc::new(NetcoreClient::new("/run/hr-netcore.sock"));
    info!("Netcore IPC client configured (socket: {})", netcore.socket_path().display());

    let edge = Arc::new(EdgeClient::new("/run/hr-edge.sock"));
    info!("Edge IPC client configured (socket: {})", edge.socket_path().display());

    let orchestrator = Arc::new(OrchestratorClient::new("/run/hr-orchestrator.sock"));
    info!("Orchestrator IPC client configured (socket: {})", orchestrator.socket_path().display());

    // ── Management API ────────────────────────────────────────────────

    let update_log = Arc::new(
        hr_api::routes::updates::UpdateAuditLog::new(std::path::Path::new("/opt/homeroute/data"))
            .expect("Failed to open update audit log"),
    );

    let api_state = hr_api::state::ApiState {
        auth: auth.clone(),
        acme: acme.clone(),
        edge: edge.clone(),
        netcore: netcore.clone(),
        orchestrator: orchestrator.clone(),
        events: events.clone(),
        env: Arc::new(env.clone()),
        dns_dhcp_config_path: env.dns_dhcp_config_path.clone(),
        proxy_config_path: env.proxy_config_path.clone(),
        reverseproxy_config_path: env.reverseproxy_config_path.clone(),
        service_registry: service_registry.clone(),

        // These fields are managed by hr-orchestrator; homeroute passes None/empty.
        registry: None,
        container_manager: None,
        git: None,
        migrations: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        renames: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        dataverse_schemas: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        update_log,
    };

    let api_router = hr_api::build_router(api_state);
    let api_port = env.api_port;

    let reg = service_registry.clone();
    spawn_supervised("api", ServicePriority::Important, reg, move || {
        let router = api_router.clone();
        let port = api_port;
        async move {
            let addr: SocketAddr = format!("[::]:{}", port).parse()?;
            let listener = tokio::net::TcpListener::bind(addr).await?;
            info!("Management API listening on {}", addr);
            axum::serve(listener, router).await?;
            Ok(())
        }
    });

    // ── Background tasks ────────────────────────────────────────────────

    // Local host metrics broadcast (every 2s)
    {
        let events = events.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            let mut prev: Option<(u64, u64)> = None;
            loop {
                interval.tick().await;
                let read_cpu = || -> Option<(u64, u64)> {
                    let content = std::fs::read_to_string("/proc/stat").ok()?;
                    let line = content.lines().next()?;
                    let vals: Vec<u64> = line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
                    if vals.len() < 4 { return None; }
                    let idle = vals[3];
                    let total: u64 = vals.iter().sum();
                    Some((idle, total))
                };
                let cur = read_cpu();
                let cpu_percent = match (prev, cur) {
                    (Some((idle1, total1)), Some((idle2, total2))) => {
                        let di = idle2.saturating_sub(idle1) as f64;
                        let dt = total2.saturating_sub(total1) as f64;
                        if dt > 0.0 { ((1.0 - di / dt) * 1000.0).round() / 10.0 } else { 0.0 }
                    }
                    _ => 0.0,
                };
                prev = cur;
                let mem = (|| -> Option<(u64, u64)> {
                    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
                    let parse_kb = |key: &str| -> Option<u64> {
                        info.lines().find(|l| l.starts_with(key))
                            .and_then(|l| l.split_whitespace().nth(1))
                            .and_then(|v| v.parse::<u64>().ok())
                    };
                    let total_kb = parse_kb("MemTotal:")?;
                    let avail_kb = parse_kb("MemAvailable:")?;
                    Some(((total_kb - avail_kb) * 1024, total_kb * 1024))
                })();
                let (mem_used, mem_total) = mem.unwrap_or((0, 0));
                let _ = events.host_metrics.send(hr_common::events::HostMetricsEvent {
                    host_id: "local".to_string(),
                    cpu_percent: cpu_percent as f32,
                    memory_used_bytes: mem_used,
                    memory_total_bytes: mem_total,
                });
            }
        });
    }

    // Migrate servers.json -> hosts.json if needed
    hr_api::routes::hosts::ensure_hosts_file().await;

    // ── Ready ───────────────────────────────────────────────────────────

    info!("HomeRoute started successfully");
    info!("  Auth: OK");
    info!(
        "  ACME: OK ({} wildcard certificates)",
        acme.list_certificates().unwrap_or_default().len()
    );
    info!("  Events: OK (broadcast bus)");
    info!("  DNS/DHCP/IPv6/Adblock: delegated to hr-netcore (IPC)");
    info!("  Proxy/TLS/ACME-writes/CloudRelay: delegated to hr-edge (IPC)");
    info!("  Containers/Registry/Git: delegated to hr-orchestrator (IPC)");
    info!("  API: listening on port {}", api_port);

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}
