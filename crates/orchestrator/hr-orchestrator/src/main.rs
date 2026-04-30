#![recursion_limit = "512"]
mod apps_handler;
mod backup_pipeline;
mod ipc_handler;
mod mcp;
mod scaffold;
mod ws_routes;

use hr_acme::{AcmeConfig, AcmeManager};
use hr_apps::{AppRegistry, AppSupervisor, ContextGenerator, DbManager, PortRegistry};
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_ipc::EdgeClient;
use hr_registry::AgentRegistry;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tracing::{info, warn};
use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use ipc_handler::OrchestratorHandler;

const ORCHESTRATOR_SOCKET: &str = "/run/hr-orchestrator.sock";
const ORCHESTRATOR_WS_PORT: u16 = 4001;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let log_store = std::sync::Arc::new(
        hr_common::logging::LogStore::new(std::path::Path::new("/opt/homeroute/data/logs.db"))
            .expect("Failed to init log store"),
    );

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,hr_orchestrator=debug".parse().unwrap()),
            ),
        )
        .with(hr_common::logging::LoggingLayer::new(
            log_store.clone(),
            hr_common::logging::LogService::Orchestrator,
        ))
        .init();

    info!("hr-orchestrator starting...");

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

    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    let events = Arc::new(EventBus::new());

    // ── ACME ────────────────────────────────────────────────────────

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
    info!("ACME manager initialized");

    // ── IPC clients ──────────────────────────────────────────────────

    let edge = Arc::new(EdgeClient::new("/run/hr-edge.sock"));

    // ── Agent Registry ──────────────────────────────────────────────

    let registry_state_path = PathBuf::from("/var/lib/server-dashboard/agent-registry.json");
    let registry = Arc::new(AgentRegistry::new(
        registry_state_path,
        Arc::new(env.clone()),
        events.clone(),
    ));
    registry.set_acme(acme.clone()).await;

    {
        let reg = registry.clone();
        tokio::spawn(async move {
            reg.run_heartbeat_monitor().await;
        });
    }

    // Populate app routes at startup
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
                        warn!(domain = route.domain, error = %e, "Failed to set app route at startup");
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

    // ── CertReady listener ──────────────────────────────────────────

    {
        let registry_cert = registry.clone();
        let mut cert_rx = events.cert_ready.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = cert_rx.recv().await {
                if event.slug.is_empty() {
                    let apps = registry_cert.list_applications().await;
                    for app in &apps {
                        if registry_cert.is_agent_connected(&app.id).await {
                            let _ = registry_cert
                                .send_to_agent(
                                    &app.id,
                                    hr_registry::RegistryMessage::CertRenewal {
                                        slug: String::new(),
                                    },
                                )
                                .await;
                        }
                    }
                } else {
                    let apps = registry_cert.list_applications().await;
                    if let Some(app) = apps.iter().find(|a| a.slug == event.slug) {
                        if registry_cert.is_agent_connected(&app.id).await {
                            let _ = registry_cert
                                .send_to_agent(
                                    &app.id,
                                    hr_registry::RegistryMessage::CertRenewal {
                                        slug: event.slug.clone(),
                                    },
                                )
                                .await;
                        }
                    }
                }
            }
        });
    }

    // ── Periodic cleanup ────────────────────────────────────────────

    {
        let reg = registry.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                reg.cleanup_stale_signals().await;
            }
        });
    }

    // Clean up stale migration files
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

    // ── Backup pipeline ──────────────────────────────────────────────

    let backup_pipeline = Arc::new(backup_pipeline::BackupPipeline::new(events.clone()));
    backup_pipeline::spawn_daily_scheduler(backup_pipeline.clone());
    info!("Backup pipeline initialized");

    // ── hr-apps ──────────────────────────────────────────────────────

    let app_registry = AppRegistry::load()
        .await
        .expect("Failed to load app registry");
    let port_registry = PortRegistry::load()
        .await
        .expect("Failed to load port registry");
    let supervisor = AppSupervisor::new(
        app_registry.clone(),
        port_registry.clone(),
        events.app_state.clone(),
    );
    let db_manager = DbManager::new("/opt/homeroute/apps");
    let todos_manager = hr_apps::todos::TodosManager::new("/opt/homeroute/apps", events.clone());
    let mcp_endpoint_url = std::env::var("HR_APPS_MCP_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:4001/mcp".to_string());
    let context_generator = Arc::new(ContextGenerator::new(
        "/opt/homeroute/apps",
        env.base_domain.clone(),
        mcp_endpoint_url,
    ));
    info!(
        "hr-apps initialized ({} apps)",
        app_registry.list().await.len()
    );

    if let Err(e) = supervisor.start_all_running().await {
        warn!(error = %e, "start_all_running failed at boot");
    }

    {
        let sup = supervisor.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            sup.detach_all().await;
            info!("AppSupervisor detached — systemd units left running");
        });
    }

    // ── IPC server ──────────────────────────────────────────────────

    let build_locks: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    > = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let handler = Arc::new(OrchestratorHandler {
        registry: registry.clone(),
        git: git_service.clone(),
        backup: backup_pipeline.clone(),
        edge: edge.clone(),
        base_domain: env.base_domain.clone(),
        app_supervisor: supervisor.clone(),
        db_manager: db_manager.clone(),
        todos: todos_manager.clone(),
        context_generator: context_generator.clone(),
        log_store: log_store.clone(),
        build_locks: build_locks.clone(),
        app_build_tx: events.app_build.clone(),
    });

    // ── Refresh per-app context for every existing app at boot ──────
    // Non-blocking: propagates new context files (e.g. app-build.md) to
    // apps that existed before the orchestrator was upgraded.
    {
        let registry_for_ctx = app_registry.clone();
        let ctx = context_generator.clone();
        tokio::spawn(async move {
            let apps = registry_for_ctx.list().await;
            info!(count = apps.len(), "boot: regenerating per-app context");
            for app in &apps {
                if let Err(e) = ctx.generate_for_app(app, &apps, None) {
                    warn!(slug = %app.slug, error = %e, "boot: context regen failed");
                }
            }
            if let Err(e) = ctx.generate_root(&apps) {
                warn!(error = %e, "boot: root context regen failed");
            }
            info!("boot: context regen done");
        });
    }

    let ipc_handle = tokio::spawn({
        let handler = handler.clone();
        async move {
            let socket_path = std::path::Path::new(ORCHESTRATOR_SOCKET);
            if let Err(e) = hr_ipc::server::run_ipc_server(socket_path, handler).await {
                tracing::error!("IPC server error: {e:#}");
            }
        }
    });
    info!("IPC server listening on {}", ORCHESTRATOR_SOCKET);

    // ── Event stream (push events to homeroute) ─────────────────────
    {
        let app_state_tx = events.app_state.clone();
        let log_tx = events.log_entry.clone();
        let app_build_tx = events.app_build.clone();
        let app_todos_tx = events.app_todos.clone();
        let host_status_tx = events.host_status.clone();
        let host_power_tx = events.host_power.clone();
        let host_metrics_tx = events.host_metrics.clone();
        tokio::spawn(async move {
            let socket_path = std::path::Path::new(hr_ipc::event_stream::EVENT_STREAM_SOCKET);
            if let Err(e) = hr_ipc::event_stream::serve_event_stream(
                socket_path,
                app_state_tx,
                log_tx,
                app_build_tx,
                app_todos_tx,
                host_status_tx,
                host_power_tx,
                host_metrics_tx,
            )
            .await
            {
                tracing::error!("Event stream server error: {e:#}");
            }
        });
        info!("Event stream server listening on {}", hr_ipc::event_stream::EVENT_STREAM_SOCKET);
    }

    // ── MCP endpoint ────────────────────────────────────────────────

    let mcp_state = {
        let mut st = mcp::McpState::from_env(registry.clone(), git_service.clone(), edge.clone());
        if let Some(ref mut s) = st {
            s.apps_ctx = Some(apps_handler::AppsContext {
                supervisor: supervisor.clone(),
                db_manager: db_manager.clone(),
                todos: todos_manager.clone(),
                context_generator: context_generator.clone(),
                edge: edge.clone(),
                git: git_service.clone(),
                base_domain: env.base_domain.clone(),
                log_store: log_store.clone(),
                build_locks: build_locks.clone(),
                app_build_tx: events.app_build.clone(),
            });
        }
        st
    };
    if mcp_state.is_some() {
        info!("MCP endpoint enabled (POST /mcp)");
    }

    // ── Axum server (port 4001) ─────────────────────────────────────

    let ws_state = ws_routes::WsState {
        registry: registry.clone(),
        events: events.clone(),
    };

    let ws_router = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"status": "ok", "service": "hr-orchestrator"}))
            }),
        )
        .route(
            "/host-agents/ws",
            axum::routing::get(ws_routes::host_agent_ws),
        )
        .route(
            "/api/hosts/agent/ws",
            axum::routing::get(ws_routes::host_agent_ws),
        )
        .nest_service("/artifacts", ServeDir::new("/opt/homeroute/data/artifacts"))
        .with_state(ws_state);

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
    info!(
        "  Registry: {} applications",
        registry.list_applications().await.len()
    );
    info!("  IPC: {}", ORCHESTRATOR_SOCKET);
    info!("  WS: port {}", ORCHESTRATOR_WS_PORT);

    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM, shutting down..."),
            _ = sigint.recv() => info!("Received SIGINT, shutting down..."),
        }
    }

    ipc_handle.abort();
    ws_handle.abort();

    Ok(())
}
