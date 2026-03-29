//! Internal reverse proxy for environment apps.
//!
//! Listens on port 80 inside the container and routes incoming requests
//! to the correct app based on the Host header:
//!   {app_slug}.{env_slug}.{base_domain} → localhost:{app_port}
//!   studio.{env_slug}.{base_domain}     → localhost:8443 (code-server)
//!   code.{env_slug}.{base_domain}       → localhost:8443 (code-server)

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode, Uri};
use axum::response::IntoResponse;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tracing::{debug, warn};

use crate::supervisor::AppSupervisor;
use hr_environment::config::EnvAgentConfig;

/// State shared by the app proxy handler.
#[derive(Clone)]
pub struct AppProxyState {
    pub supervisor: Arc<AppSupervisor>,
    pub config: Arc<EnvAgentConfig>,
    pub http_client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
}

/// Axum handler: proxy all requests to the correct app based on Host header.
pub async fn proxy_handler(
    State(state): State<AppProxyState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Extract app slug from host: {app_slug}.{env_slug}.{domain}
    let domain = host.split(':').next().unwrap_or(&host);
    let app_slug = domain.split('.').next().unwrap_or("").to_string();

    // Resolve target port
    let target_port = if app_slug == "studio" || app_slug == "code" {
        state.config.code_server_port
    } else {
        // Find app port from config
        match state.config.apps.iter().find(|a| a.slug == app_slug.as_str()) {
            Some(app) => app.port,
            None => {
                warn!(host, app_slug, "app not found for proxy");
                return (StatusCode::NOT_FOUND, format!("App '{}' not found", app_slug))
                    .into_response();
            }
        }
    };

    // Build target URI
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let target_uri = format!("http://127.0.0.1:{}{}", target_port, path_and_query);
    let uri: Uri = match target_uri.parse() {
        Ok(u) => u,
        Err(e) => {
            warn!(target_uri, error = %e, "invalid target URI");
            return (StatusCode::BAD_GATEWAY, "Invalid target URI").into_response();
        }
    };

    debug!(app_slug, target_port, path = path_and_query, "proxying request");

    // Forward the request
    let (mut parts, body) = req.into_parts();
    parts.uri = uri;

    let forwarded_req = Request::from_parts(parts, body);

    match state.http_client.request(forwarded_req).await {
        Ok(resp) => resp.into_response(),
        Err(e) => {
            debug!(app_slug, error = %e, "upstream error");
            (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", e)).into_response()
        }
    }
}
