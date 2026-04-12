use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use hr_ipc::edge::EdgeRequest;
use serde_json::{Value, json};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(status))
        .route("/routes", get(routes))
        .route("/reload", post(reload))
}

async fn status(State(state): State<ApiState>) -> Result<Json<Value>, (StatusCode, String)> {
    let resp = state
        .edge
        .request(&EdgeRequest::GetProxyConfig)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !resp.ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            resp.error.unwrap_or_default(),
        ));
    }
    let config: Value = resp.data.unwrap_or_default();

    Ok(Json(json!({
        "success": true,
        "running": true,
        "httpsPort": config.get("https_port").unwrap_or(&json!(443)),
        "httpPort": config.get("http_port").unwrap_or(&json!(80)),
        "baseDomain": config.get("base_domain").unwrap_or(&json!("")),
        "tlsMode": config.get("tls_mode").unwrap_or(&json!("acme")),
        "routeCount": config.get("routes").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0),
        "activeRoutes": config.get("routes").and_then(|r| r.as_array())
            .map(|routes| routes.iter().filter(|r| r.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true)).count())
            .unwrap_or(0)
    })))
}

async fn routes(State(state): State<ApiState>) -> Result<Json<Value>, (StatusCode, String)> {
    let resp = state
        .edge
        .request(&EdgeRequest::GetProxyConfig)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !resp.ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            resp.error.unwrap_or_default(),
        ));
    }
    let config: Value = resp.data.unwrap_or_default();
    let routes = config.get("routes").cloned().unwrap_or(json!([]));

    Ok(Json(json!({"success": true, "routes": routes})))
}

async fn reload(State(state): State<ApiState>) -> Result<Json<Value>, (StatusCode, String)> {
    let resp = state
        .edge
        .request(&EdgeRequest::ReloadConfig)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !resp.ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            resp.error.unwrap_or_default(),
        ));
    }
    Ok(Json(json!({"success": true})))
}
