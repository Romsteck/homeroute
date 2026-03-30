mod ipc_handler;

use hr_acme::{AcmeConfig, AcmeManager, WildcardType};
use hr_auth::AuthService;
use hr_common::config::EnvConfig;
use hr_common::events::{CertReadyEvent, EventBus};
use hr_common::service_registry::new_service_registry;
use hr_common::supervisor::{spawn_supervised, ServicePriority};
use hr_proxy::{ProxyConfig, ProxyState, TlsManager};
use signal_hook::consts::SIGHUP;
use signal_hook_tokio::Signals;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hr_edge=debug".parse().unwrap()),
        )
        .init();

    info!("hr-edge starting...");

    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Load environment config
    let env = EnvConfig::load(None);
    info!("Base domain: {}", env.base_domain);

    // Initialize event bus
    let events = Arc::new(EventBus::new());

    // Initialize service registry (local to hr-edge)
    let service_registry = new_service_registry();

    // ── Auth ──────────────────────────────────────────────────────────
    let auth = AuthService::new(&env.auth_data_dir, &env.base_domain)?;
    auth.start_cleanup_task();
    info!("Auth service initialized");

    // ── ACME (Let's Encrypt) ─────────────────────────────────────────
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

    // Request global wildcard certificate if not present
    if acme.get_certificate(WildcardType::Global).is_err() {
        info!("Requesting global wildcard certificate...");
        match acme.request_wildcard(WildcardType::Global).await {
            Ok(cert) => info!(
                "Global wildcard certificate issued: {} (expires {})",
                cert.id, cert.expires_at
            ),
            Err(e) => warn!("Failed to request global wildcard: {}", e),
        }
    }

    // ── TLS ──────────────────────────────────────────────────────────
    let tls_manager = TlsManager::new(env.acme_storage_path.clone());

    // Load all certificates from ACME index
    let certs = acme.list_certificates().unwrap_or_default();
    for cert_info in &certs {
        let cert_path = std::path::Path::new(&cert_info.cert_path);
        let key_path = std::path::Path::new(&cert_info.key_path);
        if cert_path.exists() && key_path.exists() {
            match tls_manager.load_cert_from_files(cert_path, key_path) {
                Ok(certified_key) => {
                    let domain = cert_info.wildcard_type.domain_pattern(&env.base_domain);
                    tls_manager.add_cert(&domain, certified_key);
                    info!(domain = %domain, "Loaded certificate");
                }
                Err(e) => {
                    warn!(cert_id = %cert_info.id, error = %e, "Failed to load certificate");
                }
            }
        }
    }

    // Set global wildcard as fallback for unknown SNI domains
    if let Ok(cert_info) = acme.get_certificate(WildcardType::Global) {
        if let Err(e) = tls_manager.set_fallback_certificate_from_pem(
            &cert_info.cert_path,
            &cert_info.key_path,
        ) {
            warn!("Failed to set fallback certificate: {}", e);
        }
    }

    // Restrict fallback cert to local domain only
    tls_manager.resolver.set_local_domain(&env.base_domain);

    let tls_config = tls_manager.build_server_config()?;

    // ── Proxy ────────────────────────────────────────────────────────
    let proxy_config_path = env.proxy_config_path.clone();
    let proxy_config = if proxy_config_path.exists() {
        ProxyConfig::load_from_file(&proxy_config_path)?
    } else {
        ProxyConfig {
            base_domain: env.base_domain.clone(),
            ca_storage_path: env.acme_storage_path.clone(),
            ..serde_json::from_str("{}")?
        }
    };

    let orchestrator_port = env.orchestrator_port;
    let env_route_cache = Arc::new(hr_proxy::EnvRouteCache::new());
    let proxy_state = Arc::new(
        ProxyState::new(proxy_config.clone(), env.api_port, orchestrator_port)
            .with_auth(auth.clone())
            .with_app_routes_path(env.data_dir.join("app-routes.json"))
            .with_env_route_cache(env_route_cache.clone()),
    );

    let https_port = proxy_config.https_port;
    let http_port = proxy_config.http_port;

    info!(
        "Loaded {} TLS certificates for {} active routes",
        tls_manager.loaded_domains().len(),
        proxy_config.active_routes().len()
    );

    // ── Env route cache refresh + app route sync (every 30s from orchestrator) ─
    {
        let cache = env_route_cache.clone();
        tokio::spawn(async move {
            let client = hr_ipc::orchestrator::OrchestratorClient::new("/run/hr-orchestrator.sock");
            // Wait 10s for orchestrator to start
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            loop {
                match client.request(&hr_ipc::orchestrator::OrchestratorRequest::ListEnvironments).await {
                    Ok(resp) if resp.ok => {
                        if let Some(data) = resp.data {
                            match serde_json::from_value::<Vec<hr_proxy::env_routes::EnvRouteSummary>>(data.clone()) {
                                Ok(envs) => {
                                    info!("Env route cache: {} environments loaded", envs.len());
                                    cache.update(envs);
                                }
                                Err(e) => {
                                    warn!("Failed to parse env routes: {}", e);
                                    debug!("Raw data: {}", serde_json::to_string(&data).unwrap_or_default());
                                }
                            }
                        }
                    }
                    Ok(resp) => {
                        warn!("ListEnvironments IPC error: {:?}", resp.error);
                    }
                    Err(e) => {
                        warn!("Env route cache refresh failed: {}", e);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    // ── Store shared refs for SIGHUP reload ──────────────────────────
    let proxy_state_reload = proxy_state.clone();
    let proxy_config_path_reload = env.proxy_config_path.clone();
    let tls_manager = Arc::new(tls_manager);
    let tls_manager_reload = tls_manager.clone();

    // ── Spawn supervised services ────────────────────────────────────
    info!("Starting supervised services...");

    // HTTPS proxy (Critical)
    {
        let proxy_state_c = proxy_state.clone();
        let tls_config_c = tls_config.clone();
        let acme_c = acme.clone();
        let reg = service_registry.clone();
        spawn_supervised("proxy-https", ServicePriority::Critical, reg, move || {
            let proxy_state = proxy_state_c.clone();
            let tls_config = tls_config_c.clone();
            let acme = acme_c.clone();
            let port = https_port;
            async move { run_https_server(proxy_state, tls_config, acme, port).await }
        });
    }

    // HTTP redirect (Critical)
    {
        let base_domain = env.base_domain.clone();
        let reg = service_registry.clone();
        spawn_supervised("proxy-http", ServicePriority::Critical, reg, move || {
            let base_domain = base_domain.clone();
            let port = http_port;
            async move { run_http_redirect(port, &base_domain).await }
        });
    }

    // ── CertReady listener ───────────────────────────────────────────
    {
        let tls_mgr = tls_manager.clone();
        let mut cert_rx = events.cert_ready.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = cert_rx.recv().await {
                let cert_path = std::path::Path::new(&event.cert_path);
                let key_path = std::path::Path::new(&event.key_path);
                match tls_mgr.load_cert_from_files(cert_path, key_path) {
                    Ok(certified_key) => {
                        tls_mgr.add_cert(&event.wildcard_domain, certified_key);
                        info!(domain = %event.wildcard_domain, "Dynamically loaded new certificate");
                    }
                    Err(e) => {
                        warn!(domain = %event.wildcard_domain, error = %e, "Failed to load dynamic certificate");
                    }
                }
            }
        });
    }

    // ── Certificate renewal task (every 12h) ─────────────────────────
    {
        let acme_renewal = acme.clone();
        let events_renewal = events.clone();
        let base_domain_renewal = env.base_domain.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(12 * 3600)).await;
                info!("Checking for certificate renewals...");
                match acme_renewal.certificates_needing_renewal() {
                    Ok(certs) if !certs.is_empty() => {
                        for cert_info in certs {
                            info!(cert_id = %cert_info.id, "Renewing certificate");
                            match acme_renewal
                                .request_wildcard(cert_info.wildcard_type.clone())
                                .await
                            {
                                Ok(new_cert) => {
                                    let domain = new_cert
                                        .wildcard_type
                                        .domain_pattern(&base_domain_renewal);
                                    let _ = events_renewal.cert_ready.send(CertReadyEvent {
                                        slug: String::new(),
                                        wildcard_domain: domain,
                                        cert_path: new_cert.cert_path.clone(),
                                        key_path: new_cert.key_path.clone(),
                                    });
                                    info!(cert_id = %new_cert.id, "Certificate renewed successfully");
                                }
                                Err(e) => {
                                    warn!(cert_id = %cert_info.id, error = %e, "Failed to renew certificate");
                                }
                            }
                            // Stagger renewals to avoid rate limits
                            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        }
                    }
                    Ok(_) => debug!("No certificates need renewal"),
                    Err(e) => warn!(error = %e, "Failed to check certificate renewals"),
                }
            }
        });
    }

    // ── IPC server ───────────────────────────────────────────────────
    {
        let handler = Arc::new(ipc_handler::EdgeHandler {
            auth: auth.clone(),
            acme: acme.clone(),
            proxy: proxy_state.clone(),
            tls_manager: tls_manager.clone(),
            env: Arc::new(env.clone()),
        });

        let ipc_reg = service_registry.clone();
        spawn_supervised("ipc-server", ServicePriority::Critical, ipc_reg, move || {
            let handler = handler.clone();
            async move {
                hr_ipc::server::run_ipc_server(
                    std::path::Path::new("/run/hr-edge.sock"),
                    handler,
                )
                .await
            }
        });
    }

    // NOTE: Environment routing is handled by the EnvRouteCache (wildcard *.{env}.{domain}).
    // The cache is refreshed every 30s by the task spawned at line ~159.
    // No per-app routes needed — the env-agent internal proxy (port 80) dispatches to apps.

    // ── SIGHUP handler ───────────────────────────────────────────────
    let acme_sighup = acme.clone();
    let env_sighup = env.clone();
    tokio::spawn(async move {
        if let Err(e) = handle_sighup(
            proxy_config_path_reload,
            proxy_state_reload,
            tls_manager_reload,
            acme_sighup,
            env_sighup,
        )
        .await
        {
            error!("SIGHUP handler error: {}", e);
        }
    });

    // ── Ready ────────────────────────────────────────────────────────
    info!("hr-edge started successfully");
    info!("  Auth: OK");
    info!(
        "  ACME: OK ({} wildcard certificates)",
        acme.list_certificates().unwrap_or_default().len()
    );
    info!("  Proxy: HTTPS:{} HTTP:{}", https_port, http_port);
    info!("  IPC: /run/hr-edge.sock");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}

// ── Metrics endpoint (Prometheus text format, localhost only) ───────────

fn build_metrics_response(
    state: &Arc<ProxyState>,
    acme: &Arc<hr_acme::AcmeManager>,
) -> hyper::Response<axum::body::Body> {
    use std::fmt::Write;

    let global = state.metrics.global_snapshot();
    let domains = state.metrics.snapshot();
    let certs = acme.cert_expiry_info().unwrap_or_default();

    let mut out = String::with_capacity(2048);

    // Global counters
    let _ = writeln!(out, "# HELP hr_proxy_requests_total Total proxied requests.");
    let _ = writeln!(out, "# TYPE hr_proxy_requests_total counter");
    let _ = writeln!(out, "hr_proxy_requests_total {}", global.total_requests);

    let _ = writeln!(out, "# HELP hr_proxy_status_2xx_total Total 2xx responses.");
    let _ = writeln!(out, "# TYPE hr_proxy_status_2xx_total counter");
    let _ = writeln!(out, "hr_proxy_status_2xx_total {}", global.status_2xx);

    let _ = writeln!(out, "# HELP hr_proxy_status_4xx_total Total 4xx responses.");
    let _ = writeln!(out, "# TYPE hr_proxy_status_4xx_total counter");
    let _ = writeln!(out, "hr_proxy_status_4xx_total {}", global.status_4xx);

    let _ = writeln!(out, "# HELP hr_proxy_errors_5xx_total Total 5xx errors.");
    let _ = writeln!(out, "# TYPE hr_proxy_errors_5xx_total counter");
    let _ = writeln!(out, "hr_proxy_errors_5xx_total {}", global.status_5xx);

    let _ = writeln!(out, "# HELP hr_proxy_uptime_seconds Proxy uptime in seconds.");
    let _ = writeln!(out, "# TYPE hr_proxy_uptime_seconds gauge");
    let _ = writeln!(out, "hr_proxy_uptime_seconds {}", global.uptime_secs);

    let _ = writeln!(out, "# HELP hr_proxy_requests_per_second Average requests per second since start.");
    let _ = writeln!(out, "# TYPE hr_proxy_requests_per_second gauge");
    let _ = writeln!(out, "hr_proxy_requests_per_second {:.2}", global.requests_per_second);

    // Per-domain counters
    let _ = writeln!(out, "# HELP hr_proxy_domain_requests_total Requests per domain.");
    let _ = writeln!(out, "# TYPE hr_proxy_domain_requests_total counter");
    for d in &domains {
        let _ = writeln!(out, "hr_proxy_domain_requests_total{{domain=\"{}\"}} {}", d.domain, d.total_requests);
    }

    let _ = writeln!(out, "# HELP hr_proxy_domain_errors_5xx_total 5xx errors per domain.");
    let _ = writeln!(out, "# TYPE hr_proxy_domain_errors_5xx_total counter");
    for d in &domains {
        let _ = writeln!(out, "hr_proxy_domain_errors_5xx_total{{domain=\"{}\"}} {}", d.domain, d.errors_5xx);
    }

    // Certificate expiry
    let _ = writeln!(out, "# HELP hr_tls_cert_expiry_days Days until TLS certificate expires.");
    let _ = writeln!(out, "# TYPE hr_tls_cert_expiry_days gauge");
    for c in &certs {
        let _ = writeln!(
            out,
            "hr_tls_cert_expiry_days{{domain=\"{}\",type=\"{}\"}} {}",
            c.domain, c.wildcard_type, c.days_remaining
        );
    }

    let _ = writeln!(out, "# HELP hr_tls_cert_needs_renewal Whether certificate needs renewal (1=yes).");
    let _ = writeln!(out, "# TYPE hr_tls_cert_needs_renewal gauge");
    for c in &certs {
        let _ = writeln!(
            out,
            "hr_tls_cert_needs_renewal{{domain=\"{}\",type=\"{}\"}} {}",
            c.domain, c.wildcard_type, if c.needs_renewal { 1 } else { 0 }
        );
    }

    hyper::Response::builder()
        .status(200)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(axum::body::Body::from(out))
        .unwrap()
}

// ── HTTPS server ───────────────────────────────────────────────────────

async fn run_https_server(
    proxy_state: Arc<ProxyState>,
    tls_config: Arc<rustls::ServerConfig>,
    acme: Arc<hr_acme::AcmeManager>,
    port: u16,
) -> anyhow::Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use tokio_rustls::TlsAcceptor;

    let addr: SocketAddr = format!("[::]:{}", port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let acceptor = TlsAcceptor::from(tls_config);

    info!("HTTPS proxy listening on {}", addr);

    loop {
        let (tcp_stream, remote_addr) = match listener.accept().await {
            Ok(r) => r,
            Err(e) => {
                warn!("TCP accept error: {}", e);
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let proxy_state = proxy_state.clone();
        let acme = acme.clone();
        let client_ip = remote_addr.ip();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake failed from {}: {}", remote_addr, e);
                    return;
                }
            };

            let io = TokioIo::new(tls_stream);
            let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let state = proxy_state.clone();
                let acme = acme.clone();
                async move {
                    // Internal /metrics endpoint — localhost only
                    if req.uri().path() == "/metrics" && client_ip.is_loopback() {
                        return Ok::<_, std::convert::Infallible>(
                            build_metrics_response(&state, &acme),
                        );
                    }

                    let (parts, body) = req.into_parts();
                    let req =
                        axum::extract::Request::from_parts(parts, axum::body::Body::new(body));
                    let resp = hr_proxy::proxy_handler(state, client_ip, req).await;
                    Ok::<_, std::convert::Infallible>(axum::response::IntoResponse::into_response(
                        resp,
                    ))
                }
            });

            if let Err(e) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(io, service)
                .with_upgrades()
                .await
            {
                let msg = e.to_string();
                if !msg.contains("connection closed")
                    && !msg.contains("not connected")
                    && !msg.contains("connection reset")
                {
                    tracing::debug!("HTTP/1 connection error from {}: {}", remote_addr, e);
                }
            }
        });
    }
}

// ── HTTP redirect server ───────────────────────────────────────────────

async fn run_http_redirect(port: u16, _base_domain: &str) -> anyhow::Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;

    let addr: SocketAddr = format!("[::]:{}", port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("HTTP redirect listening on {}", addr);

    loop {
        let (stream, _remote) = match listener.accept().await {
            Ok(r) => r,
            Err(e) => {
                warn!("HTTP accept error: {}", e);
                continue;
            }
        };

        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            let service = service_fn(|req: hyper::Request<hyper::body::Incoming>| async move {
                let host = req
                    .headers()
                    .get("host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("localhost");
                let path = req
                    .uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/");
                let location = format!("https://{}{}", host, path);

                Ok::<_, std::convert::Infallible>(
                    hyper::Response::builder()
                        .status(301)
                        .header("Location", &location)
                        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
                        .unwrap(),
                )
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                let msg = e.to_string();
                if !msg.contains("connection closed") && !msg.contains("not connected") {
                    tracing::debug!("HTTP redirect error: {}", e);
                }
            }
        });
    }
}

// ── SIGHUP handler ─────────────────────────────────────────────────────

async fn handle_sighup(
    proxy_config_path: PathBuf,
    proxy_state: Arc<ProxyState>,
    tls_manager: Arc<TlsManager>,
    acme: Arc<hr_acme::AcmeManager>,
    env: hr_common::config::EnvConfig,
) -> anyhow::Result<()> {
    let mut signals = Signals::new([SIGHUP])?;

    while let Some(signal) = signals.next().await {
        if signal == SIGHUP {
            info!("Received SIGHUP, reloading proxy config...");

            match ProxyConfig::load_from_file(&proxy_config_path) {
                Ok(new_config) => {
                    if let Err(e) = tls_manager.reload_certificates(&new_config.routes) {
                        error!("Failed to reload TLS certificates: {}", e);
                    }
                    // Re-load ACME wildcard certs (reload_certificates does replace_all
                    // which wipes certs not referenced by routes)
                    reload_acme_certs(&tls_manager, &acme, &env.base_domain);
                    proxy_state.reload_config(new_config);
                    info!("Proxy config reloaded");
                }
                Err(e) => {
                    error!("Failed to reload proxy config: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Re-load all ACME wildcard certificates into the TLS manager.
/// Called after `reload_certificates` which does `replace_all` and would
/// otherwise wipe ACME certs that were loaded at startup.
fn reload_acme_certs(
    tls_manager: &TlsManager,
    acme: &hr_acme::AcmeManager,
    base_domain: &str,
) {
    let certs = acme.list_certificates().unwrap_or_default();
    let mut loaded = 0;
    for cert_info in &certs {
        let cert_path = std::path::Path::new(&cert_info.cert_path);
        let key_path = std::path::Path::new(&cert_info.key_path);
        if cert_path.exists() && key_path.exists() {
            match tls_manager.load_cert_from_files(cert_path, key_path) {
                Ok(certified_key) => {
                    let domain = cert_info.wildcard_type.domain_pattern(base_domain);
                    tls_manager.add_cert(&domain, certified_key);
                    loaded += 1;
                }
                Err(e) => {
                    warn!(cert_id = %cert_info.id, error = %e, "Failed to reload ACME cert");
                }
            }
        }
    }
    // Re-set fallback cert
    if let Ok(cert_info) = acme.get_certificate(hr_acme::WildcardType::Global) {
        if let Err(e) = tls_manager.set_fallback_certificate_from_pem(
            &cert_info.cert_path,
            &cert_info.key_path,
        ) {
            warn!("Failed to re-set fallback certificate: {}", e);
        }
    }
    info!("Re-loaded {} ACME certificates after config reload", loaded);
}
