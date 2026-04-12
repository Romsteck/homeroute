//! Backup pipeline API routes
//! POST /api/backup/trigger   — start the backup pipeline
//! GET  /api/backup/status    — get pipeline status (global)
//! GET  /api/backup/repos     — get per-repo status
//! GET  /api/backup/jobs      — get job history
//! GET  /api/backup/progress  — get live in-flight progress

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use hr_common::tasks::{TaskContext, TaskTrigger, TaskType};
use hr_ipc::orchestrator::OrchestratorRequest;
use serde_json::json;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/trigger", post(trigger_backup))
        .route("/cancel", post(cancel_backup))
        .route("/status", get(get_backup_status))
        .route("/repos", get(get_backup_repos))
        .route("/jobs", get(get_backup_jobs))
        .route("/progress", get(get_backup_progress))
}

async fn trigger_backup(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let task = state
        .task_store
        .create_task(
            TaskType::BackupTrigger,
            "Sauvegarde manuelle",
            TaskTrigger::User("admin".to_string()),
            None,
        )
        .await;
    let task_id = task.id.clone();
    let task_ctx = TaskContext::new(
        task.id.clone(),
        state.task_store.clone(),
        state.events.clone(),
    );
    task_ctx.start().await;

    // Respond 202 immediately, spawn IPC call + tracker in background
    let orchestrator = state.orchestrator.clone();
    let events = state.events.clone();
    tokio::spawn(async move {
        let step = task_ctx
            .step("trigger", "Démarrage du pipeline de sauvegarde")
            .await;

        match orchestrator
            .request(&OrchestratorRequest::TriggerBackup)
            .await
        {
            Ok(resp) if resp.ok => {
                step.complete().await;
                // Track backup_live events for completion
                let step = task_ctx.step("backup", "Sauvegarde en cours").await;
                let mut rx = events.backup_live.subscribe();
                let timeout = tokio::time::sleep(std::time::Duration::from_secs(4 * 3600));
                tokio::pin!(timeout);
                loop {
                    tokio::select! {
                        result = rx.recv() => {
                            match result {
                                Ok(ev) => {
                                    let running = ev.progress.get("running")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    if !running {
                                        step.complete().await;
                                        task_ctx.done().await;
                                        return;
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                Err(_) => {}
                            }
                        }
                        _ = &mut timeout => {
                            step.fail("Timeout after 4 hours").await;
                            task_ctx.fail("Backup timed out").await;
                            return;
                        }
                    }
                }
                task_ctx.done().await;
            }
            Ok(resp) => {
                let err = resp.error.as_deref().unwrap_or("Unknown error");
                step.fail(err).await;
                task_ctx.fail(err).await;
            }
            Err(e) => {
                let err = format!("IPC error: {e}");
                step.fail(&err).await;
                task_ctx.fail(&err).await;
            }
        }
    });

    Json(json!({
        "message": "Backup pipeline started",
        "task_id": task_id,
    }))
}

async fn cancel_backup(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::CancelBackup)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!({"message": "Backup cancelled"}))),
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

async fn get_backup_progress(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetBackupProgress)
        .await
    {
        Ok(resp) if resp.ok => Json(resp.data.unwrap_or(json!({"running": false}))),
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
