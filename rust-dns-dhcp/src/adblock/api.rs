use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::shared::SharedState;
use super::sources;

#[derive(Serialize)]
struct StatsResponse {
    domain_count: usize,
    last_update: Option<String>,
    sources: Vec<SourceInfo>,
}

#[derive(Serialize)]
struct SourceInfo {
    name: String,
    url: String,
    count: usize,
}

#[derive(Serialize)]
struct WhitelistResponse {
    success: bool,
    domains: Vec<String>,
}

#[derive(Deserialize)]
struct WhitelistAddRequest {
    domain: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Serialize)]
struct SearchResponse {
    success: bool,
    results: Vec<String>,
}

#[derive(Serialize)]
struct UpdateResponse {
    success: bool,
    message: String,
    domain_count: usize,
}

#[derive(Serialize)]
struct MessageResponse {
    success: bool,
    message: String,
}

/// State shared between adblock API handlers
struct AdblockApiState {
    shared: SharedState,
    /// Per-source domain counts from the last update
    source_counts: RwLock<Vec<(String, String, usize)>>, // (name, url, count)
    last_update: RwLock<Option<String>>,
}

/// Build the adblock API router
pub fn build_router(state: SharedState) -> Router {
    let api_state = Arc::new(AdblockApiState {
        shared: state,
        source_counts: RwLock::new(Vec::new()),
        last_update: RwLock::new(None),
    });

    Router::new()
        .route("/stats", get(get_stats))
        .route("/whitelist", get(get_whitelist))
        .route("/whitelist", post(add_to_whitelist))
        .route("/whitelist/{domain}", delete(remove_from_whitelist))
        .route("/update", post(trigger_update))
        .route("/search", get(search_blocked))
        .with_state(api_state)
}

async fn get_stats(State(state): State<Arc<AdblockApiState>>) -> Json<StatsResponse> {
    let shared = state.shared.read().await;
    let domain_count = shared.adblock.domain_count();
    let source_counts = state.source_counts.read().await;
    let last_update = state.last_update.read().await;

    let sources_info: Vec<SourceInfo> = if source_counts.is_empty() {
        // Fall back to config sources with 0 counts
        shared
            .config
            .adblock
            .sources
            .iter()
            .map(|s| SourceInfo {
                name: s.name.clone(),
                url: s.url.clone(),
                count: 0,
            })
            .collect()
    } else {
        source_counts
            .iter()
            .map(|(name, url, count)| SourceInfo {
                name: name.clone(),
                url: url.clone(),
                count: *count,
            })
            .collect()
    };

    Json(StatsResponse {
        domain_count,
        last_update: last_update.clone(),
        sources: sources_info,
    })
}

async fn get_whitelist(State(state): State<Arc<AdblockApiState>>) -> Json<WhitelistResponse> {
    let shared = state.shared.read().await;
    Json(WhitelistResponse {
        success: true,
        domains: shared.adblock.whitelist_domains(),
    })
}

async fn add_to_whitelist(
    State(state): State<Arc<AdblockApiState>>,
    Json(req): Json<WhitelistAddRequest>,
) -> (StatusCode, Json<MessageResponse>) {
    let domain = req.domain.to_lowercase();
    if domain.is_empty() || !domain.contains('.') {
        return (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                success: false,
                message: "Invalid domain".to_string(),
            }),
        );
    }

    {
        let mut shared = state.shared.write().await;
        shared.config.adblock.whitelist.push(domain.clone());
        let wl = shared.config.adblock.whitelist.clone();
        shared.adblock.set_whitelist(wl);

        // Save config
        let config_path = std::env::var("DNS_DHCP_CONFIG_PATH")
            .unwrap_or_else(|_| "/var/lib/server-dashboard/dns-dhcp-config.json".to_string());
        if let Err(e) = shared.config.save_to_file(std::path::Path::new(&config_path)) {
            error!("Failed to save config: {}", e);
        }
    }

    info!("Added {} to adblock whitelist", domain);
    (
        StatusCode::OK,
        Json(MessageResponse {
            success: true,
            message: format!("Added {} to whitelist", domain),
        }),
    )
}

async fn remove_from_whitelist(
    State(state): State<Arc<AdblockApiState>>,
    Path(domain): Path<String>,
) -> (StatusCode, Json<MessageResponse>) {
    let domain = domain.to_lowercase();

    {
        let mut shared = state.shared.write().await;
        shared.config.adblock.whitelist.retain(|d| d.to_lowercase() != domain);
        let wl = shared.config.adblock.whitelist.clone();
        shared.adblock.set_whitelist(wl);

        let config_path = std::env::var("DNS_DHCP_CONFIG_PATH")
            .unwrap_or_else(|_| "/var/lib/server-dashboard/dns-dhcp-config.json".to_string());
        if let Err(e) = shared.config.save_to_file(std::path::Path::new(&config_path)) {
            error!("Failed to save config: {}", e);
        }
    }

    info!("Removed {} from adblock whitelist", domain);
    (
        StatusCode::OK,
        Json(MessageResponse {
            success: true,
            message: format!("Removed {} from whitelist", domain),
        }),
    )
}

async fn trigger_update(
    State(state): State<Arc<AdblockApiState>>,
) -> Json<UpdateResponse> {
    let sources_config = {
        let shared = state.shared.read().await;
        shared.config.adblock.sources.clone()
    };

    let (domains, results) = sources::download_all(&sources_config).await;
    let domain_count = domains.len();

    // Update filter
    {
        let mut shared = state.shared.write().await;
        shared.adblock.set_blocked(domains.clone());

        // Save cache
        let data_dir = &shared.config.adblock.data_dir;
        let cache_path = std::path::PathBuf::from(data_dir).join("domains.json");
        if let Err(e) = sources::save_cache(&domains, &cache_path) {
            error!("Failed to save adblock cache: {}", e);
        }
    }

    // Update source counts
    {
        let sources_config = state.shared.read().await.config.adblock.sources.clone();
        let mut counts = state.source_counts.write().await;
        *counts = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let url = sources_config.get(i).map(|s| s.url.clone()).unwrap_or_default();
                (r.name.clone(), url, r.domain_count)
            })
            .collect();
    }

    // Update last update timestamp
    {
        let mut last_update = state.last_update.write().await;
        *last_update = Some(chrono::Utc::now().to_rfc3339());
    }

    info!("Adblock update complete: {} domains", domain_count);

    Json(UpdateResponse {
        success: true,
        message: "Update completed".to_string(),
        domain_count,
    })
}

async fn search_blocked(
    State(state): State<Arc<AdblockApiState>>,
    Query(params): Query<SearchQuery>,
) -> (StatusCode, Json<SearchResponse>) {
    if params.q.len() < 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(SearchResponse {
                success: false,
                results: vec![],
            }),
        );
    }

    let shared = state.shared.read().await;
    let results = shared.adblock.search(&params.q, 100);

    (
        StatusCode::OK,
        Json(SearchResponse {
            success: true,
            results,
        }),
    )
}
