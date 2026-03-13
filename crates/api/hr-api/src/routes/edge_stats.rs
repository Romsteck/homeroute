use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use hr_ipc::edge::EdgeRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new().route("/", get(get_edge_stats))
}

/// GET /api/edge/stats
///
/// Returns proxy metrics (per-domain request counts, 5xx errors)
/// and certificate expiry information from hr-edge.
async fn get_edge_stats(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let resp = state
        .edge
        .request(&EdgeRequest::GetStats)
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Edge IPC error: {}", e),
            )
        })?;

    if !resp.ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            resp.error.unwrap_or_else(|| "Unknown error".to_string()),
        ));
    }

    Ok(Json(resp.data.unwrap_or(serde_json::json!({}))))
}
