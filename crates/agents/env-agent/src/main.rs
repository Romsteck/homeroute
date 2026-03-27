mod config;
mod connection;
mod context;
mod db_manager;
mod discovery;
mod mcp;
mod secrets;
mod supervisor;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use hr_environment::protocol::{AppControlAction, EnvAgentMessage, EnvOrchestratorMessage};
use hr_environment::types::EnvApp;
use hr_environment::EnvAgentConfig;

use crate::context::ContextGenerator;
use crate::db_manager::DbManager;
use crate::secrets::SecretsManager;
use crate::supervisor::AppSupervisor;

const CONFIG_PATH: &str = "/etc/env-agent.toml";
const MAX_BACKOFF_SECS: u64 = 60;
const INITIAL_BACKOFF_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    // Check for MCP subcommand
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "mcp" {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .init();

        return mcp::run_mcp_server().await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,env_agent=debug".parse().unwrap()),
        )
        .init();

    info!("env-agent v{} starting", env!("CARGO_PKG_VERSION"));

    // ── 1. Load configuration ────────────────────────────────────────
    let cfg = EnvAgentConfig::load(CONFIG_PATH)?;
    info!(
        env = cfg.env_slug,
        apps = cfg.apps.len(),
        orchestrator = format!("{}:{}", cfg.homeroute_address, cfg.homeroute_port),
        "Config loaded"
    );

    // ── 2. Initialize DbManager ──────────────────────────────────────
    let db_manager = Arc::new(DbManager::new(&PathBuf::from(&cfg.db_path)));
    info!(path = cfg.db_path, "DbManager initialized");

    // ── 2b. Initialize SecretsManager ──────────────────────────────
    let data_path = PathBuf::from(&cfg.db_path).parent().unwrap_or(std::path::Path::new("/var/lib/env-agent")).to_path_buf();
    let secrets = Arc::new(SecretsManager::new(&data_path.join("secrets")));
    info!("SecretsManager initialized");

    // ── 3. Initialize AppSupervisor ──────────────────────────────────
    let supervisor = Arc::new(AppSupervisor::new(cfg.apps.clone()));
    info!(apps = cfg.apps.len(), "AppSupervisor initialized");

    // ── 4. Start all configured apps ─────────────────────────────────
    supervisor.start_all().await;

    // ── 5. Generate Claude Code context files ────────────────────────
    let context_gen = Arc::new(ContextGenerator {
        env_slug: cfg.env_slug.clone(),
        env_type: cfg.env_type(),
        base_domain: "mynetwk.biz".to_string(),
        apps_path: cfg.apps_path.clone(),
        mcp_port: cfg.mcp_port,
        homeroute_address: cfg.homeroute_address.clone(),
        homeroute_port: cfg.homeroute_port,
    });
    if let Err(e) = context_gen.refresh_all(&cfg.apps) {
        warn!("Failed to generate initial context files: {e}");
    }

    // ── 6. Start discovery + MCP HTTP server ─────────────────────────
    let discovery_state = discovery::DiscoveryState {
        env_slug: cfg.env_slug.clone(),
        supervisor: Arc::clone(&supervisor),
    };

    let mcp_state = mcp::McpState {
        db: Arc::clone(&db_manager),
        supervisor: Arc::clone(&supervisor),
        config: Arc::new(cfg.clone()),
        context: Arc::clone(&context_gen),
        secrets: Arc::clone(&secrets),
    };

    let discovery_router = discovery::router(discovery_state);
    let mcp_router = axum::Router::new()
        .route("/mcp", axum::routing::post(mcp::mcp_handler))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(mcp_state);

    let app_router = mcp_router.merge(discovery_router);

    let http_port = cfg.mcp_port;
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", http_port)).await
        {
            Ok(l) => l,
            Err(e) => {
                error!(port = http_port, "Failed to bind HTTP server: {e}");
                return;
            }
        };
        info!(port = http_port, "HTTP server started (discovery + MCP)");
        if let Err(e) = axum::serve(listener, app_router).await {
            error!("HTTP server error: {e}");
        }
    });

    // ── 7. WebSocket reconnection loop to orchestrator ───────────────
    let mut backoff = INITIAL_BACKOFF_SECS;

    loop {
        let (orchestrator_tx, mut orchestrator_rx) = mpsc::channel::<EnvOrchestratorMessage>(32);
        let (outbound_tx, outbound_rx) = mpsc::channel::<EnvAgentMessage>(64);

        info!(backoff_secs = backoff, "Connecting to orchestrator...");

        // Ensure network interfaces are up
        connection::ensure_network_up(&cfg.interface).await;

        // Spawn WebSocket connection
        let cfg_clone = cfg.clone();
        let mut conn_handle = tokio::spawn(async move {
            connection::run_connection(&cfg_clone, orchestrator_tx, outbound_rx).await
        });

        // Send initial app discovery after connection
        let apps_info = build_app_discovery(&supervisor).await;
        let _ = outbound_tx
            .send(EnvAgentMessage::AppDiscovery { apps: apps_info })
            .await;

        // ── Message loop ─────────────────────────────────────────────
        let mut connected = false;
        loop {
            tokio::select! {
                result = &mut conn_handle => {
                    match result {
                        Ok(Ok(())) => {
                            if connected {
                                info!("Connection closed, will reconnect");
                            }
                        }
                        Ok(Err(e)) => {
                            if connected {
                                error!("Connection lost: {e}");
                            } else {
                                error!("Connection failed: {e}");
                            }
                        }
                        Err(e) => error!("Connection task panicked: {e}"),
                    }
                    break;
                }

                msg = orchestrator_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if !connected {
                                connected = true;
                                backoff = INITIAL_BACKOFF_SECS;
                                info!("Connected to orchestrator");
                            }
                            handle_orchestrator_message(
                                msg,
                                &outbound_tx,
                                &supervisor,
                                &db_manager,
                                &cfg,
                            ).await;
                        }
                        None => break,
                    }
                }
            }
        }

        // Wait before reconnecting
        info!(secs = backoff, "Waiting before reconnect...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

/// Handle a message from the orchestrator.
async fn handle_orchestrator_message(
    msg: EnvOrchestratorMessage,
    outbound_tx: &mpsc::Sender<EnvAgentMessage>,
    supervisor: &Arc<AppSupervisor>,
    db_manager: &Arc<DbManager>,
    _cfg: &EnvAgentConfig,
) {
    match msg {
        EnvOrchestratorMessage::AuthResult { success, error, .. } => {
            if success {
                info!("Authenticated with orchestrator");
            } else {
                error!("Auth failed: {}", error.unwrap_or_default());
            }
        }

        EnvOrchestratorMessage::Config { env_slug, env_type, .. } => {
            info!(env_slug, ?env_type, "Received environment config");
            // TODO: update context generator with actual env_type + base_domain
        }

        EnvOrchestratorMessage::AppControl { app_slug, action } => {
            info!(app = %app_slug, ?action, "App control command");
            let result = match action {
                AppControlAction::Start => supervisor.start_app(&app_slug).await,
                AppControlAction::Stop => supervisor.stop_app(&app_slug).await,
                AppControlAction::Restart => supervisor.restart_app(&app_slug).await,
            };
            if let Err(e) = result {
                error!(app = %app_slug, "App control failed: {e}");
                let _ = outbound_tx
                    .send(EnvAgentMessage::AppStatus {
                        app_slug,
                        running: false,
                        error: Some(e.to_string()),
                    })
                    .await;
            } else {
                // Report new status
                let running = supervisor
                    .app_status(&app_slug)
                    .await
                    .map(|s| s == supervisor::AppProcessStatus::Running)
                    .unwrap_or(false);
                let _ = outbound_tx
                    .send(EnvAgentMessage::AppStatus {
                        app_slug,
                        running,
                        error: None,
                    })
                    .await;
            }
        }

        EnvOrchestratorMessage::DeployApp {
            pipeline_id,
            app_slug,
            ..
        } => {
            info!(app = %app_slug, pipeline = %pipeline_id, "Deploy requested");
            // TODO Phase 5: download artifact, extract, restart app
            let _ = outbound_tx
                .send(EnvAgentMessage::PipelineProgress {
                    pipeline_id,
                    step: "deploy".into(),
                    success: false,
                    message: Some("Not yet implemented".into()),
                })
                .await;
        }

        EnvOrchestratorMessage::MigrateDb {
            pipeline_id,
            app_slug,
            migrations,
        } => {
            info!(
                app = %app_slug,
                pipeline = %pipeline_id,
                count = migrations.len(),
                "DB migration requested"
            );

            // Snapshot before migration
            if let Err(e) = db_manager.snapshot_db(&app_slug).await {
                warn!(app = %app_slug, "Snapshot failed: {e}");
            }

            // Apply migrations
            let engine = match db_manager.get_engine(&app_slug).await {
                Ok(e) => e,
                Err(e) => {
                    let _ = outbound_tx
                        .send(EnvAgentMessage::MigrationResult {
                            pipeline_id,
                            app_slug,
                            success: false,
                            migrations_applied: 0,
                            error: Some(e),
                        })
                        .await;
                    return;
                }
            };

            let engine = engine.lock().await;
            let mut applied = 0u32;
            let mut last_error = None;
            for sql in &migrations {
                match engine.connection().execute_batch(sql) {
                    Ok(()) => applied += 1,
                    Err(e) => {
                        last_error = Some(e.to_string());
                        break;
                    }
                }
            }

            let _ = outbound_tx
                .send(EnvAgentMessage::MigrationResult {
                    pipeline_id,
                    app_slug,
                    success: last_error.is_none(),
                    migrations_applied: applied,
                    error: last_error,
                })
                .await;
        }

        EnvOrchestratorMessage::RollbackApp {
            pipeline_id,
            app_slug,
        } => {
            info!(app = %app_slug, pipeline = %pipeline_id, "Rollback requested");
            // TODO Phase 5: restore previous binary + DB snapshot
        }

        EnvOrchestratorMessage::SnapshotDb {
            pipeline_id,
            app_slug,
        } => {
            info!(app = %app_slug, pipeline = %pipeline_id, "DB snapshot requested");
            match db_manager.snapshot_db(&app_slug).await {
                Ok(path) => info!(path = %path.display(), "Snapshot created"),
                Err(e) => error!(app = %app_slug, "Snapshot failed: {e}"),
            }
        }

        EnvOrchestratorMessage::RefreshContext { app_slug } => {
            info!(app = %app_slug, "Context refresh requested");
            // TODO: re-generate CLAUDE.md for this app with updated DB schema
        }

        EnvOrchestratorMessage::HostInfo { host_id, hostname, total_memory_mb, available_memory_mb } => {
            info!(host_id, hostname, total_memory_mb, available_memory_mb, "Received host info from orchestrator");
            // Store for local awareness if needed in the future
        }

        EnvOrchestratorMessage::Shutdown => {
            info!("Shutdown requested by orchestrator");
            supervisor.stop_all().await;
            std::process::exit(0);
        }

        EnvOrchestratorMessage::UpdateAvailable {
            version,
            download_url,
            ..
        } => {
            info!(version, download_url, "Update available");
            // TODO: self-update mechanism
        }
    }
}

/// Build the app discovery payload from supervisor state.
async fn build_app_discovery(supervisor: &AppSupervisor) -> Vec<EnvApp> {
    use hr_environment::types::AppStackType;

    supervisor
        .list_apps()
        .await
        .into_iter()
        .map(|app| EnvApp {
            slug: app.slug,
            name: app.name,
            stack: AppStackType::default(),
            port: app.port,
            version: app.version,
            running: app.status == supervisor::AppProcessStatus::Running,
            has_db: false, // TODO: check DbManager
        })
        .collect()
}
