//! HTTP webhook handlers for hr-orchestrator.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use tracing::info;

use hr_pipeline::PipelineEngine;
use hr_pipeline::PipelineStore;

/// Shared state for hook handlers.
#[derive(Clone)]
pub struct HookState {
    /// Will be used by Phase 5 to trigger promote_chain().
    #[allow(dead_code)]
    pub pipeline_engine: Arc<PipelineEngine>,
    pub pipeline_store: Arc<PipelineStore>,
}

#[derive(Deserialize)]
pub struct GitPushPayload {
    pub slug: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit: String,
}

/// Handle git push webhook from post-receive hooks.
///
/// Called by the post-receive hook script installed by `GitService::setup_pipeline_hook`.
/// Checks the PipelineConfig for auto-promote settings and logs the push event.
pub async fn handle_git_push_hook(
    State(state): State<HookState>,
    Json(payload): Json<GitPushPayload>,
) -> StatusCode {
    info!(
        slug = %payload.slug,
        git_ref = %payload.git_ref,
        commit = %payload.commit,
        "Git push hook received"
    );

    // Only trigger on main/master
    if !payload.git_ref.ends_with("/main") && !payload.git_ref.ends_with("/master") {
        return StatusCode::OK;
    }

    // Version = short commit SHA
    let version = if payload.commit.len() > 7 {
        payload.commit[..7].to_string()
    } else {
        payload.commit.clone()
    };

    // Look up pipeline config for this app
    let config = state.pipeline_store.get_config(&payload.slug).await;

    // Check if auto-promote is enabled for the first env in the chain
    if let Some(config) = config {
        if !config.env_chain.is_empty() {
            let first_env = &config.env_chain[0];
            if config.auto_promote.contains(first_env) {
                info!(
                    slug = %payload.slug,
                    version = %version,
                    target = %first_env,
                    "Auto-promote eligible — pipeline will be triggered when promote_chain() is implemented (Phase 5)"
                );
                // TODO: trigger pipeline promote here via promote_chain()
                // This will be fully wired in Phase 5 when promote_chain() is implemented
            }
        }
    } else {
        info!(
            slug = %payload.slug,
            "No pipeline config found for app, skipping auto-promote"
        );
    }

    StatusCode::OK
}
