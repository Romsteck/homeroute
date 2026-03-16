// Container manager has the full canonical implementation. Some methods are
// not yet wired to IPC (create, migrate, rename) -- they will be activated
// as the deploy pipeline migrates from hr-api.
#[allow(dead_code)]
mod container_manager;
mod ipc_handler;
mod mcp;
mod backup_pipeline;
mod ws_routes;
mod container_watcher;

use hr_acme::{AcmeConfig, AcmeManager};
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_ipc::{EdgeClient, NetcoreClient};
use hr_registry::AgentRegistry;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use container_manager::ContainerManager;
use ipc_handler::OrchestratorHandler;

const ORCHESTRATOR_SOCKET: &str = "/run/hr-orchestrator.sock";
const ORCHESTRATOR_WS_PORT: u16 = 4001;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install rustls crypto provider (ring)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hr_orchestrator=debug".parse().unwrap()),
        )
        .init();

    info!("hr-orchestrator starting...");

    // Load environment config
    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    // Initialize event bus (local to hr-orchestrator)
    let events = Arc::new(EventBus::new());

    // ── ACME (read-only, for checking existing certificates) ────────

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
        account_email: env
            .acme_email
            .clone()
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

    // ── IPC client for hr-edge ──────────────────────────────────────

    let edge = Arc::new(EdgeClient::new("/run/hr-edge.sock"));
    info!(
        "Edge IPC client configured (socket: {})",
        edge.socket_path().display()
    );

    // ── Agent Registry ──────────────────────────────────────────────

    let registry_state_path = PathBuf::from("/var/lib/server-dashboard/agent-registry.json");
    let registry = Arc::new(AgentRegistry::new(
        registry_state_path,
        Arc::new(env.clone()),
        events.clone(),
    ));

    // Provide ACME manager to registry for per-app certificate management
    registry.set_acme(acme.clone()).await;

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
                    if let Err(e) = edge
                        .set_app_route(
                            route.domain.clone(),
                            app.id.clone(),
                            app.host_id.clone(),
                            ipv4.to_string(),
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
                            "Failed to set app route via edge IPC at startup"
                        );
                    }
                }
            }
        }
    }

    info!(
        "Agent registry initialized ({} applications)",
        registry.list_applications().await.len()
    );

    // ── Git Service ──────────────────────────────────────────────────

    let git_service = Arc::new(hr_git::GitService::new());
    if let Err(e) = git_service.init().await {
        warn!("Failed to initialize git service: {e}");
    }
    info!("Git service initialized");

    // ── Container V2 Manager (nspawn) ────────────────────────────────

    let container_v2_state_path = PathBuf::from("/var/lib/server-dashboard/containers-v2.json");
    let container_manager = Arc::new(ContainerManager::new(
        container_v2_state_path,
        Arc::new(env.clone()),
        events.clone(),
        registry.clone(),
        Some(git_service.clone()),
    ));

    // Recover registry state from container configs if state file was lost/corrupted
    {
        let records = container_manager.list_container_records().await;
        let tuples: Vec<_> = records
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    c.name.clone(),
                    c.slug.clone(),
                    c.container_name.clone(),
                    c.host_id.clone(),
                    c.environment,
                    c.stack,
                    c.created_at,
                )
            })
            .collect();
        match registry.recover_from_containers(&tuples).await {
            Ok(0) => {} // nothing to recover
            Ok(n) => warn!(count = n, "Recovered applications from container configs"),
            Err(e) => warn!(error = %e, "Failed to recover registry from containers"),
        }
    }

    // Restore local containers that were running before reboot
    container_manager.restore_local_containers().await;

    // ── CertReady listener — notify agents of certificate renewals ───

    {
        let registry_cert = registry.clone();
        let mut cert_rx = events.cert_ready.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = cert_rx.recv().await {
                if event.slug.is_empty() {
                    // Global cert — notify ALL connected agents
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
                                warn!(
                                    app = %app.slug,
                                    error = %e,
                                    "Failed to send CertRenewal to agent"
                                );
                            }
                        }
                    }
                    info!("Sent global CertRenewal notification to all connected agents");
                } else {
                    // Per-app cert — notify only the matching agent
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
                                warn!(
                                    slug = %event.slug,
                                    error = %e,
                                    "Failed to send CertRenewal to agent"
                                );
                            } else {
                                info!(slug = %event.slug, "Sent CertRenewal notification to agent");
                            }
                        }
                    }
                }
            }
        });
    }

    // ── Periodic cleanup of stale signals ────────────────────────────

    {
        let reg = registry.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                reg.cleanup_stale_signals().await;
            }
        });
    }

    // Clean up stale migration transfer files from /tmp
    tokio::spawn(async {
        if let Ok(mut entries) = tokio::fs::read_dir("/tmp").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".tar.gz") && name_str.len() > 40 {
                    let stem = &name_str[..name_str.len() - 7];
                    if uuid::Uuid::parse_str(stem).is_ok() {
                        info!("Cleaning stale migration file: /tmp/{}", name_str);
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    });

    // ── IPC server (Unix socket) ─────────────────────────────────────

    let migrations = Arc::new(RwLock::new(HashMap::new()));
    let renames = Arc::new(RwLock::new(HashMap::new()));

    // ── Backup pipeline ──────────────────────────────────────────────────
    let backup_pipeline = Arc::new(backup_pipeline::BackupPipeline::new(events.clone()));
    backup_pipeline::spawn_daily_scheduler(backup_pipeline.clone());
    info!("Backup pipeline initialized (daily at 20:00 UTC / 22h Brussels)");

    // ── Container health watcher ──────────────────────────────────────────
    container_watcher::ContainerWatcher::spawn(registry.clone(), events.clone());
    info!("Container health watcher spawned (60s interval, auto-recovery)");

    let handler = Arc::new(OrchestratorHandler {
        registry: registry.clone(),
        container_manager: container_manager.clone(),
        git: git_service.clone(),
        migrations,
        renames,
        backup: backup_pipeline.clone(),
    });

    let ipc_handle = tokio::spawn({
        let handler = handler.clone();
        async move {
            let socket_path = std::path::Path::new(ORCHESTRATOR_SOCKET);
            if let Err(e) = hr_ipc::server::run_ipc_server(socket_path, handler).await {
                tracing::error!("IPC server error: {e:#}");
            }
        }
    });

    info!(
        "IPC server listening on {}",
        ORCHESTRATOR_SOCKET
    );

    // ── IPC client for hr-netcore (needed by WS routes for DNS records) ──

    let netcore = Arc::new(NetcoreClient::new("/run/hr-netcore.sock"));

    // ── MCP endpoint (optional, requires MCP_TOKEN env var) ─────────

    let mcp_state = mcp::McpState::from_env(registry.clone(), container_manager.clone(), git_service.clone(), edge.clone(), backup_pipeline.clone());
    if mcp_state.is_some() {
        info!("MCP endpoint enabled (POST /mcp)");
    }

    // ── Axum server (port 4001) — WebSocket + health endpoints ────

    let ws_state = ws_routes::WsState {
        registry: registry.clone(),
        container_manager: container_manager.clone(),
        env: Arc::new(env.clone()),
        events: events.clone(),
        edge: edge.clone(),
        netcore,
    };

    let ws_router = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"status": "ok", "service": "hr-orchestrator"}))
            }),
        )
        .route("/agents/ws", axum::routing::get(ws_routes::agent_ws))
        .route("/host-agents/ws", axum::routing::get(ws_routes::host_agent_ws))
        // Alias: host agents connect via legacy path /api/hosts/agent/ws
        .route("/api/hosts/agent/ws", axum::routing::get(ws_routes::host_agent_ws))
        .route(
            "/containers/{id}/terminal",
            axum::routing::get(ws_routes::terminal_ws),
        )
        .with_state(ws_state);

    // Merge MCP route (has its own state, only if MCP_TOKEN is set)
    let ws_router = if let Some(mcp_st) = mcp_state {
        let mcp_router = axum::Router::new()
            .route("/mcp", axum::routing::post(mcp::mcp_handler))
            .with_state(mcp_st);
        ws_router.merge(mcp_router)
    } else {
        ws_router
    };

    let ws_handle = tokio::spawn(async move {
        let addr: SocketAddr = format!("[::]:{}", ORCHESTRATOR_WS_PORT)
            .parse()
            .expect("Invalid WS bind address");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("Failed to bind WS port");
        info!("WebSocket server listening on {}", addr);
        if let Err(e) = axum::serve(listener, ws_router).await {
            tracing::error!("WebSocket server error: {e}");
        }
    });

    // ── Ready ────────────────────────────────────────────────────────

    info!("hr-orchestrator started successfully");
    info!("  Registry: {} applications", registry.list_applications().await.len());
    info!("  Containers: restored");
    info!("  Git: initialized");
    info!("  IPC: {}", ORCHESTRATOR_SOCKET);
    info!("  WS: port {}", ORCHESTRATOR_WS_PORT);
    info!("  MCP: {}", if std::env::var("MCP_TOKEN").is_ok() { "enabled" } else { "disabled (set MCP_TOKEN)" });

    // Wait for shutdown signal (SIGINT or SIGTERM)
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM, shutting down..."),
            _ = sigint.recv() => info!("Received SIGINT, shutting down..."),
        }
    }

    // Abort background tasks
    ipc_handle.abort();
    ws_handle.abort();

    Ok(())
}
