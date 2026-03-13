//! Backup pipeline API routes
//! POST /api/backup/trigger  — start the backup pipeline
//! GET  /api/backup/status   — get pipeline status

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use hr_ipc::orchestrator::OrchestratorRequest;
use serde_json::json;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/trigger", post(trigger_backup))
        .route("/status", get(get_backup_status))
}

/// POST /api/backup/trigger
/// Starts the backup pipeline (WOL → borg backup → sleep).
/// Returns immediately; the pipeline runs asynchronously.
async fn trigger_backup(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::TriggerBackup)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!({"message": "Backup pipeline started"}))),
        Ok(resp) => Json(json!({
            "success": false,
            "error": resp.error.unwrap_or_else(|| "Unknown error".to_string())
        })),
        Err(e) => Json(json!({
            "success": false,
            "error": format!("IPC error: {e}")
        })),
    }
}

/// GET /api/backup/status
/// Returns the current backup pipeline status and last run result.
async fn get_backup_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetBackupStatus)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!({}))),
        Ok(resp) => Json(json!({
            "success": false,
            "error": resp.error.unwrap_or_else(|| "Unknown error".to_string())
        })),
        Err(e) => Json(json!({
            "success": false,
            "error": format!("IPC error: {e}")
        })),
    }
}
