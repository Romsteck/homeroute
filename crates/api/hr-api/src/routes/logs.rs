use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use hr_common::logging::LogQuery;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(get_logs))
        .route("/stats", get(get_stats))
}

async fn get_logs(
    State(state): State<ApiState>,
    Query(filter): Query<LogQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    match state.log_store.query(&filter).await {
        Ok(entries) => Ok(Json(serde_json::json!({ "logs": entries }))),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to query logs: {e}"),
        )),
    }
}

async fn get_stats(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    match state.log_store.stats().await {
        Ok(stats) => Ok(Json(serde_json::json!(stats))),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get log stats: {e}"),
        )),
    }
}
