use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use hr_acme::WildcardType;
use serde_json::{Value, json};
use tracing::{error, info};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(status))
        .route("/certificates", get(list_certificates))
        .route("/renew", post(renew_certificates))
        .route("/push", post(push_certificates))
        .route("/certificate/wildcard", get(get_wildcard_cert))
}

/// Get ACME status and certificate overview
async fn status(State(state): State<ApiState>) -> Json<Value> {
    let certs = state.acme.list_certificates().unwrap_or_default();
    let global_cert = certs
        .iter()
        .find(|c| c.wildcard_type == WildcardType::Global);

    Json(json!({
        "success": true,
        "initialized": state.acme.is_initialized(),
        "provider": "Let's Encrypt",
        "base_domain": state.acme.base_domain(),
        "certificates": {
            "global": global_cert.map(|c| json!({
                "id": c.id,
                "domain": c.domains.first().unwrap_or(&String::new()),
                "issued_at": c.issued_at.to_rfc3339(),
                "expires_at": c.expires_at.to_rfc3339(),
                "days_until_expiry": c.days_until_expiry(),
                "needs_renewal": c.needs_renewal(state.acme.renewal_threshold_days())
            }))
        }
    }))
}

/// List all certificates with details
async fn list_certificates(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.list_certificates() {
        Ok(certs) => {
            let threshold = state.acme.renewal_threshold_days();
            let certs_json: Vec<Value> = certs
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "global",
                        "type_display": c.wildcard_type.display_name(),
                        "domains": c.domains,
                        "issued_at": c.issued_at.to_rfc3339(),
                        "expires_at": c.expires_at.to_rfc3339(),
                        "days_until_expiry": c.days_until_expiry(),
                        "needs_renewal": c.needs_renewal(threshold),
                        "expired": c.is_expired()
                    })
                })
                .collect();
            Json(json!({"success": true, "certificates": certs_json}))
        }
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}

/// Force renewal of all certificates that need it.
async fn renew_certificates(State(state): State<ApiState>) -> Json<Value> {
    let mut renewed = Vec::new();
    let mut errors = Vec::new();

    let certs = state.acme.list_certificates().unwrap_or_default();

    // Check if global wildcard needs renewal
    let global_needs = certs
        .iter()
        .find(|c| c.wildcard_type == WildcardType::Global)
        .map(|c| c.needs_renewal(state.acme.renewal_threshold_days()))
        .unwrap_or(true);

    if global_needs {
        info!(wildcard_type = "global", "Renewing certificate");
        match state.acme.request_wildcard(WildcardType::Global).await {
            Ok(cert) => {
                renewed.push(json!({
                    "type": "global",
                    "type_display": "Global (Dashboard)",
                    "domain": cert.domains.first().unwrap_or(&String::new()),
                    "expires_at": cert.expires_at.to_rfc3339()
                }));
            }
            Err(e) => {
                error!(wildcard_type = "global", error = %e, "Failed to renew certificate");
                errors.push(json!({
                    "type": "global",
                    "type_display": "Global (Dashboard)",
                    "error": e.to_string()
                }));
            }
        }
    }

    Json(json!({
        "success": errors.is_empty(),
        "renewed": renewed,
        "errors": errors
    }))
}

/// Force push certificates (no-op: agents no longer handle TLS)
async fn push_certificates(State(_state): State<ApiState>) -> Json<Value> {
    Json(json!({
        "success": true,
        "message": "TLS is now handled centrally by hr-proxy. No agent push needed."
    }))
}

/// Get wildcard certificate (global) for agents
async fn get_wildcard_cert(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.get_cert_pem(WildcardType::Global).await {
        Ok((cert_pem, key_pem)) => Json(json!({
            "success": true,
            "cert_pem": cert_pem,
            "key_pem": key_pem
        })),
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}
