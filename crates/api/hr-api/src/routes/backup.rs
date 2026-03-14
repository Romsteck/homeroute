//! Backup pipeline API routes
//! POST /api/backup/trigger  — start the backup pipeline
//! GET  /api/backup/status   — get pipeline status (global)
//! GET  /api/backup/repos    — get per-repo status
//! GET  /api/backup/jobs     — get job history

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
        .route("/repos", get(get_backup_repos))
        .route("/jobs", get(get_backup_jobs))
}

/// POST /api/backup/trigger
/// Starts the backup pipeline (WOL → rustic backup per repo → sleep).
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

/// GET /api/backup/repos
/// Returns per-repo backup status (last backup time, success, snapshot ID, etc.).
async fn get_backup_repos(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetBackupRepos)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!([]))),
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

/// GET /api/backup/jobs
/// Returns job history (last 20 jobs, most recent first).
async fn get_backup_jobs(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetBackupJobs)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!([]))),
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
