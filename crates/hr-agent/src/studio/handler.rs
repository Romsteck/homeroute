use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response};

use crate::proxy::{BoxBody, full_body};
use super::StudioBridge;

/// Handle non-WebSocket Studio requests (static files + API).
/// WebSocket requests are handled by the separate WS server on STUDIO_WS_PORT
/// and proxied there by the agent proxy's normal bidirectional copy mechanism.
pub async fn handle_studio_request(
    req: Request<Incoming>,
    _studio: &Arc<StudioBridge>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let path = req.uri().path().to_string();

    // API routes
    if path.starts_with("/api/") {
        return Ok(match path.as_str() {
            "/api/sessions" => {
                let sessions = super::sessions::list_sessions();
                let body = serde_json::to_string(&serde_json::json!({ "sessions": sessions }))
                    .unwrap_or_else(|_| "{}".to_string());
                Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(full_body(body))
                    .unwrap()
            }
            "/api/status" => {
                let body = serde_json::json!({ "status": "ok" }).to_string();
                Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(full_body(body))
                    .unwrap()
            }
            _ => Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(full_body(r#"{"error":"not found"}"#))
                .unwrap(),
        });
    }

    // Static files (SPA)
    Ok(super::static_files::serve_asset(&path))
}
