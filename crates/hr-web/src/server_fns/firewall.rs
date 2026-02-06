use leptos::prelude::*;

use crate::types::FirewallData;
#[cfg(feature = "ssr")]
use crate::types::FirewallRuleInfo;

#[server]
pub async fn get_firewall_data() -> Result<FirewallData, ServerFnError> {
    use std::sync::Arc;
    use hr_firewall::FirewallEngine;

    let firewall: Option<Arc<FirewallEngine>> = expect_context();

    let engine = match firewall {
        Some(ref e) => e,
        None => {
            return Ok(FirewallData {
                available: false,
                enabled: false,
                lan_interface: String::new(),
                wan_interface: String::new(),
                default_policy: String::new(),
                lan_prefix: None,
                rules: vec![],
            })
        }
    };

    let config = engine.get_config().await;
    let lan_prefix = engine.get_lan_prefix().await;
    let rules: Vec<FirewallRuleInfo> = config
        .allow_rules
        .into_iter()
        .map(|r| FirewallRuleInfo {
            id: r.id,
            description: r.description,
            protocol: r.protocol,
            dest_port: r.dest_port,
            dest_port_end: r.dest_port_end,
            dest_address: r.dest_address,
            source_address: r.source_address,
            enabled: r.enabled,
        })
        .collect();

    Ok(FirewallData {
        available: true,
        enabled: config.enabled,
        lan_interface: config.lan_interface,
        wan_interface: config.wan_interface,
        default_policy: config.default_inbound_policy,
        lan_prefix,
        rules,
    })
}

#[server]
pub async fn add_firewall_rule(
    protocol: String,
    dest_port: u16,
    dest_port_end: u16,
    dest_address: String,
    source_address: String,
    description: String,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_firewall::{FirewallEngine, FirewallRule};

    let firewall: Option<Arc<FirewallEngine>> = expect_context();
    let engine = firewall.ok_or_else(|| ServerFnError::new("Firewall non disponible"))?;

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

    let rule = FirewallRule {
        id,
        description,
        protocol,
        dest_port,
        dest_port_end,
        dest_address,
        source_address,
        enabled: true,
    };

    engine
        .add_rule(rule)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/firewall?msg=R%C3%A8gle+ajout%C3%A9e");
    Ok(())
}

#[server]
pub async fn toggle_firewall_rule(id: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_firewall::FirewallEngine;

    let firewall: Option<Arc<FirewallEngine>> = expect_context();
    let engine = firewall.ok_or_else(|| ServerFnError::new("Firewall non disponible"))?;

    engine
        .toggle_rule(&id)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?
        .ok_or_else(|| ServerFnError::new("Règle introuvable"))?;

    leptos_axum::redirect("/firewall?msg=R%C3%A8gle+modifi%C3%A9e");
    Ok(())
}

#[server]
pub async fn delete_firewall_rule(id: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_firewall::FirewallEngine;

    let firewall: Option<Arc<FirewallEngine>> = expect_context();
    let engine = firewall.ok_or_else(|| ServerFnError::new("Firewall non disponible"))?;

    let found = engine
        .remove_rule(&id)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    if !found {
        return Err(ServerFnError::new("Règle introuvable"));
    }
    leptos_axum::redirect("/firewall?msg=R%C3%A8gle+supprim%C3%A9e");
    Ok(())
}
