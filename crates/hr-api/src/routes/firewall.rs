use axum::extract::State;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(firewall_status))
        .route("/rules", get(list_rules).post(add_rule))
        .route("/rules/{id}", delete(remove_rule).patch(toggle_rule))
        .route("/current-ruleset", get(current_ruleset))
}

async fn firewall_status(State(state): State<ApiState>) -> Json<Value> {
    match &state.firewall {
        Some(engine) => {
            let config = engine.get_config().await;
            let prefix = engine.get_lan_prefix().await;
            Json(json!({
                "success": true,
                "enabled": config.enabled,
                "lan_interface": config.lan_interface,
                "wan_interface": config.wan_interface,
                "default_inbound_policy": config.default_inbound_policy,
                "lan_prefix": prefix,
                "rules_count": config.allow_rules.len(),
            }))
        }
        None => Json(json!({
            "success": true,
            "enabled": false,
        })),
    }
}

async fn list_rules(State(state): State<ApiState>) -> Json<Value> {
    match &state.firewall {
        Some(engine) => {
            let rules = engine.get_rules().await;
            Json(json!({ "success": true, "rules": rules }))
        }
        None => Json(json!({ "success": false, "error": "Firewall not enabled" })),
    }
}

async fn add_rule(
    State(state): State<ApiState>,
    Json(rule): Json<hr_firewall::FirewallRule>,
) -> Json<Value> {
    match &state.firewall {
        Some(engine) => {
            match engine.add_rule(rule).await {
                Ok(()) => Json(json!({ "success": true })),
                Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
            }
        }
        None => Json(json!({ "success": false, "error": "Firewall not enabled" })),
    }
}

async fn remove_rule(
    State(state): State<ApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    match &state.firewall {
        Some(engine) => {
            match engine.remove_rule(&id).await {
                Ok(true) => Json(json!({ "success": true })),
                Ok(false) => Json(json!({ "success": false, "error": "Rule not found" })),
                Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
            }
        }
        None => Json(json!({ "success": false, "error": "Firewall not enabled" })),
    }
}

async fn toggle_rule(
    State(state): State<ApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    match &state.firewall {
        Some(engine) => {
            match engine.toggle_rule(&id).await {
                Ok(Some(new_state)) => Json(json!({ "success": true, "enabled": new_state })),
                Ok(None) => Json(json!({ "success": false, "error": "Rule not found" })),
                Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
            }
        }
        None => Json(json!({ "success": false, "error": "Firewall not enabled" })),
    }
}

async fn current_ruleset(State(state): State<ApiState>) -> Json<Value> {
    if state.firewall.is_none() {
        return Json(json!({ "success": false, "error": "Firewall not enabled" }));
    }

    match hr_firewall::nftables::get_current_rules().await {
        Ok(rules) => Json(json!({ "success": true, "ruleset": rules })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}
