use axum::{extract::State, routing::get, Json, Router};
use hr_common::service_registry::{ServicePriorityLevel, ServiceState, ServiceStatus};
use serde_json::{json, Value};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new().route("/status", get(status))
}

async fn status(State(state): State<ApiState>) -> Json<Value> {
    let registry = state.service_registry.read().await;

    let mut services: Vec<ServiceStatus> = registry.values().cloned().collect();

    // Merge services from hr-netcore (dns, dhcp, ipv6, adblock)
    if let Ok(netcore_services) = state.netcore.service_status().await {
        for entry in netcore_services {
            let svc_state = match entry.state.as_str() {
                "running" => ServiceState::Running,
                "disabled" => ServiceState::Disabled,
                "failed" => ServiceState::Failed,
                "starting" => ServiceState::Starting,
                "stopped" => ServiceState::Stopped,
                _ => ServiceState::Starting,
            };
            let priority = match entry.priority.as_str() {
                "critical" => ServicePriorityLevel::Critical,
                "important" => ServicePriorityLevel::Important,
                _ => ServicePriorityLevel::Background,
            };
            services.push(ServiceStatus {
                name: entry.name,
                state: svc_state,
                priority,
                restart_count: entry.restart_count,
                last_state_change: entry.last_state_change,
                error: entry.error,
            });
        }
    }

    services.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then(a.name.cmp(&b.name))
    });

    Json(json!({
        "success": true,
        "services": services
    }))
}
