use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use hr_acme::WildcardType;
use serde_json::{json, Value};
use tracing::{error, info};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(status))
        .route("/certificates", get(list_certificates))
        .route("/renew", post(renew_certificates))
        .route("/push", post(push_certificates))
        .route("/certificate/wildcard", get(get_wildcard_cert))
        .route("/certificate/code", get(get_code_cert))
        .route("/certificate/app/{slug}", post(request_app_cert))
}

/// Helper: convert a WildcardType to a display string for JSON.
fn wildcard_type_label(wt: &WildcardType) -> &'static str {
    match wt {
        WildcardType::Global => "global",
        WildcardType::App { .. } => "app",
        WildcardType::LegacyCode => "legacy_code",
    }
}

/// Get ACME status and certificate overview
async fn status(State(state): State<ApiState>) -> Json<Value> {
    let certs = state.acme.list_certificates().unwrap_or_default();
    let global_cert = certs.iter().find(|c| c.wildcard_type == WildcardType::Global);
    let legacy_code_cert = certs.iter().find(|c| c.wildcard_type == WildcardType::LegacyCode);

    // Collect per-app certs
    let app_certs: Vec<Value> = certs.iter()
        .filter(|c| matches!(c.wildcard_type, WildcardType::App { .. }))
        .map(|c| json!({
            "id": c.id,
            "type": "app",
            "type_display": c.wildcard_type.display_name(),
            "domain": c.domains.first().unwrap_or(&String::new()),
            "issued_at": c.issued_at.to_rfc3339(),
            "expires_at": c.expires_at.to_rfc3339(),
            "days_until_expiry": c.days_until_expiry(),
            "needs_renewal": c.needs_renewal(state.acme.renewal_threshold_days())
        }))
        .collect();

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
            })),
            "legacy_code": legacy_code_cert.map(|c| json!({
                "id": c.id,
                "domain": c.domains.first().unwrap_or(&String::new()),
                "issued_at": c.issued_at.to_rfc3339(),
                "expires_at": c.expires_at.to_rfc3339(),
                "days_until_expiry": c.days_until_expiry(),
                "needs_renewal": c.needs_renewal(state.acme.renewal_threshold_days())
            })),
            "apps": app_certs
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
                        "type": wildcard_type_label(&c.wildcard_type),
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

/// Force renewal of all certificates that need it (global, legacy code, and per-app).
async fn renew_certificates(State(state): State<ApiState>) -> Json<Value> {
    let mut renewed = Vec::new();
    let mut errors = Vec::new();

    // Get all certificates and renew those that need it
    let certs = state.acme.list_certificates().unwrap_or_default();

    // Determine which wildcard types need renewal
    let mut types_to_renew: Vec<WildcardType> = Vec::new();

    // Check global wildcard
    let global_needs = certs.iter()
        .find(|c| c.wildcard_type == WildcardType::Global)
        .map(|c| c.needs_renewal(state.acme.renewal_threshold_days()))
        .unwrap_or(true);
    if global_needs {
        types_to_renew.push(WildcardType::Global);
    }

    // Check legacy code wildcard
    let code_needs = certs.iter()
        .find(|c| c.wildcard_type == WildcardType::LegacyCode)
        .map(|c| c.needs_renewal(state.acme.renewal_threshold_days()))
        .unwrap_or(true);
    if code_needs {
        types_to_renew.push(WildcardType::LegacyCode);
    }

    // Check per-app wildcards
    for cert in &certs {
        if let WildcardType::App { .. } = &cert.wildcard_type {
            if cert.needs_renewal(state.acme.renewal_threshold_days()) {
                types_to_renew.push(cert.wildcard_type.clone());
            }
        }
    }

    for wildcard_type in types_to_renew {
        let label = wildcard_type.display_name();
        info!(wildcard_type = %label, "Renewing certificate");
        match state.acme.request_wildcard(wildcard_type.clone()).await {
            Ok(cert) => {
                renewed.push(json!({
                    "type": wildcard_type_label(&wildcard_type),
                    "type_display": label,
                    "domain": cert.domains.first().unwrap_or(&String::new()),
                    "expires_at": cert.expires_at.to_rfc3339()
                }));
            }
            Err(e) => {
                error!(wildcard_type = %label, error = %e, "Failed to renew certificate");
                errors.push(json!({
                    "type": wildcard_type_label(&wildcard_type),
                    "type_display": label,
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

/// Get code-server wildcard certificate (legacy) for agents
async fn get_code_cert(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.get_cert_pem(WildcardType::LegacyCode).await {
        Ok((cert_pem, key_pem)) => Json(json!({
            "success": true,
            "cert_pem": cert_pem,
            "key_pem": key_pem
        })),
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}

/// Request a per-app wildcard certificate manually.
/// POST /acme/certificate/app/{slug}
async fn request_app_cert(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> Json<Value> {
    info!(slug = %slug, "Requesting per-app wildcard certificate");
    match state.acme.request_app_wildcard(&slug).await {
        Ok(cert) => {
            info!(slug = %slug, "Per-app wildcard certificate issued");
            Json(json!({
                "success": true,
                "certificate": {
                    "id": cert.id,
                    "type": "app",
                    "type_display": cert.wildcard_type.display_name(),
                    "domains": cert.domains,
                    "issued_at": cert.issued_at.to_rfc3339(),
                    "expires_at": cert.expires_at.to_rfc3339(),
                    "days_until_expiry": cert.days_until_expiry(),
                }
            }))
        }
        Err(e) => {
            error!(slug = %slug, error = %e, "Failed to issue per-app wildcard certificate");
            Json(json!({"success": false, "error": e.to_string()}))
        }
    }
}
