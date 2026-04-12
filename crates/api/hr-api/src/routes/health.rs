use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}
