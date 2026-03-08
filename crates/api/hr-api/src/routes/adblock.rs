use axum::{
    extract::{Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/stats", get(stats))
        .route("/whitelist", get(get_whitelist).post(add_whitelist))
        .route("/whitelist/{domain}", delete(remove_whitelist))
        .route("/update", post(trigger_update))
        .route("/search", get(search))
}

async fn stats(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.adblock_stats().await {
        Ok(s) => {
            let sources: Vec<Value> = s.sources.iter().map(|src| {
                json!({"name": src.name, "url": src.url})
            }).collect();
            Json(json!({
                "success": true,
                "stats": {
                    "domainCount": s.domain_count,
                    "sources": sources,
                    "lastUpdate": s.last_update,
                    "enabled": s.enabled
                }
            }))
        }
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

async fn get_whitelist(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.adblock_whitelist_list().await {
        Ok(domains) => Json(json!({"success": true, "domains": domains})),
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

#[derive(Deserialize)]
struct AddWhitelistRequest {
    domain: String,
}

async fn add_whitelist(
    State(state): State<ApiState>,
    Json(body): Json<AddWhitelistRequest>,
) -> Json<Value> {
    let domain = body.domain.to_lowercase().trim().to_string();
    if domain.is_empty() {
        return Json(json!({"success": false, "error": "Domain requis"}));
    }

    match state.netcore.adblock_whitelist_add(&domain).await {
        Ok(resp) if resp.ok => Json(json!({"success": true, "domain": domain})),
        Ok(resp) => Json(json!({
            "success": false,
            "error": resp.error.unwrap_or_else(|| "Unknown error".into())
        })),
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

async fn remove_whitelist(
    State(state): State<ApiState>,
    axum::extract::Path(domain): axum::extract::Path<String>,
) -> Json<Value> {
    let domain = domain.to_lowercase();

    match state.netcore.adblock_whitelist_remove(&domain).await {
        Ok(resp) if resp.ok => Json(json!({"success": true})),
        Ok(resp) => Json(json!({
            "success": false,
            "error": resp.error.unwrap_or_else(|| "Unknown error".into())
        })),
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

async fn trigger_update(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.adblock_update().await {
        Ok(result) => {
            let source_results: Vec<Value> = result.sources.iter().map(|r| {
                json!({"name": r.name, "domains": r.domains})
            }).collect();
            Json(json!({
                "success": true,
                "total_domains": result.total_domains,
                "sources": source_results
            }))
        }
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn search(
    State(state): State<ApiState>,
    Query(query): Query<SearchQuery>,
) -> Json<Value> {
    let q = query.q.unwrap_or_default();
    if q.is_empty() {
        return Json(json!({"success": true, "results": [], "query": ""}));
    }

    match state.netcore.adblock_search(&q, Some(50)).await {
        Ok(result) => Json(json!({
            "success": true,
            "query": result.query,
            "is_blocked": result.is_blocked,
            "results": result.results
        })),
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}
