use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(status))
        .route("/reload", post(reload))
        .route("/config", get(get_config).put(update_config))
        .route("/leases", get(get_leases))
}

async fn status(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.service_status().await {
        Ok(services) => {
            let active = services
                .iter()
                .any(|s| s.name.starts_with("dns-") && s.state == "running");
            Json(json!({
                "success": true,
                "active": active,
                "service": "hr-netcore"
            }))
        }
        Err(_) => Json(json!({
            "success": false,
            "active": false,
            "error": "Network core unavailable"
        })),
    }
}

async fn reload(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.reload_config().await {
        Ok(resp) if resp.ok => Json(json!({"success": true})),
        Ok(resp) => Json(json!({
            "success": false,
            "error": resp.error.unwrap_or_else(|| "Unknown error".into())
        })),
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}

async fn get_config(State(state): State<ApiState>) -> Json<Value> {
    let config_path = &state.dns_dhcp_config_path;
    match tokio::fs::read_to_string(config_path).await {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(mut config) => {
                // Merge runtime DNS static records from hr-netcore
                if let Ok(static_data) = state.netcore.dns_static_records().await {
                    if let Some(dns_obj) = config.get_mut("dns").and_then(|d| d.as_object_mut()) {
                        dns_obj.insert(
                            "static_records".to_string(),
                            serde_json::to_value(&static_data.records).unwrap_or_default(),
                        );
                    }
                }
                Json(json!({"success": true, "config": config}))
            }
            Err(e) => Json(json!({"success": false, "error": format!("Invalid config: {}", e)})),
        },
        Err(e) => Json(json!({"success": false, "error": format!("Failed to read config: {}", e)})),
    }
}

async fn update_config(State(state): State<ApiState>, Json(body): Json<Value>) -> Json<Value> {
    let config_path = &state.dns_dhcp_config_path;

    // Write the new config
    let content = match serde_json::to_string_pretty(&body) {
        Ok(c) => c,
        Err(e) => {
            return Json(json!({"success": false, "error": format!("Serialization error: {}", e)}));
        }
    };

    let tmp_path = config_path.with_extension("json.tmp");
    if let Err(e) = tokio::fs::write(&tmp_path, &content).await {
        return Json(json!({"success": false, "error": format!("Write failed: {}", e)}));
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, config_path).await {
        return Json(json!({"success": false, "error": format!("Rename failed: {}", e)}));
    }

    // Tell hr-netcore to reload config from disk
    reload(State(state)).await
}

async fn get_leases(State(state): State<ApiState>) -> Json<Value> {
    match state.netcore.dhcp_leases().await {
        Ok(leases) => {
            let result: Vec<serde_json::Value> = leases
                .iter()
                .map(|l| {
                    json!({
                        "expiry": l.expiry,
                        "mac": l.mac,
                        "ip": l.ip,
                        "hostname": l.hostname,
                        "client_id": l.client_id,
                    })
                })
                .collect();
            Json(json!({"success": true, "leases": result}))
        }
        Err(_) => Json(json!({"success": false, "error": "Network core unavailable"})),
    }
}
