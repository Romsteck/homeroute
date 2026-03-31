mod app_proxy;
mod config;
mod connection;
mod context;
mod db_manager;
mod deploy;
mod discovery;
mod mcp;
mod port_registry;
mod proxy;
mod scaffold;
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

    // ── 2c. Port Registry ────────────────────────────────────────────
    let registry_path = data_path.join("port-registry.json");
    let mut port_reg = port_registry::PortRegistry::new(3001, registry_path);
    let _ = port_reg.load();
    let app_ports: Vec<(String, u16)> = cfg.apps.iter().map(|a| (a.slug.clone(), a.port)).collect();
    port_reg.assign_all(&app_ports)?;
    port_reg.save()?;
    // Apply assigned ports back to config
    let mut cfg = cfg;
    for app in &mut cfg.apps {
        if let Some(port) = port_reg.port_for(&app.slug) {
            app.port = port;
        }
    }
    info!(apps = cfg.apps.len(), "Port registry initialized");

    // ── 2d. Auto-derive git_repo for dev envs ───────────────────────
    if cfg.env_type() == hr_environment::types::EnvType::Development {
        for app in &mut cfg.apps {
            if app.git_repo.is_none() {
                app.git_repo = Some(format!(
                    "http://{}:4000/api/git/repos/{}.git/",
                    cfg.homeroute_address, app.slug
                ));
                info!(slug = app.slug, "Auto-derived git_repo URL");
            }
        }
    }

    // ── 3. Initialize AppSupervisor ──────────────────────────────────
    let supervisor = Arc::new(AppSupervisor::new(cfg.apps.clone(), cfg.env_type(), cfg.apps_path.clone(), cfg.db_path.clone()));
    info!(apps = cfg.apps.len(), "AppSupervisor initialized");

    // ── 3b. Generate systemd service units ─────────────────────────────
    if let Err(e) = supervisor.generate_service_units() {
        warn!("Failed to generate service units: {e}");
    } else {
        info!("Systemd service units generated");
    }

    // ── 4. Start all configured apps ─────────────────────────────────
    supervisor.start_all().await;

    // ── 5. Generate Claude Code context files ────────────────────────
    let context_gen = Arc::new(ContextGenerator {
        env_slug: cfg.env_slug.clone(),
        env_type: cfg.env_type(),
        base_domain: "mynetwk.biz".to_string(),
        apps_path: cfg.apps_path.clone(),
        mcp_port: cfg.mcp_port,
    });
    if let Err(e) = context_gen.refresh_all(&cfg.apps) {
        warn!("Failed to generate initial context files: {e}");
    }

    // ── 6. Start discovery + MCP HTTP server ─────────────────────────
    let discovery_state = discovery::DiscoveryState {
        env_slug: cfg.env_slug.clone(),
        supervisor: Arc::clone(&supervisor),
    };

    let shared_config = Arc::new(tokio::sync::RwLock::new(cfg.clone()));
    let shared_port_registry = Arc::new(std::sync::Mutex::new(port_reg));

    let mcp_state = mcp::McpState {
        db: Arc::clone(&db_manager),
        supervisor: Arc::clone(&supervisor),
        config: Arc::clone(&shared_config),
        context: Arc::clone(&context_gen),
        secrets: Arc::clone(&secrets),
        http_client: reqwest::Client::new(),
        port_registry: Arc::clone(&shared_port_registry),
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

    // ── 6b. Start internal app proxy (port 80) ────────────────────────
    // Routes *.{env}.{domain} traffic to the correct app based on Host header.
    {
        let proxy_state = app_proxy::AppProxyState {
            config: Arc::clone(&shared_config),
            http_client: hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http(),
        };

        let proxy_router = axum::Router::new()
            .fallback(app_proxy::proxy_handler)
            .with_state(proxy_state);

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind("0.0.0.0:80").await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind app proxy on port 80: {e}");
                    return;
                }
            };
            info!("App proxy started on port 80");
            if let Err(e) = axum::serve(listener, proxy_router).await {
                error!("App proxy error: {e}");
            }
        });
    }

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

        // Spawn periodic app status re-scan (every 10s) so the orchestrator
        // always has up-to-date running/stopped state for each app.
        let status_tx = outbound_tx.clone();
        let status_supervisor = Arc::clone(&supervisor);
        let status_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            interval.tick().await; // skip immediate first tick (initial discovery already sent)
            loop {
                interval.tick().await;
                let apps = build_app_discovery(&status_supervisor).await;
                if status_tx
                    .send(EnvAgentMessage::AppDiscovery { apps })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Spawn metrics sender (every 5 seconds) — uses cgroups for container-accurate metrics
        let metrics_tx = outbound_tx.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            let mut prev_cpu_usec: Option<(u64, std::time::Instant)> = None;
            loop {
                interval.tick().await;

                // CPU usage from cgroup cpu.stat (usage_usec / elapsed wall time)
                let cpu_percent = {
                    let now = std::time::Instant::now();
                    let usage_usec = std::fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()
                        .and_then(|s| s.lines().find(|l| l.starts_with("usage_usec"))
                            .and_then(|l| l.split_whitespace().nth(1))
                            .and_then(|v| v.parse::<u64>().ok()));
                    let pct = match (prev_cpu_usec, usage_usec) {
                        (Some((prev_usec, prev_time)), Some(cur_usec)) => {
                            let delta_usec = cur_usec.saturating_sub(prev_usec) as f64;
                            let delta_wall = now.duration_since(prev_time).as_micros() as f64;
                            // Normalize by number of CPUs for percentage
                            let num_cpus = num_cpus().unwrap_or(1) as f64;
                            if delta_wall > 0.0 {
                                ((delta_usec / delta_wall / num_cpus) * 1000.0).round() / 10.0
                            } else { 0.0 }
                        }
                        _ => 0.0,
                    };
                    if let Some(usec) = usage_usec {
                        prev_cpu_usec = Some((usec, now));
                    }
                    pct as f32
                };

                // Memory from cgroup (container-specific, not host)
                let mem_used = read_u64_file("/sys/fs/cgroup/memory.current").unwrap_or(0);
                let mem_max = read_u64_file("/sys/fs/cgroup/memory.max");
                // If memory.max is "max" (unlimited), fall back to host MemTotal
                let mem_total = mem_max.unwrap_or_else(|| {
                    std::fs::read_to_string("/proc/meminfo").ok()
                        .and_then(|s| s.lines().find(|l| l.starts_with("MemTotal:"))
                            .and_then(|l| l.split_whitespace().nth(1))
                            .and_then(|v| v.parse::<u64>().ok()))
                        .map(|kb| kb * 1024)
                        .unwrap_or(0)
                });

                // Disk usage from df (container shares host disk but shows its own overlay)
                let (disk_used, disk_total) = {
                    std::process::Command::new("df")
                        .args(["-B1", "--output=used,size", "/"])
                        .output().ok()
                        .and_then(|o| {
                            let text = String::from_utf8_lossy(&o.stdout);
                            let line = text.lines().nth(1)?;
                            let p: Vec<&str> = line.split_whitespace().collect();
                            Some((p.first()?.parse().ok()?, p.get(1)?.parse().ok()?))
                        })
                        .unwrap_or((0, 0))
                };

                let metrics = hr_environment::protocol::EnvAgentMetrics {
                    memory_bytes: mem_used,
                    memory_total_bytes: mem_total,
                    cpu_percent,
                    disk_used_bytes: disk_used,
                    disk_total_bytes: disk_total,
                    apps_memory_bytes: 0,
                };

                if metrics_tx.send(EnvAgentMessage::Metrics { data: metrics }).await.is_err() {
                    break;
                }
            }
        });

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

        // Cancel periodic tasks
        status_handle.abort();
        metrics_handle.abort();

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
            version,
            artifact_url,
            sha256,
        } => {
            info!(app = %app_slug, version = %version, pipeline = %pipeline_id, "Deploy requested");

            if !supervisor.has_app(&app_slug) {
                let _ = outbound_tx
                    .send(EnvAgentMessage::PipelineProgress {
                        pipeline_id,
                        step: "deploy".into(),
                        success: false,
                        message: Some(format!("App '{}' not found in supervisor config", app_slug)),
                    })
                    .await;
                return;
            }

            let app_path = PathBuf::from(format!("/apps/{}", app_slug));

            match deploy::deploy_artifact(&artifact_url, &sha256, &app_path).await {
                Ok(msg) => {
                    // Restart the app after successful deploy
                    if let Err(e) = supervisor.restart_app(&app_slug).await {
                        warn!(app = %app_slug, error = %e, "Restart after deploy failed");
                        let _ = outbound_tx
                            .send(EnvAgentMessage::PipelineProgress {
                                pipeline_id,
                                step: "deploy".into(),
                                success: false,
                                message: Some(format!("Deployed but restart failed: {e}")),
                            })
                            .await;
                    } else {
                        info!(app = %app_slug, version = %version, "Deploy + restart successful");
                        let _ = outbound_tx
                            .send(EnvAgentMessage::PipelineProgress {
                                pipeline_id,
                                step: "deploy".into(),
                                success: true,
                                message: Some(msg),
                            })
                            .await;
                    }
                }
                Err(e) => {
                    error!(app = %app_slug, error = %e, "Deploy failed");
                    let _ = outbound_tx
                        .send(EnvAgentMessage::PipelineProgress {
                            pipeline_id,
                            step: "deploy".into(),
                            success: false,
                            message: Some(format!("Deploy failed: {e}")),
                        })
                        .await;
                }
            }
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

            let app_path = PathBuf::from(format!("/apps/{}", app_slug));

            match deploy::rollback_app(&app_path).await {
                Ok(msg) => {
                    // Restart the app after rollback
                    if let Err(e) = supervisor.restart_app(&app_slug).await {
                        warn!(app = %app_slug, error = %e, "Restart after rollback failed");
                        let _ = outbound_tx
                            .send(EnvAgentMessage::PipelineProgress {
                                pipeline_id,
                                step: "rollback".into(),
                                success: false,
                                message: Some(format!("Rolled back but restart failed: {e}")),
                            })
                            .await;
                    } else {
                        info!(app = %app_slug, "Rollback + restart successful");
                        let _ = outbound_tx
                            .send(EnvAgentMessage::PipelineProgress {
                                pipeline_id,
                                step: "rollback".into(),
                                success: true,
                                message: Some(msg),
                            })
                            .await;
                    }
                }
                Err(e) => {
                    error!(app = %app_slug, error = %e, "Rollback failed");
                    let _ = outbound_tx
                        .send(EnvAgentMessage::PipelineProgress {
                            pipeline_id,
                            step: "rollback".into(),
                            success: false,
                            message: Some(format!("Rollback failed: {e}")),
                        })
                        .await;
                }
            }
        }

        EnvOrchestratorMessage::SnapshotDb {
            pipeline_id,
            app_slug,
        } => {
            info!(app = %app_slug, pipeline = %pipeline_id, "DB snapshot requested");
            match db_manager.snapshot_db(&app_slug).await {
                Ok(path) => {
                    let msg = format!("Snapshot created at {}", path.display());
                    info!("{msg}");
                    let _ = outbound_tx
                        .send(EnvAgentMessage::PipelineProgress {
                            pipeline_id,
                            step: "backup-db".into(),
                            success: true,
                            message: Some(msg),
                        })
                        .await;
                }
                Err(e) => {
                    let msg = format!("Snapshot failed: {e}");
                    error!(app = %app_slug, "{msg}");
                    let _ = outbound_tx
                        .send(EnvAgentMessage::PipelineProgress {
                            pipeline_id,
                            step: "backup-db".into(),
                            success: false,
                            message: Some(msg),
                        })
                        .await;
                }
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
    supervisor
        .list_apps()
        .await
        .into_iter()
        .map(|app| EnvApp {
            slug: app.slug,
            name: app.name,
            stack: app.stack,
            port: app.port,
            version: app.version,
            running: app.status == supervisor::AppProcessStatus::Running,
            has_db: app.has_db,
            public: false, // Set by orchestrator, not by env-agent
        })
        .collect()
}

/// Read a u64 from a cgroup file (e.g., memory.current).
/// Returns None if file doesn't exist or contains "max".
fn read_u64_file(path: &str) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed == "max" { return None; }
    trimmed.parse().ok()
}

/// Get number of CPUs available (for normalizing cgroup CPU usage).
fn num_cpus() -> Option<u32> {
    let content = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    Some(content.matches("processor").count() as u32)
}
