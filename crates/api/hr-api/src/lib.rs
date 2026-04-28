pub mod middleware;
pub mod routes;
pub mod state;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use state::ApiState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub fn build_router(state: ApiState) -> Router {
    let web_dist = state.env.web_dist_path.clone();
    let index_html = web_dist.join("index.html");

    let spa_fallback = ServeDir::new(&web_dist).fallback(ServeFile::new(&index_html));

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
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
        .allow_credentials(true);

    Router::new()
        .nest("/api", api_routes())
        .with_state(state)
        .layer(cors)
        .layer(axum::middleware::from_fn(middleware::request_logging))
        .fallback_service(spa_fallback)
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
        .merge(routes::apps::router())
        .merge(routes::apps_db::router())
        .nest("/edge/routes", routes::edge::router())
        .nest("/edge/stats", routes::edge_stats::router())
        .nest("/store", routes::store::router())
        .nest("/git", routes::git::router())
        .merge(routes::ws::router())
        .nest("/backup", routes::backup::router())
        .nest("/tasks", routes::tasks::router())
        .nest("/docs", routes::docs::router())
        .nest("/logs", routes::logs::router())
        .merge(routes::health::router())
}
