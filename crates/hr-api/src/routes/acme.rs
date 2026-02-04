use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use hr_acme::WildcardType;
use serde::Deserialize;
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
}

/// Get ACME status and certificate overview
async fn status(State(state): State<ApiState>) -> Json<Value> {
    let certs = state.acme.list_certificates().unwrap_or_default();
    let main_cert = certs.iter().find(|c| c.wildcard_type == WildcardType::Main);
    let code_cert = certs.iter().find(|c| c.wildcard_type == WildcardType::Code);

    Json(json!({
        "success": true,
        "initialized": state.acme.is_initialized(),
        "provider": "Let's Encrypt",
        "base_domain": state.acme.base_domain(),
        "certificates": {
            "main": main_cert.map(|c| json!({
                "id": c.id,
                "domain": c.domains.first().unwrap_or(&String::new()),
                "issued_at": c.issued_at.to_rfc3339(),
                "expires_at": c.expires_at.to_rfc3339(),
                "days_until_expiry": c.days_until_expiry(),
                "needs_renewal": c.needs_renewal(state.acme.renewal_threshold_days())
            })),
            "code": code_cert.map(|c| json!({
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
                        "type": match c.wildcard_type {
                            WildcardType::Main => "main",
                            WildcardType::Code => "code",
                        },
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

/// Force renewal of certificates
async fn renew_certificates(State(state): State<ApiState>) -> Json<Value> {
    let mut renewed = Vec::new();
    let mut errors = Vec::new();

    for wildcard_type in [WildcardType::Main, WildcardType::Code] {
        let needs_renewal = state
            .acme
            .get_certificate(wildcard_type)
            .map(|c| c.needs_renewal(state.acme.renewal_threshold_days()))
            .unwrap_or(true); // If cert doesn't exist, we need to create it

        if needs_renewal {
            info!(wildcard_type = ?wildcard_type, "Renewing certificate");
            match state.acme.request_wildcard(wildcard_type).await {
                Ok(cert) => {
                    renewed.push(json!({
                        "type": match wildcard_type {
                            WildcardType::Main => "main",
                            WildcardType::Code => "code",
                        },
                        "domain": cert.domains.first().unwrap_or(&String::new()),
                        "expires_at": cert.expires_at.to_rfc3339()
                    }));
                }
                Err(e) => {
                    error!(wildcard_type = ?wildcard_type, error = %e, "Failed to renew certificate");
                    errors.push(json!({
                        "type": match wildcard_type {
                            WildcardType::Main => "main",
                            WildcardType::Code => "code",
                        },
                        "error": e.to_string()
                    }));
                }
            }
        }
    }

    // Push updated certs to all connected agents if any were renewed
    if !renewed.is_empty() {
        if let Some(registry) = &state.registry {
            info!("Pushing certificate updates to agents");
            registry.push_cert_updates().await;
        }
    }

    Json(json!({
        "success": errors.is_empty(),
        "renewed": renewed,
        "errors": errors
    }))
}

/// Force push certificates to all connected agents
async fn push_certificates(State(state): State<ApiState>) -> Json<Value> {
    if let Some(registry) = &state.registry {
        info!("Force pushing certificate updates to all agents");
        registry.push_cert_updates().await;
        Json(json!({
            "success": true,
            "message": "Certificates pushed to all connected agents"
        }))
    } else {
        Json(json!({
            "success": false,
            "error": "Registry not available"
        }))
    }
}

/// Get wildcard certificate (main) for agents
async fn get_wildcard_cert(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.get_cert_pem(WildcardType::Main).await {
        Ok((cert_pem, key_pem)) => Json(json!({
            "success": true,
            "cert_pem": cert_pem,
            "key_pem": key_pem
        })),
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}

/// Get code-server wildcard certificate for agents
async fn get_code_cert(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.get_cert_pem(WildcardType::Code).await {
        Ok((cert_pem, key_pem)) => Json(json!({
            "success": true,
            "cert_pem": cert_pem,
            "key_pem": key_pem
        })),
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}
