pub mod cache;
pub mod config;
pub mod logging;
pub mod packet;
pub mod records;
pub mod resolver;
pub mod server;
pub mod upstream;

pub use config::DnsConfig;

use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DnsState {
    pub config: config::DnsConfig,
    pub dns_cache: cache::DnsCache,
    pub upstream: upstream::UpstreamForwarder,
    pub query_logger: Option<logging::QueryLogger>,
    pub adblock: Arc<RwLock<hr_adblock::AdblockEngine>>,
    pub lease_store: Arc<RwLock<hr_dhcp::LeaseStore>>,
    pub adblock_enabled: bool,
    pub adblock_block_response: String,
}

impl DnsState {
    pub fn server_ip(&self) -> std::net::Ipv4Addr {
        self.config
            .listen_addresses
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(std::net::Ipv4Addr::UNSPECIFIED)
    }

    /// Add a static record at runtime (not persisted).
    /// Deduplicates by name + record_type: if an existing record has the same
    /// name (case-insensitive) and type, it is replaced.
    pub fn add_static_record(&mut self, record: config::StaticRecord) {
        let name_lc = record.name.to_lowercase();
        let rtype = record.record_type.to_uppercase();
        // Remove any existing record with same name+type
        self.config.static_records.retain(|r| {
            !(r.name.to_lowercase() == name_lc && r.record_type.to_uppercase() == rtype)
        });
        self.config.static_records.push(record);
    }

    /// Remove all static records whose value matches the given string.
    /// Useful for cleaning up all DNS records pointing to a specific IP.
    pub fn remove_static_records_by_value(&mut self, value: &str) {
        self.config.static_records.retain(|r| r.value != value);
    }

    /// Replace the full set of records owned by `owner`. Records with a
    /// different `managed_by` (including `None` = user records) are preserved.
    /// New records are stamped with `managed_by = Some(owner)` to make ownership
    /// idempotent across calls.
    ///
    /// The DNS cache is NOT cleared here — the caller (handler) is expected to
    /// invalidate it after mutation, since `clear()` is async and we want to
    /// keep this method synchronous like its peers.
    pub fn replace_managed_records(
        &mut self,
        owner: &str,
        new_records: Vec<config::StaticRecord>,
    ) {
        config::replace_managed_in(&mut self.config.static_records, owner, new_records);
    }
}

pub type SharedDnsState = Arc<RwLock<DnsState>>;
