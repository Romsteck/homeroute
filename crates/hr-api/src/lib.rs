pub mod routes;
pub mod state;

use axum::http::{header, HeaderValue, Method};
use axum::Router;
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use state::{ApiState, AppState};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

/// Build the complete router: JSON API on `/api/*`, Leptos SSR for everything else.
pub fn build_router(state: AppState) -> Router {
    let base = format!(".{}", state.api.env.base_domain);
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

    // API routes: built with ApiState, finalized via .with_state()
    let api: Router = api_routes().with_state(state.api.clone());

    // Leptos SSR routes: Router<AppState> then finalized via .with_state()
    let routes = generate_route_list(hr_web::app::App);
    let site_root = state.leptos_options.site_root.to_string();
    let auth_ctx = state.api.auth.clone();
    let leptos: Router = Router::<AppState>::new()
        .leptos_routes_with_context(
            &state,
            routes,
            move || {
                provide_context(auth_ctx.clone());
            },
            {
                let leptos_options = state.leptos_options.clone();
                move || hr_web::app::shell(leptos_options.clone())
            },
        )
        .with_state(state);

    // Merge: API takes priority (nested under /api), then Leptos SSR,
    // then static files (WASM, CSS, assets) from target/site/
    Router::new()
        .nest("/api", api)
        .merge(leptos)
        .fallback_service(ServeDir::new(site_root))
        .layer(cors)
}

fn api_routes() -> Router<ApiState> {
    Router::new()
        .nest("/auth", routes::auth::router())
        .nest("/users", routes::users::router())
        .nest("/dns-dhcp", routes::dns_dhcp::router())
        .nest("/dns", routes::dns::router())
        .nest("/adblock", routes::adblock::router())
        .nest("/network", routes::network::router())
        .nest("/nat", routes::nat::router())
        .nest("/ddns", routes::ddns::router())
        .nest("/reverseproxy", routes::reverseproxy::router())
        .nest("/rust-proxy", routes::rust_proxy::router())
        .nest("/acme", routes::acme::router())
        .nest("/energy", routes::energy::router())
        .nest("/updates", routes::updates::router())
        .nest("/traffic", routes::traffic::router())
        .nest("/servers", routes::servers::router())
        .nest("/wol", routes::wol::router())
        .nest("/services", routes::services::router())
        .nest("/firewall", routes::firewall::router())
        .nest("/applications", routes::applications::router())
        .merge(routes::ws::router())
        .merge(routes::health::router())
}
