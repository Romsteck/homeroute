use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use hr_ipc::edge::EdgeRequest;
use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/config", get(get_config))
        .route("/config/domain", put(update_domain))
        .route("/hosts", get(list_hosts).post(add_host))
        .route("/hosts/{id}", put(update_host).delete(delete_host))
        .route("/hosts/{id}/toggle", post(toggle_host))
        .route("/status", get(proxy_status))
        .route("/reload", post(reload_proxy))
        .route("/certificates/status", get(certificates_status))
        .route("/certificates/renew", post(renew_certificates))
        .route("/system-routes", get(system_routes))
}

/// Load the reverseproxy-config.json
async fn load_rp_config(state: &ApiState) -> Result<Value, String> {
    let content = tokio::fs::read_to_string(&state.reverseproxy_config_path)
        .await
        .map_err(|e| format!("Read error: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Parse error: {}", e))
}

/// Save the reverseproxy-config.json
async fn save_rp_config(state: &ApiState, config: &Value) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(config).map_err(|e| format!("Serialize error: {}", e))?;
    let tmp = state.reverseproxy_config_path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &content)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    tokio::fs::rename(&tmp, &state.reverseproxy_config_path)
        .await
        .map_err(|e| format!("Rename error: {}", e))?;
    Ok(())
}

/// Sync all routes to rust-proxy-config.json and reload proxy via edge IPC
async fn sync_and_reload(state: &ApiState) -> Result<(), String> {
    let rp_config = load_rp_config(state).await?;
    let base_domain = rp_config
        .get("baseDomain")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();

    let hosts = rp_config
        .get("hosts")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();

    // Build proxy routes from standalone hosts
    // With ACME wildcards, we don't need per-domain cert_ids
    let mut routes = Vec::new();
    for host in &hosts {
        if host.get("enabled").and_then(|e| e.as_bool()) != Some(true) {
            continue;
        }
        let domain = if let Some(custom) = host.get("customDomain").and_then(|d| d.as_str()) {
            if !custom.is_empty() {
                custom.to_string()
            } else if let Some(sub) = host.get("subdomain").and_then(|s| s.as_str()) {
                format!("{}.{}", sub, base_domain)
            } else {
                continue;
            }
        } else if let Some(sub) = host.get("subdomain").and_then(|s| s.as_str()) {
            format!("{}.{}", sub, base_domain)
        } else {
            continue;
        };

        routes.push(json!({
            "id": host.get("id").unwrap_or(&json!("")),
            "domain": domain,
            "backend": "rust",
            "target_host": host.get("targetHost").unwrap_or(&json!("localhost")),
            "target_port": host.get("targetPort").unwrap_or(&json!(80)),
            "local_only": host.get("localOnly").unwrap_or(&json!(false)),
            "require_auth": host.get("requireAuth").unwrap_or(&json!(false)),
            "enabled": true
        }));
    }

    // Load current proxy config, update routes, save
    let proxy_config_path = &state.proxy_config_path;
    let proxy_content = tokio::fs::read_to_string(proxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let mut proxy_config: Value =
        serde_json::from_str(&proxy_content).unwrap_or_else(|_| json!({}));

    proxy_config["routes"] = json!(routes);
    proxy_config["base_domain"] = json!(base_domain);

    let content =
        serde_json::to_string_pretty(&proxy_config).map_err(|e| format!("Serialize: {}", e))?;
    let tmp = proxy_config_path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &content)
        .await
        .map_err(|e| format!("Write: {}", e))?;
    tokio::fs::rename(&tmp, proxy_config_path)
        .await
        .map_err(|e| format!("Rename: {}", e))?;

    // Tell hr-edge to reload config (it will reload TLS certs + proxy config internally)
    let resp = state.edge.request(&EdgeRequest::ReloadConfig).await
        .map_err(|e| format!("Edge IPC error: {}", e))?;
    if !resp.ok {
        return Err(format!("Edge reload failed: {}", resp.error.unwrap_or_default()));
    }

    Ok(())
}

async fn get_config(State(state): State<ApiState>) -> Json<Value> {
    match load_rp_config(&state).await {
        Ok(config) => Json(json!({"success": true, "config": config})),
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

#[derive(Deserialize)]
struct UpdateDomainRequest {
    domain: String,
}

async fn update_domain(
    State(state): State<ApiState>,
    Json(body): Json<UpdateDomainRequest>,
) -> Json<Value> {
    let mut config = match load_rp_config(&state).await {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": e})),
    };

    config["baseDomain"] = json!(body.domain);

    if let Err(e) = save_rp_config(&state, &config).await {
        return Json(json!({"success": false, "error": e}));
    }

    if let Err(e) = sync_and_reload(&state).await {
        return Json(json!({"success": false, "error": format!("Sync failed: {}", e)}));
    }

    Json(json!({"success": true}))
}


async fn list_hosts(State(state): State<ApiState>) -> Json<Value> {
    match load_rp_config(&state).await {
        Ok(config) => {
            let hosts = config.get("hosts").cloned().unwrap_or(json!([]));
            Json(json!({"success": true, "hosts": hosts}))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

async fn add_host(State(state): State<ApiState>, Json(body): Json<Value>) -> Json<Value> {
    let mut config = match load_rp_config(&state).await {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": e})),
    };

    let id = uuid::Uuid::new_v4().to_string();
    let mut host = body;
    host["id"] = json!(id);
    host["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
    if host.get("enabled").is_none() {
        host["enabled"] = json!(true);
    }

    let hosts = config
        .get_mut("hosts")
        .and_then(|h| h.as_array_mut());
    match hosts {
        Some(arr) => arr.push(host.clone()),
        None => config["hosts"] = json!([host]),
    }

    if let Err(e) = save_rp_config(&state, &config).await {
        return Json(json!({"success": false, "error": e}));
    }
    if let Err(e) = sync_and_reload(&state).await {
        return Json(json!({"success": false, "error": format!("Sync failed: {}", e)}));
    }

    Json(json!({"success": true, "host": host}))
}

async fn update_host(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(updates): Json<Value>,
) -> Json<Value> {
    let mut config = match load_rp_config(&state).await {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": e})),
    };

    let hosts = config.get_mut("hosts").and_then(|h| h.as_array_mut());
    if let Some(hosts) = hosts {
        if let Some(host) = hosts.iter_mut().find(|h| h.get("id").and_then(|i| i.as_str()) == Some(&id)) {
            if let Some(obj) = updates.as_object() {
                for (k, v) in obj {
                    host[k] = v.clone();
                }
            }
            host["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        } else {
            return Json(json!({"success": false, "error": "Host non trouve"}));
        }
    }

    if let Err(e) = save_rp_config(&state, &config).await {
        return Json(json!({"success": false, "error": e}));
    }
    if let Err(e) = sync_and_reload(&state).await {
        return Json(json!({"success": false, "error": format!("Sync failed: {}", e)}));
    }

    Json(json!({"success": true}))
}

async fn delete_host(State(state): State<ApiState>, Path(id): Path<String>) -> Json<Value> {
    let mut config = match load_rp_config(&state).await {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": e})),
    };

    if let Some(hosts) = config.get_mut("hosts").and_then(|h| h.as_array_mut()) {
        hosts.retain(|h| h.get("id").and_then(|i| i.as_str()) != Some(&id));
    }

    if let Err(e) = save_rp_config(&state, &config).await {
        return Json(json!({"success": false, "error": e}));
    }
    if let Err(e) = sync_and_reload(&state).await {
        return Json(json!({"success": false, "error": format!("Sync failed: {}", e)}));
    }

    Json(json!({"success": true}))
}

async fn toggle_host(State(state): State<ApiState>, Path(id): Path<String>) -> Json<Value> {
    let mut config = match load_rp_config(&state).await {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": e})),
    };

    if let Some(hosts) = config.get_mut("hosts").and_then(|h| h.as_array_mut()) {
        if let Some(host) = hosts.iter_mut().find(|h| h.get("id").and_then(|i| i.as_str()) == Some(&id)) {
            let current = host.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            host["enabled"] = json!(!current);
        }
    }

    if let Err(e) = save_rp_config(&state, &config).await {
        return Json(json!({"success": false, "error": e}));
    }
    if let Err(e) = sync_and_reload(&state).await {
        return Json(json!({"success": false, "error": format!("Sync failed: {}", e)}));
    }

    Json(json!({"success": true}))
}

async fn proxy_status(State(state): State<ApiState>) -> Json<Value> {
    let resp = match state.edge.request(&EdgeRequest::GetProxyConfig).await {
        Ok(r) => r,
        Err(e) => return Json(json!({"success": false, "error": e.to_string()})),
    };
    if !resp.ok {
        return Json(json!({"success": false, "error": resp.error.unwrap_or_default()}));
    }
    let config = resp.data.unwrap_or_default();

    let routes = config.get("routes").and_then(|r| r.as_array());
    let route_count = routes.map(|r| r.len()).unwrap_or(0);
    let active_count = routes
        .map(|r| r.iter().filter(|r| r.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true)).count())
        .unwrap_or(0);

    Json(json!({
        "success": true,
        "active": true,
        "routes": route_count,
        "active_routes": active_count,
        "base_domain": config.get("base_domain").unwrap_or(&json!(""))
    }))
}

async fn reload_proxy(State(state): State<ApiState>) -> Json<Value> {
    match sync_and_reload(&state).await {
        Ok(()) => Json(json!({"success": true})),
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

async fn certificates_status(State(state): State<ApiState>) -> Json<Value> {
    match state.acme.list_certificates() {
        Ok(certs) => {
            let statuses: Vec<Value> = certs
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "domains": c.domains,
                        "wildcard_type": format!("{:?}", c.wildcard_type),
                        "issued_at": c.issued_at.to_rfc3339(),
                        "expires_at": c.expires_at.to_rfc3339(),
                        "expired": c.is_expired(),
                        "needs_renewal": c.needs_renewal(30)
                    })
                })
                .collect();
            Json(json!({"success": true, "certificates": statuses}))
        }
        Err(e) => Json(json!({"success": false, "error": format!("{e}")})),
    }
}

async fn renew_certificates(State(state): State<ApiState>) -> Json<Value> {
    let candidates = match state.acme.certificates_needing_renewal() {
        Ok(c) => c,
        Err(e) => return Json(json!({"success": false, "error": format!("{e}")})),
    };

    let mut renewed = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for cert in candidates {
        match state.acme.request_wildcard(cert.wildcard_type).await {
            Ok(new_cert) => renewed.push(json!({"id": new_cert.id, "domains": new_cert.domains})),
            Err(e) => errors.push(json!({"id": cert.id, "error": format!("{e}")})),
        }
    }

    Json(json!({
        "success": errors.is_empty(),
        "renewed": renewed,
        "errors": errors
    }))
}

/// GET /api/reverseproxy/system-routes
/// Returns auto-managed routes derived from running environments and their apps.
async fn system_routes(State(state): State<ApiState>) -> impl IntoResponse {
    let base_domain = &state.env.base_domain;

    let resp = match state.orchestrator.request(&OrchestratorRequest::ListEnvironments).await {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({"success": false, "error": format!("Orchestrator unavailable: {e}")}));
        }
    };

    if !resp.ok {
        return Json(json!({"success": false, "error": resp.error.unwrap_or_default()}));
    }

    let envs: Vec<Value> = resp
        .data
        .and_then(|d| serde_json::from_value(d).ok())
        .unwrap_or_default();

    let mut routes = Vec::new();

    for env in &envs {
        let slug = env.get("slug").and_then(|s| s.as_str()).unwrap_or_default();
        let status = env.get("status").and_then(|s| s.as_str()).unwrap_or("stopped");
        let connected = env.get("agent_connected").and_then(|a| a.as_bool()).unwrap_or(false);
        let ip = match env.get("ipv4_address").and_then(|a| a.as_str()) {
            Some(ip) => ip,
            None => continue,
        };

        let apps_count = env.get("apps").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
        let apps_running = env.get("apps").and_then(|a| a.as_array())
            .map(|apps| apps.iter().filter(|a| a.get("running").and_then(|r| r.as_bool()).unwrap_or(false)).count())
            .unwrap_or(0);
        let env_status = if connected && status == "running" { "running" } else { "stopped" };

        // One wildcard route per environment
        routes.push(json!({
            "domain": format!("*.{slug}.{base_domain}"),
            "target": format!("{ip}:80"),
            "environment": slug,
            "status": env_status,
            "type": "wildcard",
            "apps_count": apps_count,
            "apps_running": apps_running,
        }));
    }

    Json(json!({"success": true, "routes": routes}))
}
