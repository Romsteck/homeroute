use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};

use crate::state::ApiState;

/// Legacy DNS-only routes (compat with old dnsmasq-era frontend).
/// Most functionality is in /api/dns-dhcp.
pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/cache-stats", get(cache_stats))
        .route("/status", get(status))
}

async fn cache_stats(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.dns_cache_stats().await {
        Ok(stats) => Json(json!({
            "success": true,
            "cache_size": stats.cache_size,
            "adblock_enabled": stats.adblock_enabled
        })),
        Err(_) => Json(json!({
            "success": false,
            "error": "Network core unavailable"
        })),
    }
}

async fn status(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.dns_status().await {
        Ok(s) => Json(json!({
            "success": true,
            "active": s.active,
            "port": s.port,
            "upstream_servers": s.upstream_servers,
            "cache_size": s.cache_size,
            "local_domain": s.local_domain,
            "adblock_enabled": s.adblock_enabled
        })),
        Err(_) => Json(json!({
            "success": false,
            "error": "Network core unavailable"
        })),
    }
}
