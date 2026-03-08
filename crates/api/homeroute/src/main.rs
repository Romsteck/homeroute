use hr_auth::AuthService;
use hr_acme::{AcmeConfig, AcmeManager};
use hr_common::config::EnvConfig;
use hr_common::events::{CertReadyEvent, EventBus};
use hr_common::service_registry::new_service_registry;
use hr_common::supervisor::{spawn_supervised, ServicePriority};
use hr_ipc::{NetcoreClient, EdgeClient};
use hr_ipc::orchestrator::OrchestratorClient;
use hr_registry::AgentRegistry;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

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

    // Load environment config
    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    // Initialize event bus
    let events = Arc::new(EventBus::new());

    // Initialize service registry
    let service_registry = new_service_registry();

    // Initialize auth service
    let auth = AuthService::new(&env.auth_data_dir, &env.base_domain)?;
    auth.start_cleanup_task();
    info!("Auth service initialized");

    // Initialize ACME (Let's Encrypt) -- kept for reads (list_certificates, etc.)
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
        if acme.is_initialized() {
            "account loaded"
        } else {
            "new account created"
        }
    );

    // ── IPC client for hr-netcore ────────────────────────────────────────

    let netcore = Arc::new(NetcoreClient::new("/run/hr-netcore.sock"));
    info!("Netcore IPC client configured (socket: {})", netcore.socket_path().display());

    // ── IPC client for hr-edge ──────────────────────────────────────────

    let edge = Arc::new(EdgeClient::new("/run/hr-edge.sock"));
    info!("Edge IPC client configured (socket: {})", edge.socket_path().display());

    // ── IPC client for hr-orchestrator ────────────────────────────────────

    let orchestrator = Arc::new(OrchestratorClient::new("/run/hr-orchestrator.sock"));
    info!("Orchestrator IPC client configured (socket: {})", orchestrator.socket_path().display());

    // ── Agent Registry ──────────────────────────────────────────────

    let registry_state_path =
        PathBuf::from("/var/lib/server-dashboard/agent-registry.json");
    let registry = Arc::new(AgentRegistry::new(
        registry_state_path,
        Arc::new(env.clone()),
        events.clone(),
    ));

    // Provide ACME manager to registry for per-app certificate management
    registry.set_acme(acme.clone()).await;

    // Request per-app wildcard certificates for existing applications that don't have one yet
    // Certificate requests are sent to hr-edge via IPC; it handles TLS loading internally.
    {
        let apps = registry.list_applications().await;
        let edge_init = edge.clone();
        let events_init = events.clone();
        let base_domain_init = env.base_domain.clone();
        let acme_init = acme.clone();
        let missing_apps: Vec<_> = apps
            .iter()
            .filter(|app| acme_init.get_app_certificate(&app.slug).is_err())
            .map(|app| app.slug.clone())
            .collect();

        if !missing_apps.is_empty() {
            info!(
                count = missing_apps.len(),
                "Requesting per-app wildcard certificates for existing applications"
            );
            tokio::spawn(async move {
                for slug in missing_apps {
                    info!(slug = %slug, "Requesting per-app wildcard certificate via edge IPC");
                    match edge_init.request(&hr_ipc::edge::EdgeRequest::AcmeRequestAppWildcard {
                        slug: slug.clone(),
                    }).await {
                        Ok(resp) if resp.ok => {
                            let domain = format!("*.{}.{}", slug, base_domain_init);
                            // Emit CertReadyEvent so agents get notified
                            let cert_path = resp.data.as_ref()
                                .and_then(|d| d.get("cert_path"))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let key_path = resp.data.as_ref()
                                .and_then(|d| d.get("key_path"))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let _ = events_init.cert_ready.send(CertReadyEvent {
                                slug: slug.clone(),
                                wildcard_domain: domain.clone(),
                                cert_path,
                                key_path,
                            });
                            info!(slug = %slug, domain = %domain, "Per-app wildcard certificate issued");
                        }
                        Ok(resp) => {
                            warn!(slug = %slug, error = ?resp.error, "Edge returned error for per-app wildcard");
                        }
                        Err(e) => {
                            warn!(slug = %slug, error = %e, "Failed to request per-app wildcard via edge IPC");
                        }
                    }
                    // Stagger requests to avoid Let's Encrypt rate limits
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            });
        }
    }

    // Heartbeat monitor
    {
        let reg = registry.clone();
        tokio::spawn(async move {
            reg.run_heartbeat_monitor().await;
        });
    }

    // Populate app routes for all applications with IPv4 addresses via edge IPC
    {
        let apps = registry.list_applications().await;
        let base_domain = &env.base_domain;
        for app in &apps {
            if let Some(ipv4) = app.ipv4_address {
                for route in app.routes(base_domain) {
                    if let Err(e) = edge.set_app_route(
                        route.domain.clone(),
                        app.id.clone(),
                        app.host_id.clone(),
                        ipv4.to_string(),
                        route.target_port,
                        route.auth_required,
                        route.allowed_groups.clone(),
                        app.frontend.local_only,
                    ).await {
                        warn!(domain = route.domain, error = %e, "Failed to set app route via edge IPC at startup");
                    }
                }
            }
        }
    }

    info!(
        "Agent registry initialized ({} applications)",
        registry.list_applications().await.len()
    );

    // ── Container V2 Manager (nspawn) ────────────────────────────────

    // ── Git Service ──────────────────────────────────────────────────
    let git_service = Arc::new(hr_git::GitService::new());
    if let Err(e) = git_service.init().await {
        warn!("Failed to initialize git service: {e}");
    }
    info!("Git service initialized");

    let container_v2_state_path = PathBuf::from("/var/lib/server-dashboard/containers-v2.json");
    let container_manager = Arc::new(hr_api::container_manager::ContainerManager::new(
        container_v2_state_path,
        Arc::new(env.clone()),
        events.clone(),
        registry.clone(),
        Some(git_service.clone()),
    ));

    // ── Restore local containers that were running before reboot ──────
    container_manager.restore_local_containers().await;

    // ── Management API (Important) ────────────────────────────────────

    let update_log = std::sync::Arc::new(
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

        registry: Some(registry.clone()),
        container_manager: Some(container_manager.clone()),
        git: Some(git_service.clone()),
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

    // ── Background tasks ───────────────────────────────────────────────

    // Local host metrics broadcast (every 2s)
    {
        let events = events.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            // Track previous CPU idle/total for delta calculation
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
                // Memory
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

    // CertReady listener -- notify agents of certificate renewals.
    // TLS loading is handled by hr-edge internally (no more tls_manager here).
    {
        let registry_cert = registry.clone();
        let mut cert_rx = events.cert_ready.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = cert_rx.recv().await {
                // Notify agents of certificate renewal so they can hot-reload
                if event.slug.is_empty() {
                    // Global cert -- notify ALL connected agents
                    let apps = registry_cert.list_applications().await;
                    for app in &apps {
                        if registry_cert.is_agent_connected(&app.id).await {
                            if let Err(e) = registry_cert
                                .send_to_agent(
                                    &app.id,
                                    hr_registry::RegistryMessage::CertRenewal {
                                        slug: String::new(),
                                    },
                                )
                                .await
                            {
                                warn!(app = %app.slug, error = %e, "Failed to send CertRenewal to agent");
                            }
                        }
                    }
                    info!("Sent global CertRenewal notification to all connected agents");
                } else {
                    // Per-app cert -- notify only the matching agent
                    let apps = registry_cert.list_applications().await;
                    if let Some(app) = apps.iter().find(|a| a.slug == event.slug) {
                        if registry_cert.is_agent_connected(&app.id).await {
                            if let Err(e) = registry_cert
                                .send_to_agent(
                                    &app.id,
                                    hr_registry::RegistryMessage::CertRenewal {
                                        slug: event.slug.clone(),
                                    },
                                )
                                .await
                            {
                                warn!(slug = %event.slug, error = %e, "Failed to send CertRenewal to agent");
                            } else {
                                info!(slug = %event.slug, "Sent CertRenewal notification to agent");
                            }
                        }
                    }
                }
            }
        });
    }

    // Migrate servers.json -> hosts.json if needed
    hr_api::routes::hosts::ensure_hosts_file().await;

    // ── Startup cleanup + periodic maintenance ─────────────────────────

    // Clean up stale migration transfer files from /tmp
    tokio::spawn(async {
        if let Ok(mut entries) = tokio::fs::read_dir("/tmp").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".tar.gz") && name_str.len() > 40 {
                    let stem = &name_str[..name_str.len() - 7];
                    if uuid::Uuid::parse_str(stem).is_ok() {
                        tracing::info!("Cleaning stale migration file: /tmp/{}", name_str);
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    });

    // Periodic cleanup of stale migration/exec signals
    {
        let reg = registry.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                reg.cleanup_stale_signals().await;
            }
        });
    }

    // ── Ready ──────────────────────────────────────────────────────────

    info!("HomeRoute started successfully");
    info!("  Auth: OK");
    info!(
        "  ACME: OK ({} wildcard certificates)",
        acme.list_certificates().unwrap_or_default().len()
    );
    info!("  Events: OK (broadcast bus)");
    info!("  DNS/DHCP/IPv6/Adblock: delegated to hr-netcore (IPC)");
    info!("  Proxy/TLS/ACME-writes/CloudRelay: delegated to hr-edge (IPC)");
    info!("  API: listening on port {}", api_port);
    info!("  Hosts: status via host-agent WebSocket");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}
