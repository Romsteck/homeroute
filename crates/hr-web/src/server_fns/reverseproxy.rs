use leptos::prelude::*;

use crate::types::ReverseProxyPageData;
#[cfg(feature = "ssr")]
use crate::types::ProxyHost;

#[server]
pub async fn get_reverseproxy_data() -> Result<ReverseProxyPageData, ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;
    use hr_proxy::ProxyState;

    let env: Arc<EnvConfig> = expect_context();
    let proxy: Arc<ProxyState> = expect_context();

    // Read reverseproxy-config.json
    let config_json = tokio::fs::read_to_string(&env.reverseproxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let config: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));

    let base_domain = config
        .get("baseDomain")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();

    let local_networks: Vec<String> = config
        .get("localNetworks")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let hosts: Vec<ProxyHost> = config
        .get("hosts")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    Some(ProxyHost {
                        id: h.get("id")?.as_str()?.to_string(),
                        subdomain: h
                            .get("subdomain")
                            .and_then(|s| s.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        custom_domain: h
                            .get("customDomain")
                            .and_then(|s| s.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        target_host: h.get("targetHost")?.as_str()?.to_string(),
                        target_port: h.get("targetPort")?.as_u64()? as u16,
                        enabled: h.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false),
                        local_only: h
                            .get("localOnly")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false),
                        require_auth: h
                            .get("requireAuth")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Get proxy status from ProxyConfig struct
    let proxy_config = proxy.config();
    let active_routes = proxy_config.active_routes().len();

    Ok(ReverseProxyPageData {
        base_domain,
        hosts,
        proxy_running: true,
        active_routes,
        local_networks,
    })
}

#[server]
pub async fn add_proxy_host(
    subdomain: String,
    custom_domain: String,
    target_host: String,
    target_port: u16,
    local_only: Option<String>,
    require_auth: Option<String>,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    let config_json = tokio::fs::read_to_string(&env.reverseproxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let mut config: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));

    let id = {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    };
    let host = serde_json::json!({
        "id": id,
        "subdomain": subdomain,
        "customDomain": if custom_domain.is_empty() { serde_json::Value::Null } else { serde_json::json!(custom_domain) },
        "targetHost": target_host,
        "targetPort": target_port,
        "enabled": true,
        "localOnly": local_only.is_some(),
        "requireAuth": require_auth.is_some(),
    });

    let hosts = config
        .get_mut("hosts")
        .and_then(|h| h.as_array_mut());
    match hosts {
        Some(arr) => arr.push(host),
        None => config["hosts"] = serde_json::json!([host]),
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::write(&env.reverseproxy_config_path, &content)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    // Reload proxy
    reload_proxy_config(&env).await;

    leptos_axum::redirect("/reverseproxy?msg=H%C3%B4te+ajout%C3%A9");
    Ok(())
}

#[server]
pub async fn delete_proxy_host(id: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    let config_json = tokio::fs::read_to_string(&env.reverseproxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let mut config: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));

    if let Some(hosts) = config.get_mut("hosts").and_then(|h| h.as_array_mut()) {
        hosts.retain(|h| h.get("id").and_then(|i| i.as_str()) != Some(&id));
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::write(&env.reverseproxy_config_path, &content)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    reload_proxy_config(&env).await;

    leptos_axum::redirect("/reverseproxy?msg=H%C3%B4te+supprim%C3%A9");
    Ok(())
}

#[server]
pub async fn toggle_proxy_host(id: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    let config_json = tokio::fs::read_to_string(&env.reverseproxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let mut config: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));

    if let Some(hosts) = config.get_mut("hosts").and_then(|h| h.as_array_mut()) {
        if let Some(host) = hosts.iter_mut().find(|h| h.get("id").and_then(|i| i.as_str()) == Some(&id)) {
            let current = host.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            host["enabled"] = serde_json::json!(!current);
        }
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::write(&env.reverseproxy_config_path, &content)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    reload_proxy_config(&env).await;

    leptos_axum::redirect("/reverseproxy?msg=%C3%89tat+modifi%C3%A9");
    Ok(())
}

#[cfg(feature = "ssr")]
async fn reload_proxy_config(env: &hr_common::config::EnvConfig) {
    use std::sync::Arc;
    use hr_proxy::ProxyState;

    let proxy: Arc<ProxyState> = expect_context();

    // Rebuild proxy routes from reverseproxy config
    let rp_json = tokio::fs::read_to_string(&env.reverseproxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let rp_config: serde_json::Value =
        serde_json::from_str(&rp_json).unwrap_or(serde_json::json!({}));

    let base_domain = rp_config
        .get("baseDomain")
        .and_then(|d| d.as_str())
        .unwrap_or("");

    let hosts = rp_config
        .get("hosts")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();

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

        routes.push(serde_json::json!({
            "id": host.get("id").unwrap_or(&serde_json::json!("")),
            "domain": domain,
            "backend": "rust",
            "target_host": host.get("targetHost").unwrap_or(&serde_json::json!("localhost")),
            "target_port": host.get("targetPort").unwrap_or(&serde_json::json!(80)),
            "local_only": host.get("localOnly").unwrap_or(&serde_json::json!(false)),
            "require_auth": host.get("requireAuth").unwrap_or(&serde_json::json!(false)),
            "enabled": true
        }));
    }

    // Update proxy config file
    let proxy_config_path = &env.proxy_config_path;
    let proxy_content = tokio::fs::read_to_string(proxy_config_path)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let mut proxy_config: serde_json::Value =
        serde_json::from_str(&proxy_content).unwrap_or_else(|_| serde_json::json!({}));

    proxy_config["routes"] = serde_json::json!(routes);
    proxy_config["base_domain"] = serde_json::json!(base_domain);

    if let Some(networks) = rp_config.get("localNetworks").and_then(|n| n.as_array()) {
        proxy_config["local_networks"] = serde_json::json!(networks);
    }

    if let Ok(content) = serde_json::to_string_pretty(&proxy_config) {
        let _ = tokio::fs::write(proxy_config_path, &content).await;
    }

    // Reload proxy in memory
    if let Ok(new_proxy_config) = hr_proxy::ProxyConfig::load_from_file(proxy_config_path) {
        proxy.reload_config(new_proxy_config);
    }
}
