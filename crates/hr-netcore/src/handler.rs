use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use hr_adblock::AdblockEngine;
use hr_common::service_registry::SharedServiceRegistry;
use hr_ipc::server::IpcHandler;
use hr_ipc::types::*;

pub struct NetcoreHandler {
    pub dns_state: hr_dns::SharedDnsState,
    pub dhcp_state: hr_dhcp::SharedDhcpState,
    pub adblock: Arc<RwLock<AdblockEngine>>,
    pub service_registry: SharedServiceRegistry,
    pub dns_dhcp_config_path: PathBuf,
}

impl IpcHandler for NetcoreHandler {
    async fn handle(&self, request: IpcRequest) -> IpcResponse {
        match request {
            IpcRequest::ReloadConfig => self.handle_reload_config().await,
            IpcRequest::DnsCacheStats => self.handle_dns_cache_stats().await,
            IpcRequest::DnsStatus => self.handle_dns_status().await,
            IpcRequest::DnsStaticRecords => self.handle_dns_static_records().await,
            IpcRequest::DnsAddStaticRecord {
                name,
                record_type,
                value,
                ttl,
            } => {
                self.handle_dns_add_static_record(name, record_type, value, ttl)
                    .await
            }
            IpcRequest::DnsRemoveStaticRecordsByValue { value } => {
                self.handle_dns_remove_static_records_by_value(value).await
            }
            IpcRequest::DhcpLeases => self.handle_dhcp_leases().await,
            IpcRequest::AdblockStats => self.handle_adblock_stats().await,
            IpcRequest::AdblockWhitelistList => self.handle_adblock_whitelist_list().await,
            IpcRequest::AdblockWhitelistAdd { domain } => {
                self.handle_adblock_whitelist_add(domain).await
            }
            IpcRequest::AdblockWhitelistRemove { domain } => {
                self.handle_adblock_whitelist_remove(domain).await
            }
            IpcRequest::AdblockUpdate => self.handle_adblock_update().await,
            IpcRequest::AdblockSearch { query, limit } => {
                self.handle_adblock_search(query, limit).await
            }
            IpcRequest::ServiceStatus => self.handle_service_status().await,
        }
    }
}

impl NetcoreHandler {
    // ── ReloadConfig ────────────────────────────────────────────────────

    async fn handle_reload_config(&self) -> IpcResponse {
        info!("IPC: ReloadConfig requested");

        match self.load_dns_dhcp_config() {
            Ok(new_config) => {
                let mut s = self.dns_state.write().await;
                s.upstream = hr_dns::upstream::UpstreamForwarder::new(
                    new_config.dns.upstream_servers.clone(),
                    new_config.dns.upstream_timeout_ms,
                );
                s.config = new_config.dns;
                s.adblock_enabled = new_config.adblock.enabled;
                s.adblock_block_response = new_config.adblock.block_response;
                s.dns_cache.clear().await;

                let mut ab = self.adblock.write().await;
                ab.set_whitelist(new_config.adblock.whitelist);

                info!("DNS/DHCP config reloaded via IPC");
                IpcResponse::ok_empty()
            }
            Err(e) => {
                error!("Failed to reload DNS/DHCP config: {}", e);
                IpcResponse::err(format!("Failed to reload config: {}", e))
            }
        }
    }

    // ── DnsCacheStats ───────────────────────────────────────────────────

    async fn handle_dns_cache_stats(&self) -> IpcResponse {
        let s = self.dns_state.read().await;
        let ab = self.adblock.read().await;
        IpcResponse::ok_data(DnsCacheStatsData {
            cache_size: s.dns_cache.len().await,
            adblock_enabled: s.adblock_enabled,
            adblock_domains: ab.domain_count(),
        })
    }

    // ── DnsStatus ───────────────────────────────────────────────────────

    async fn handle_dns_status(&self) -> IpcResponse {
        let s = self.dns_state.read().await;
        IpcResponse::ok_data(DnsStatusData {
            active: true,
            port: s.config.port,
            upstream_servers: s.config.upstream_servers.clone(),
            cache_size: s.dns_cache.len().await,
            local_domain: s.config.local_domain.clone(),
            adblock_enabled: s.adblock_enabled,
        })
    }

    // ── DnsStaticRecords ────────────────────────────────────────────────

    async fn handle_dns_static_records(&self) -> IpcResponse {
        let s = self.dns_state.read().await;
        let records: Vec<StaticRecordDto> = s
            .config
            .static_records
            .iter()
            .map(|r| StaticRecordDto {
                name: r.name.clone(),
                record_type: r.record_type.clone(),
                value: r.value.clone(),
                ttl: r.ttl,
            })
            .collect();
        IpcResponse::ok_data(DnsStaticRecordsData { records })
    }

    // ── DnsAddStaticRecord ──────────────────────────────────────────────

    async fn handle_dns_add_static_record(
        &self,
        name: String,
        record_type: String,
        value: String,
        ttl: u32,
    ) -> IpcResponse {
        let mut s = self.dns_state.write().await;
        s.add_static_record(hr_dns::config::StaticRecord {
            name,
            record_type,
            value,
            ttl,
        });
        IpcResponse::ok_empty()
    }

    // ── DnsRemoveStaticRecordsByValue ───────────────────────────────────

    async fn handle_dns_remove_static_records_by_value(&self, value: String) -> IpcResponse {
        let mut s = self.dns_state.write().await;
        s.remove_static_records_by_value(&value);
        IpcResponse::ok_empty()
    }

    // ── DhcpLeases ──────────────────────────────────────────────────────

    async fn handle_dhcp_leases(&self) -> IpcResponse {
        let mut s = self.dhcp_state.write().await;
        s.lease_store.purge_expired();
        let leases: Vec<LeaseInfo> = s
            .lease_store
            .all_leases()
            .into_iter()
            .map(|l| LeaseInfo {
                ip: l.ip.to_string(),
                mac: l.mac.clone(),
                hostname: l.hostname.clone(),
                expiry: l.expiry,
                client_id: l.client_id.clone(),
            })
            .collect();
        IpcResponse::ok_data(leases)
    }

    // ── AdblockStats ────────────────────────────────────────────────────

    async fn handle_adblock_stats(&self) -> IpcResponse {
        let engine = self.adblock.read().await;
        let dns = self.dns_state.read().await;

        // Read sources from config for display
        let sources = self.read_adblock_sources().await;

        // Check cache file mtime for lastUpdate
        let last_update =
            tokio::fs::metadata("/var/lib/server-dashboard/adblock/domains.json")
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64
                });

        IpcResponse::ok_data(AdblockStatsData {
            domain_count: engine.domain_count(),
            enabled: dns.adblock_enabled,
            sources,
            last_update,
        })
    }

    // ── AdblockWhitelistList ────────────────────────────────────────────

    async fn handle_adblock_whitelist_list(&self) -> IpcResponse {
        let engine = self.adblock.read().await;
        let domains = engine.whitelist_domains();
        IpcResponse::ok_data(domains)
    }

    // ── AdblockWhitelistAdd ─────────────────────────────────────────────

    async fn handle_adblock_whitelist_add(&self, domain: String) -> IpcResponse {
        let domain = domain.to_lowercase().trim().to_string();
        if domain.is_empty() {
            return IpcResponse::err("Domain requis");
        }

        // Read current config, add domain to whitelist, save atomically
        let config_path = &self.dns_dhcp_config_path;
        let content = match tokio::fs::read_to_string(config_path).await {
            Ok(c) => c,
            Err(e) => {
                return IpcResponse::err(format!("Config read error: {}", e));
            }
        };

        let mut config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                return IpcResponse::err(format!("Config parse error: {}", e));
            }
        };

        // Update whitelist in config
        let adblock = config.get_mut("adblock").and_then(|a| a.as_object_mut());
        if let Some(adblock) = adblock {
            let whitelist = adblock
                .entry("whitelist")
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut();
            if let Some(wl) = whitelist {
                let domain_val = serde_json::json!(domain);
                if !wl.contains(&domain_val) {
                    wl.push(domain_val);
                }
            }
        }

        // Save config atomically
        if let Ok(new_content) = serde_json::to_string_pretty(&config) {
            let tmp = config_path.with_extension("json.tmp");
            let _ = tokio::fs::write(&tmp, &new_content).await;
            let _ = tokio::fs::rename(&tmp, config_path).await;
        }

        // Update engine in memory
        {
            let mut engine = self.adblock.write().await;
            let mut domains = engine.whitelist_domains();
            if !domains.contains(&domain) {
                domains.push(domain.clone());
            }
            engine.set_whitelist(domains);
        }

        IpcResponse::ok_data(serde_json::json!({ "domain": domain }))
    }

    // ── AdblockWhitelistRemove ──────────────────────────────────────────

    async fn handle_adblock_whitelist_remove(&self, domain: String) -> IpcResponse {
        let domain = domain.to_lowercase();

        // Update config file
        let config_path = &self.dns_dhcp_config_path;
        if let Ok(content) = tokio::fs::read_to_string(config_path).await {
            if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(adblock) =
                    config.get_mut("adblock").and_then(|a| a.as_object_mut())
                {
                    if let Some(wl) =
                        adblock.get_mut("whitelist").and_then(|w| w.as_array_mut())
                    {
                        wl.retain(|d| d.as_str() != Some(&domain));
                    }
                }
                if let Ok(new_content) = serde_json::to_string_pretty(&config) {
                    let tmp = config_path.with_extension("json.tmp");
                    let _ = tokio::fs::write(&tmp, &new_content).await;
                    let _ = tokio::fs::rename(&tmp, config_path).await;
                }
            }
        }

        // Update engine in memory
        {
            let mut engine = self.adblock.write().await;
            let mut domains = engine.whitelist_domains();
            domains.retain(|d| d != &domain);
            engine.set_whitelist(domains);
        }

        IpcResponse::ok_empty()
    }

    // ── AdblockUpdate ───────────────────────────────────────────────────

    async fn handle_adblock_update(&self) -> IpcResponse {
        // Read adblock config from file
        let config_path = &self.dns_dhcp_config_path;
        let content = match tokio::fs::read_to_string(config_path).await {
            Ok(c) => c,
            Err(e) => {
                return IpcResponse::err(format!("Config read error: {}", e));
            }
        };

        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                return IpcResponse::err(format!("Config parse error: {}", e));
            }
        };

        let adblock_config: hr_adblock::config::AdblockConfig = match config
            .get("adblock")
            .map(|v| serde_json::from_value(v.clone()))
        {
            Some(Ok(c)) => c,
            _ => hr_adblock::config::AdblockConfig::default(),
        };

        // Download and update
        let (domains, results) =
            hr_adblock::sources::download_all(&adblock_config.sources).await;
        let count = domains.len();

        // Save cache
        let cache_path =
            std::path::PathBuf::from(&adblock_config.data_dir).join("domains.json");
        let _ = hr_adblock::sources::save_cache(&domains, &cache_path);

        // Apply to engine
        {
            let mut engine = self.adblock.write().await;
            engine.set_blocked(domains);
            engine.set_whitelist(adblock_config.whitelist);
        }

        let sources: Vec<AdblockSourceResult> = results
            .iter()
            .map(|r| AdblockSourceResult {
                name: r.name.clone(),
                domains: r.domain_count,
            })
            .collect();

        IpcResponse::ok_data(AdblockUpdateResult {
            total_domains: count,
            sources,
        })
    }

    // ── AdblockSearch ───────────────────────────────────────────────────

    async fn handle_adblock_search(
        &self,
        query: String,
        limit: Option<usize>,
    ) -> IpcResponse {
        if query.is_empty() {
            return IpcResponse::ok_data(AdblockSearchResult {
                query: String::new(),
                is_blocked: false,
                results: vec![],
            });
        }

        let engine = self.adblock.read().await;
        let results = engine.search(&query, limit.unwrap_or(50));
        let is_blocked = engine.is_blocked(&query);

        IpcResponse::ok_data(AdblockSearchResult {
            query,
            is_blocked,
            results,
        })
    }

    // ── ServiceStatus ───────────────────────────────────────────────────

    async fn handle_service_status(&self) -> IpcResponse {
        let reg = self.service_registry.read().await;
        let entries: Vec<ServiceStatusEntry> = reg
            .values()
            .map(|s| ServiceStatusEntry {
                name: s.name.clone(),
                state: format!("{:?}", s.state),
                priority: format!("{:?}", s.priority),
                restart_count: s.restart_count,
                last_state_change: s.last_state_change,
                error: s.error.clone(),
            })
            .collect();
        IpcResponse::ok_data(entries)
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn load_dns_dhcp_config(&self) -> anyhow::Result<DnsDhcpConfigRaw> {
        let path = &self.dns_dhcp_config_path;
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(DnsDhcpConfigRaw::default())
        }
    }

    async fn read_adblock_sources(&self) -> Vec<AdblockSourceInfo> {
        let config_path = &self.dns_dhcp_config_path;
        let content = match tokio::fs::read_to_string(config_path).await {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return vec![],
        };
        config
            .get("adblock")
            .and_then(|a| a.get("sources"))
            .and_then(|s| s.as_array())
            .map(|sources| {
                sources
                    .iter()
                    .map(|s| AdblockSourceInfo {
                        name: s
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        url: s
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Combined config from dns-dhcp-config.json (matches the original file layout).
/// This is a local copy used only for config reload in the handler.
/// All fields must be present for correct deserialization even if not all are read.
#[derive(serde::Deserialize, Default)]
#[allow(dead_code)]
pub struct DnsDhcpConfigRaw {
    #[serde(default)]
    pub dns: hr_dns::DnsConfig,
    #[serde(default)]
    pub dhcp: hr_dhcp::DhcpConfig,
    #[serde(default)]
    pub ipv6: hr_ipv6::Ipv6Config,
    #[serde(default)]
    pub adblock: hr_adblock::config::AdblockConfig,
}
