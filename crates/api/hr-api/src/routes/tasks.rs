use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_tasks))
        .route("/active", get(get_active_tasks))
        .route("/{id}", get(get_task))
        .route("/{id}/cancel", post(cancel_task))
}

#[derive(Deserialize)]
struct ListParams {
    limit: Option<u32>,
    offset: Option<u32>,
    status: Option<String>,
}

async fn list_tasks(
    State(state): State<ApiState>,
    Query(params): Query<ListParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(30).min(100);
    let offset = params.offset.unwrap_or(0);
    let (tasks, total) = state
        .task_store
        .list_tasks(limit, offset, params.status.as_deref())
        .await;
    Json(json!({ "tasks": tasks, "total": total }))
}

async fn get_active_tasks(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let tasks = state.task_store.get_active_tasks().await;
    Json(json!(tasks))
}

async fn get_task(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.task_store.get_task(&id).await {
        Some(task) => {
            let steps = state.task_store.get_steps(&id).await;
            Json(json!({ "task": task, "steps": steps }))
        }
        None => Json(json!({ "error": "Task not found" })),
    }
}

async fn cancel_task(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.task_store.get_task(&id).await {
        Some(task) => {
            if task.status == hr_common::tasks::TaskStatus::Pending
                || task.status == hr_common::tasks::TaskStatus::Running
            {
                state
                    .task_store
                    .update_task_status(&id, hr_common::tasks::TaskStatus::Cancelled, None)
                    .await;
                Json(json!({ "success": true }))
            } else {
                Json(json!({ "success": false, "error": "Task is not active" }))
            }
        }
        None => Json(json!({ "success": false, "error": "Task not found" })),
    }
}
