//! REST routes managing hr-edge dynamic state (app routes).
//!
//! Currently exposes `POST /api/edge/routes` so external tooling
//! (`scripts/setup-studio.sh`) can register a domain → upstream mapping
//! without crafting raw IPC messages.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info, warn};

use hr_ipc::edge::EdgeRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", post(set_route))
        .route("/{domain}", delete(remove_route))
}

#[derive(Debug, Deserialize)]
struct SetRouteRequest {
    /// Public domain hr-edge will route from (e.g. "studio.mynetwk.biz").
    domain: String,
    /// Upstream "ip:port" (e.g. "10.0.0.10:8443").
    target: String,
    #[serde(default)]
    auth_required: bool,
    #[serde(default)]
    allowed_groups: Vec<String>,
    /// If true, the route only matches requests reaching hr-edge from the LAN.
    #[serde(default)]
    local_only: bool,
    /// Optional logical app id; defaults to the subdomain label of `domain`.
    #[serde(default)]
    app_id: Option<String>,
    /// Optional host id; defaults to "manual".
    #[serde(default)]
    host_id: Option<String>,
}

/// Parse "host:port" or "ip:port" into its parts. Only IPv4 + bare numeric
/// port supported — hr-proxy itself only stores Ipv4Addr today.
fn parse_target(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port))
}

#[tracing::instrument(skip(state))]
async fn set_route(
    State(state): State<ApiState>,
    Json(body): Json<SetRouteRequest>,
) -> impl IntoResponse {
    if body.domain.trim().is_empty() {
        warn!("set_route: empty domain");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "domain must not be empty"})),
        )
            .into_response();
    }
    let (target_ip, target_port) = match parse_target(&body.target) {
        Some(t) => t,
        None => {
            warn!(target = %body.target, "set_route: malformed target");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": "target must be 'ip:port' (e.g. 10.0.0.10:8443)"
                })),
            )
                .into_response();
        }
    };

    let app_id = body.app_id.clone().unwrap_or_else(|| {
        body.domain
            .split_once('.')
            .map(|(label, _)| label.to_string())
            .unwrap_or_else(|| body.domain.clone())
    });
    let host_id = body.host_id.clone().unwrap_or_else(|| "manual".to_string());

    info!(
        domain = %body.domain,
        target_ip = %target_ip,
        target_port = target_port,
        auth_required = body.auth_required,
        local_only = body.local_only,
        "edge route registered: {} -> {}",
        body.domain,
        body.target
    );

    let req = EdgeRequest::SetAppRoute {
        domain: body.domain.clone(),
        app_id,
        host_id,
        target_ip,
        target_port,
        auth_required: body.auth_required,
        allowed_groups: body.allowed_groups.clone(),
        local_only: body.local_only,
    };

    match state.edge.request(&req).await {
        Ok(resp) if resp.ok => Json(json!({
            "success": true,
            "domain": body.domain,
            "target": body.target,
        }))
        .into_response(),
        Ok(resp) => {
            let err = resp.error.unwrap_or_else(|| "unknown error".into());
            error!(domain = %body.domain, error = %err, "edge IPC SetAppRoute failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": err})),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "edge IPC unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"success": false, "error": format!("Edge unavailable: {e}")})),
            )
                .into_response()
        }
    }
}

#[tracing::instrument(skip(state))]
async fn remove_route(
    State(state): State<ApiState>,
    Path(domain): Path<String>,
) -> impl IntoResponse {
    if domain.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "domain must not be empty"})),
        )
            .into_response();
    }
    info!(domain = %domain, "edge route removed");
    let req = EdgeRequest::RemoveAppRoute {
        domain: domain.clone(),
    };
    match state.edge.request(&req).await {
        Ok(resp) if resp.ok => {
            Json(json!({"success": true, "domain": domain})).into_response()
        }
        Ok(resp) => {
            let err = resp.error.unwrap_or_else(|| "unknown error".into());
            error!(domain = %domain, error = %err, "edge IPC RemoveAppRoute failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": err})),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "edge IPC unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"success": false, "error": format!("Edge unavailable: {e}")})),
            )
                .into_response()
        }
    }
}
