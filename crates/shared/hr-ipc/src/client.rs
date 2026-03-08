use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::transport;
use crate::types::*;

/// Client IPC pour communiquer avec hr-netcore via Unix socket.
pub struct NetcoreClient {
    socket_path: PathBuf,
}

impl NetcoreClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Envoie une requête IPC et attend la réponse.
    /// Ouvre une connexion par appel (les appels sont rares, UI humaine).
    pub async fn request(&self, req: &IpcRequest) -> Result<IpcResponse> {
        self.request_with_timeout(req, Duration::from_secs(2)).await
    }

    /// Requête avec timeout personnalisé.
    pub async fn request_with_timeout(
        &self,
        req: &IpcRequest,
        timeout: Duration,
    ) -> Result<IpcResponse> {
        match transport::request::<IpcRequest, IpcResponse>(&self.socket_path, req, timeout).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                // Preserve backward-compatible behavior: timeout returns IpcResponse::err
                // instead of propagating anyhow error for timeouts.
                let msg = format!("{e:#}");
                if msg.contains("timed out") {
                    Ok(IpcResponse::err("hr-netcore request timed out"))
                } else {
                    Err(e)
                }
            }
        }
    }

    // ── Typed helpers ────────────────────────────────────────

    pub async fn reload_config(&self) -> Result<IpcResponse> {
        self.request(&IpcRequest::ReloadConfig).await
    }

    pub async fn dns_cache_stats(&self) -> Result<DnsCacheStatsData> {
        let resp = self.request(&IpcRequest::DnsCacheStats).await?;
        extract_data(resp)
    }

    pub async fn dns_status(&self) -> Result<DnsStatusData> {
        let resp = self.request(&IpcRequest::DnsStatus).await?;
        extract_data(resp)
    }

    pub async fn dns_static_records(&self) -> Result<DnsStaticRecordsData> {
        let resp = self.request(&IpcRequest::DnsStaticRecords).await?;
        extract_data(resp)
    }

    pub async fn dns_add_static_record(
        &self,
        name: String,
        record_type: String,
        value: String,
        ttl: u32,
    ) -> Result<IpcResponse> {
        self.request(&IpcRequest::DnsAddStaticRecord { name, record_type, value, ttl }).await
    }

    pub async fn dns_remove_static_records_by_value(&self, value: &str) -> Result<IpcResponse> {
        self.request(&IpcRequest::DnsRemoveStaticRecordsByValue { value: value.to_string() }).await
    }

    pub async fn dhcp_leases(&self) -> Result<Vec<LeaseInfo>> {
        let resp = self.request(&IpcRequest::DhcpLeases).await?;
        extract_data(resp)
    }

    pub async fn adblock_stats(&self) -> Result<AdblockStatsData> {
        let resp = self.request(&IpcRequest::AdblockStats).await?;
        extract_data(resp)
    }

    pub async fn adblock_whitelist_list(&self) -> Result<Vec<String>> {
        let resp = self.request(&IpcRequest::AdblockWhitelistList).await?;
        extract_data(resp)
    }

    pub async fn adblock_whitelist_add(&self, domain: &str) -> Result<IpcResponse> {
        self.request(&IpcRequest::AdblockWhitelistAdd { domain: domain.to_string() }).await
    }

    pub async fn adblock_whitelist_remove(&self, domain: &str) -> Result<IpcResponse> {
        self.request(&IpcRequest::AdblockWhitelistRemove { domain: domain.to_string() }).await
    }

    pub async fn adblock_update(&self) -> Result<AdblockUpdateResult> {
        // Adblock download can be slow — 60s timeout
        let resp = self
            .request_with_timeout(&IpcRequest::AdblockUpdate, Duration::from_secs(60))
            .await?;
        extract_data(resp)
    }

    pub async fn adblock_search(&self, query: &str, limit: Option<usize>) -> Result<AdblockSearchResult> {
        let resp = self
            .request(&IpcRequest::AdblockSearch {
                query: query.to_string(),
                limit,
            })
            .await?;
        extract_data(resp)
    }

    pub async fn service_status(&self) -> Result<Vec<ServiceStatusEntry>> {
        let resp = self.request(&IpcRequest::ServiceStatus).await?;
        extract_data(resp)
    }
}

/// Extract typed data from IpcResponse, returning an error if the response indicates failure.
fn extract_data<T: serde::de::DeserializeOwned>(resp: IpcResponse) -> Result<T> {
    if !resp.ok {
        anyhow::bail!(
            "hr-netcore error: {}",
            resp.error.unwrap_or_else(|| "unknown error".into())
        );
    }
    let data = resp.data.context("hr-netcore returned no data")?;
    Ok(serde_json::from_value(data)?)
}
