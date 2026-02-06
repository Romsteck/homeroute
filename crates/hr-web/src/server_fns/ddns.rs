use leptos::prelude::*;

use crate::types::DdnsPageData;

#[server]
pub async fn get_ddns_data() -> Result<DdnsPageData, ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    let configured = env.cf_record_name.is_some();

    let zone_id_masked = env.cf_zone_id.as_ref().map(|z| {
        if z.len() > 4 {
            format!("...{}", &z[z.len() - 4..])
        } else {
            "****".to_string()
        }
    });

    let current_ipv6 = fetch_current_ipv6(&env.cf_interface).await;
    let logs = fetch_ddns_logs().await;

    Ok(DdnsPageData {
        configured,
        record_name: env.cf_record_name.clone(),
        current_ipv6,
        zone_id_masked,
        proxied: env.cf_proxied,
        interface: env.cf_interface.clone(),
        cloudflare_ip: None,
        in_sync: false,
        logs,
    })
}

#[server]
pub async fn force_ddns_update() -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    let token = env
        .cf_api_token
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Token Cloudflare non configuré"))?;
    let zone_id = env
        .cf_zone_id
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Zone ID non configuré"))?;
    let record_name = env
        .cf_record_name
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Nom d'enregistrement non configuré"))?;

    let ipv6 = fetch_current_ipv6(&env.cf_interface)
        .await
        .ok_or_else(|| ServerFnError::new("Impossible de déterminer l'adresse IPv6"))?;

    hr_registry::cloudflare::upsert_aaaa_record(token, zone_id, record_name, &ipv6, env.cf_proxied)
        .await
        .map_err(|e| ServerFnError::new(e))?;

    leptos_axum::redirect("/ddns?msg=Mise+%C3%A0+jour+DNS+lanc%C3%A9e");
    Ok(())
}

#[cfg(feature = "ssr")]
async fn fetch_current_ipv6(interface: &str) -> Option<String> {
    use tokio::process::Command;

    let output = Command::new("ip")
        .args(["-6", "addr", "show", interface, "scope", "global"])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with("inet6") && !line.contains("deprecated") {
            line.split_whitespace()
                .nth(1)
                .and_then(|addr| addr.split('/').next())
                .map(String::from)
        } else {
            None
        }
    })
}

#[cfg(feature = "ssr")]
async fn fetch_ddns_logs() -> Vec<String> {
    tokio::fs::read_to_string("/data/ddns.log")
        .await
        .map(|s| s.lines().rev().take(20).map(String::from).collect())
        .unwrap_or_default()
}
