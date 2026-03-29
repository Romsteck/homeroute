pub mod routes;
pub mod state;

use axum::http::{header, HeaderValue, Method};
use axum::Router;
use state::ApiState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub fn build_router(state: ApiState) -> Router {
    let web_dist = state.env.web_dist_path.clone();
    let index_html = web_dist.join("index.html");

    let spa_fallback = ServeDir::new(&web_dist)
        .fallback(ServeFile::new(&index_html));

    let base = format!(".{}", state.env.base_domain);
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            origin
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("https://"))
                .map(|host| host.ends_with(&base))
                .unwrap_or(false)
        }))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
        .allow_credentials(true);

    Router::new()
        .nest("/api", api_routes())
        .with_state(state)
        .layer(cors)
        .fallback_service(spa_fallback)
}

/// Build a separate router for the Maker Portal (make.mynetwk.biz).
/// Serves web-make/dist/ with SPA fallback + shares the same /api routes.
/// Runs on a dedicated port (4002).
pub fn build_make_router(state: ApiState) -> Router {
    // web_dist_path = /opt/homeroute/web/dist → parent.parent = /opt/homeroute
    let homeroute_root = state.env.web_dist_path.parent()
        .and_then(|p| p.parent())
        .unwrap_or(&state.env.web_dist_path);
    let make_dist = homeroute_root.join("web-make").join("dist");
    let make_index = make_dist.join("index.html");

    tracing::info!(path = %make_dist.display(), "Maker Portal dist path");

    let make_spa = ServeDir::new(&make_dist)
        .fallback(ServeFile::new(&make_index));

    let base = format!(".{}", state.env.base_domain);
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            origin
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("https://"))
                .map(|host| host.ends_with(&base))
                .unwrap_or(false)
        }))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
        .allow_credentials(true);

    Router::new()
        .nest("/api", api_routes())
        .with_state(state)
        .layer(cors)
        .fallback_service(make_spa)
}

/// Build a separate router for the Studio (studio.{env}.mynetwk.biz).
/// Serves web-studio-env/dist/ with SPA fallback + shares the same /api routes.
/// Runs on a dedicated port (4003).
pub fn build_studio_router(state: ApiState) -> Router {
    let homeroute_root = state.env.web_dist_path.parent()
        .and_then(|p| p.parent())
        .unwrap_or(&state.env.web_dist_path);
    let studio_dist = homeroute_root.join("web-studio-env").join("dist");
    let studio_index = studio_dist.join("index.html");

    tracing::info!(path = %studio_dist.display(), "Studio dist path");

    let studio_spa = ServeDir::new(&studio_dist)
        .fallback(ServeFile::new(&studio_index));

    let base = format!(".{}", state.env.base_domain);
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            origin
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("https://"))
                .map(|host| host.ends_with(&base))
                .unwrap_or(false)
        }))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
        .allow_credentials(true);

    Router::new()
        .nest("/api", api_routes())
        .with_state(state)
        .layer(cors)
        .fallback_service(studio_spa)
}

fn api_routes() -> Router<ApiState> {
    Router::new()
        .nest("/auth", routes::auth::router())
        .nest("/dashboard", routes::dashboard::router())
        .nest("/dns-dhcp", routes::dns_dhcp::router())
        .nest("/dns", routes::dns::router())
        .nest("/adblock", routes::adblock::router())

        .nest("/ddns", routes::ddns::router())
        .nest("/reverseproxy", routes::reverseproxy::router())
        .nest("/rust-proxy", routes::rust_proxy::router())
        .nest("/acme", routes::acme::router())
        .nest("/energy", routes::energy::router())
        .nest("/updates", routes::updates::router())
        .nest("/hosts", routes::hosts::router())
        .nest("/services", routes::services::router())

        .nest("/applications", routes::applications::router())
        .nest("/edge/stats", routes::edge_stats::router())
        .nest("/store", routes::store::router())
        .nest("/git", routes::git::router())
        .merge(routes::ws::router())
        .nest("/backup", routes::backup::router())
        .nest("/tasks", routes::tasks::router())
        .nest("/docs", routes::docs::router())
        .merge(routes::environments::router())
        .merge(routes::health::router())
}
