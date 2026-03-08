use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::warn;

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
        let result = tokio::time::timeout(timeout, self.do_request(req)).await;
        match result {
            Ok(inner) => inner,
            Err(_) => {
                warn!(socket = %self.socket_path.display(), "IPC request timed out");
                Ok(IpcResponse::err("hr-netcore request timed out"))
            }
        }
    }

    async fn do_request(&self, req: &IpcRequest) -> Result<IpcResponse> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .context("hr-netcore unavailable")?;

        let (reader, mut writer) = stream.into_split();

        // Write request as JSON line
        let mut line = serde_json::to_string(req)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.shutdown().await?;

        // Read response
        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await?;

        if response_line.is_empty() {
            return Ok(IpcResponse::err("hr-netcore returned empty response"));
        }

        let resp: IpcResponse = serde_json::from_str(response_line.trim())?;
        Ok(resp)
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
