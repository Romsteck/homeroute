//! Firewall engine: manages state and reacts to prefix changes.

use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use anyhow::Result;
use tracing::{info, warn, error};

use hr_ipv6::PrefixInfo;

use crate::config::{FirewallConfig, FirewallRule};
use crate::nftables;

pub struct FirewallEngine {
    config: Arc<RwLock<FirewallConfig>>,
    lan_prefix: Arc<RwLock<Option<String>>>,
}

impl FirewallEngine {
    pub fn new(config: FirewallConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            lan_prefix: Arc::new(RwLock::new(None)),
        }
    }

    /// Update the LAN prefix and re-apply firewall rules.
    pub async fn update_lan_prefix(&self, prefix: &str) -> Result<()> {
        {
            let mut p = self.lan_prefix.write().await;
            *p = Some(prefix.to_string());
        }
        self.apply().await
    }

    /// Clear the LAN prefix and flush firewall rules.
    pub async fn clear_lan_prefix(&self) -> Result<()> {
        {
            let mut p = self.lan_prefix.write().await;
            *p = None;
        }
        nftables::flush_rules().await
    }

    pub async fn add_rule(&self, rule: FirewallRule) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.allow_rules.push(rule);
            config.save()?;
        }
        self.apply().await
    }

    pub async fn remove_rule(&self, rule_id: &str) -> Result<bool> {
        let removed = {
            let mut config = self.config.write().await;
            let before = config.allow_rules.len();
            config.allow_rules.retain(|r| r.id != rule_id);
            let removed = config.allow_rules.len() < before;
            if removed {
                config.save()?;
            }
            removed
        };
        if removed {
            self.apply().await?;
        }
        Ok(removed)
    }

    pub async fn toggle_rule(&self, rule_id: &str) -> Result<Option<bool>> {
        let new_state = {
            let mut config = self.config.write().await;
            let rule = config.allow_rules.iter_mut().find(|r| r.id == rule_id);
            match rule {
                Some(r) => {
                    r.enabled = !r.enabled;
                    let state = r.enabled;
                    config.save()?;
                    Some(state)
                }
                None => None,
            }
        };
        if new_state.is_some() {
            self.apply().await?;
        }
        Ok(new_state)
    }

    /// Apply the current config to nftables.
    pub async fn apply(&self) -> Result<()> {
        let config = self.config.read().await;
        let prefix = self.lan_prefix.read().await;

        match &*prefix {
            Some(pfx) => nftables::apply_ruleset(&config, pfx).await,
            None => {
                // No GUA prefix yet — nothing to protect
                info!("Firewall: no LAN prefix, skipping rule application");
                Ok(())
            }
        }
    }

    pub async fn get_config(&self) -> FirewallConfig {
        self.config.read().await.clone()
    }

    pub async fn get_lan_prefix(&self) -> Option<String> {
        self.lan_prefix.read().await.clone()
    }

    pub async fn get_rules(&self) -> Vec<FirewallRule> {
        self.config.read().await.allow_rules.clone()
    }
}

/// Run the firewall service: listens for prefix changes and applies rules.
pub async fn run_firewall(
    engine: Arc<FirewallEngine>,
    mut prefix_rx: watch::Receiver<Option<PrefixInfo>>,
) -> Result<()> {
    info!("IPv6 firewall service started");

    // Apply initial rules if a prefix is already available
    let initial = prefix_rx.borrow().clone();
    if let Some(info) = initial {
        let prefix_str = format!("{}/{}", info.prefix, info.prefix_len);
        if let Err(e) = engine.update_lan_prefix(&prefix_str).await {
            error!("Failed to apply initial firewall rules: {}", e);
        }
    }

    // Watch for prefix changes
    loop {
        if prefix_rx.changed().await.is_err() {
            warn!("Prefix watch channel closed, firewall service stopping");
            break;
        }

        let current = prefix_rx.borrow().clone();
        match current {
            Some(info) => {
                let prefix_str = format!("{}/{}", info.prefix, info.prefix_len);
                info!("Firewall: updating rules for prefix {}", prefix_str);
                if let Err(e) = engine.update_lan_prefix(&prefix_str).await {
                    error!("Failed to update firewall for new prefix: {}", e);
                }
            }
            None => {
                info!("Firewall: prefix withdrawn, flushing rules");
                if let Err(e) = engine.clear_lan_prefix().await {
                    error!("Failed to flush firewall rules: {}", e);
                }
            }
        }
    }

    Ok(())
}
