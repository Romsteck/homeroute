use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(status))
        .route("/reload", post(reload))
        .route("/config", get(get_config).put(update_config))
        .route("/leases", get(get_leases))
}

async fn status() -> Json<Value> {
    // In the unified binary, the DNS/DHCP service is always running
    Json(json!({
        "success": true,
        "active": true,
        "service": "integrated"
    }))
}

async fn reload(State(state): State<ApiState>) -> Json<Value> {
    // Reload DNS/DHCP config from file and apply
    let config_path = &state.dns_dhcp_config_path;
    let content = match tokio::fs::read_to_string(config_path).await {
        Ok(c) => c,
        Err(e) => {
            return Json(json!({"success": false, "error": format!("Failed to read config: {}", e)}));
        }
    };

    let combined: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return Json(json!({"success": false, "error": format!("Invalid config: {}", e)}));
        }
    };

    // Reload DNS config
    if let Ok(dns_config) = serde_json::from_value::<hr_dns::DnsConfig>(
        combined.get("dns").cloned().unwrap_or(json!({})),
    ) {
        let mut dns = state.dns.write().await;
        dns.config = dns_config;
    }

    // Reload DHCP config
    if let Ok(dhcp_config) = serde_json::from_value::<hr_dhcp::DhcpConfig>(
        combined.get("dhcp").cloned().unwrap_or(json!({})),
    ) {
        let mut dhcp = state.dhcp.write().await;
        dhcp.config = dhcp_config;
    }

    // Reload adblock config
    if let Some(adblock_val) = combined.get("adblock") {
        if let Ok(adblock_config) =
            serde_json::from_value::<hr_adblock::config::AdblockConfig>(adblock_val.clone())
        {
            let mut engine = state.adblock.write().await;
            engine.set_whitelist(adblock_config.whitelist);
        }
    }

    Json(json!({"success": true}))
}

async fn get_config(State(state): State<ApiState>) -> Json<Value> {
    let config_path = &state.dns_dhcp_config_path;
    match tokio::fs::read_to_string(config_path).await {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(mut config) => {
                // Merge in-memory DNS static records (added at runtime by agent connections)
                let dns_state = state.dns.read().await;
                if let Some(dns_obj) = config.get_mut("dns").and_then(|d| d.as_object_mut()) {
                    dns_obj.insert(
                        "static_records".to_string(),
                        serde_json::to_value(&dns_state.config.static_records).unwrap_or_default(),
                    );
                }
                Json(json!({"success": true, "config": config}))
            }
            Err(e) => Json(json!({"success": false, "error": format!("Invalid config: {}", e)})),
        },
        Err(e) => Json(json!({"success": false, "error": format!("Failed to read config: {}", e)})),
    }
}

async fn update_config(
    State(state): State<ApiState>,
    Json(body): Json<Value>,
) -> Json<Value> {
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

    // Apply config by reloading
    reload(State(state)).await
}

async fn get_leases(State(state): State<ApiState>) -> Json<Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Get DHCPv4 leases
    let dhcpv4_leases: Vec<(u64, String, String, Option<String>, Option<String>)> = {
        let mut dhcp = state.dhcp.write().await;
        let purged = dhcp.lease_store.purge_expired();
        if purged > 0 {
            tracing::info!("Purged {} expired DHCPv4 leases", purged);
            let _ = dhcp.lease_store.save_to_file();
        }
        dhcp.lease_store
            .all_leases()
            .iter()
            .filter(|l| l.expiry > now)
            .map(|l| (l.expiry, l.mac.clone(), l.ip.to_string(), l.hostname.clone(), l.client_id.clone()))
            .collect()
    };

    let result: Vec<serde_json::Value> = dhcpv4_leases
        .iter()
        .map(|(expiry, mac, ip, hostname, client_id)| {
            json!({
                "expiry": expiry,
                "mac": mac,
                "ip": ip,
                "hostname": hostname,
                "client_id": client_id,
            })
        })
        .collect();

    Json(json!({"success": true, "leases": result}))
}
