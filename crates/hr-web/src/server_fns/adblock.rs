use leptos::prelude::*;

use crate::types::{AdblockPageData, AdblockSearchResult};
#[cfg(feature = "ssr")]
use crate::types::AdblockSource;

#[server]
pub async fn get_adblock_data() -> Result<AdblockPageData, ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;
    use hr_common::config::EnvConfig;

    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let env: Arc<EnvConfig> = expect_context();

    let engine = adblock.read().await;
    let domain_count = engine.domain_count();
    let whitelist = engine.whitelist_domains();
    drop(engine);

    let (sources, enabled) = tokio::fs::read_to_string(&env.dns_dhcp_config_path)
        .await
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            let sources = v
                .get("adblock")
                .and_then(|a| a.get("sources"))
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| {
                            Some(AdblockSource {
                                name: s.get("name")?.as_str()?.to_string(),
                                url: s.get("url")?.as_str()?.to_string(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let en = v
                .get("adblock")
                .and_then(|a| a.get("enabled"))
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            (sources, en)
        })
        .unwrap_or_default();

    let source_count = sources.len();
    let whitelist_count = whitelist.len();

    Ok(AdblockPageData {
        enabled,
        domain_count,
        source_count,
        whitelist_count,
        sources,
        whitelist,
    })
}

#[server]
pub async fn update_adblock_sources() -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;
    use hr_common::config::EnvConfig;

    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let env: Arc<EnvConfig> = expect_context();

    let content = tokio::fs::read_to_string(&env.dns_dhcp_config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur lecture config: {e}")))?;
    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ServerFnError::new(format!("Erreur parse config: {e}")))?;

    let adblock_config: hr_adblock::config::AdblockConfig = config
        .get("adblock")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ServerFnError::new(format!("Erreur config adblock: {e}")))?
        .unwrap_or_default();

    let (domains, _results) = hr_adblock::sources::download_all(&adblock_config.sources).await;
    let cache_path = std::path::PathBuf::from(&adblock_config.data_dir).join("domains.json");
    let _ = hr_adblock::sources::save_cache(&domains, &cache_path);

    {
        let mut engine = adblock.write().await;
        engine.set_blocked(domains);
        engine.set_whitelist(adblock_config.whitelist);
    }

    leptos_axum::redirect("/adblock?msg=Sources+mises+%C3%A0+jour");
    Ok(())
}

#[server]
pub async fn add_whitelist_domain(domain: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;
    use hr_common::config::EnvConfig;

    let domain = domain.to_lowercase().trim().to_string();
    if domain.is_empty() {
        return Err(ServerFnError::new("Domaine requis"));
    }

    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let env: Arc<EnvConfig> = expect_context();

    // Update config file
    let content = tokio::fs::read_to_string(&env.dns_dhcp_config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur lecture config: {e}")))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ServerFnError::new(format!("Erreur parse config: {e}")))?;

    if let Some(adblock_obj) = config.get_mut("adblock").and_then(|a| a.as_object_mut()) {
        let whitelist = adblock_obj
            .entry("whitelist")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut();
        if let Some(wl) = whitelist {
            let val = serde_json::json!(domain);
            if !wl.contains(&val) {
                wl.push(val);
            }
        }
    }

    let new_content = serde_json::to_string_pretty(&config)
        .map_err(|e| ServerFnError::new(format!("Erreur sérialisation: {e}")))?;
    let config_path = std::path::PathBuf::from(&env.dns_dhcp_config_path);
    let tmp = config_path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &new_content)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur écriture: {e}")))?;
    tokio::fs::rename(&tmp, &config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur rename: {e}")))?;

    // Update engine in memory
    {
        let mut engine = adblock.write().await;
        let mut domains = engine.whitelist_domains();
        if !domains.contains(&domain) {
            domains.push(domain);
        }
        engine.set_whitelist(domains);
    }

    leptos_axum::redirect("/adblock?msg=Domaine+ajout%C3%A9");
    Ok(())
}

#[server]
pub async fn remove_whitelist_domain(domain: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;
    use hr_common::config::EnvConfig;

    let domain = domain.to_lowercase();

    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let env: Arc<EnvConfig> = expect_context();

    // Update config file
    let content = tokio::fs::read_to_string(&env.dns_dhcp_config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur lecture config: {e}")))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ServerFnError::new(format!("Erreur parse config: {e}")))?;

    if let Some(adblock_obj) = config.get_mut("adblock").and_then(|a| a.as_object_mut()) {
        if let Some(wl) = adblock_obj.get_mut("whitelist").and_then(|w| w.as_array_mut()) {
            wl.retain(|d| d.as_str() != Some(&domain));
        }
    }

    let new_content = serde_json::to_string_pretty(&config)
        .map_err(|e| ServerFnError::new(format!("Erreur sérialisation: {e}")))?;
    let config_path = std::path::PathBuf::from(&env.dns_dhcp_config_path);
    let tmp = config_path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &new_content)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur écriture: {e}")))?;
    tokio::fs::rename(&tmp, &config_path)
        .await
        .map_err(|e| ServerFnError::new(format!("Erreur rename: {e}")))?;

    // Update engine in memory
    {
        let mut engine = adblock.write().await;
        let mut domains = engine.whitelist_domains();
        domains.retain(|d| d != &domain);
        engine.set_whitelist(domains);
    }

    leptos_axum::redirect("/adblock?msg=Domaine+supprim%C3%A9");
    Ok(())
}

#[server]
pub async fn search_adblock(query: String) -> Result<AdblockSearchResult, ServerFnError> {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use hr_adblock::filter::AdblockEngine;

    if query.is_empty() {
        return Ok(AdblockSearchResult {
            query,
            is_blocked: false,
            results: vec![],
        });
    }

    let adblock: Arc<RwLock<AdblockEngine>> = expect_context();
    let engine = adblock.read().await;
    let results = engine.search(&query, 50);
    let is_blocked = engine.is_blocked(&query);

    Ok(AdblockSearchResult {
        query,
        is_blocked,
        results,
    })
}
