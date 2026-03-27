//! Discovery endpoint — exposes running apps and their ports.
//!
//! hr-edge queries GET /discovery to know how to route traffic.

use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use crate::supervisor::{AppProcessStatus, AppSupervisor};

/// Discovery response returned by GET /discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    pub env_slug: String,
    pub apps: Vec<DiscoveryApp>,
}

/// An app entry in the discovery response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryApp {
    pub slug: String,
    pub port: u16,
    pub running: bool,
    pub version: Option<String>,
}

/// Shared state for the discovery endpoint.
#[derive(Clone)]
pub struct DiscoveryState {
    pub env_slug: String,
    pub supervisor: Arc<AppSupervisor>,
}

/// Build the discovery router with a single GET /discovery endpoint.
pub fn router(state: DiscoveryState) -> Router {
    Router::new()
        .route("/discovery", get(handle_discovery))
        .with_state(state)
}

/// Handler for GET /discovery.
///
/// Returns JSON listing all apps managed by this environment with their
/// ports and running status. hr-edge uses this to build routing tables.
async fn handle_discovery(State(state): State<DiscoveryState>) -> impl IntoResponse {
    let apps = state
        .supervisor
        .list_apps()
        .await
        .into_iter()
        .map(|app| DiscoveryApp {
            slug: app.slug,
            port: app.port,
            running: app.status == AppProcessStatus::Running,
            version: app.version,
        })
        .collect();

    Json(DiscoveryResponse {
        env_slug: state.env_slug.clone(),
        apps,
    })
}
