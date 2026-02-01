use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::adblock::AdblockEngine;
use crate::config::Config;
use crate::dhcp::lease_store::LeaseStore;
use crate::dns::cache::DnsCache;
use crate::dns::upstream::UpstreamForwarder;
use crate::logging::QueryLogger;

/// Shared server state, wrapped in Arc<RwLock<>> for hot-reload support.
pub struct ServerState {
    pub config: Config,
    pub dns_cache: DnsCache,
    pub lease_store: LeaseStore,
    pub adblock: AdblockEngine,
    pub query_logger: Option<QueryLogger>,
    pub upstream: UpstreamForwarder,
}

impl ServerState {
    pub fn server_ip(&self) -> Ipv4Addr {
        self.config
            .dns
            .listen_addresses
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(Ipv4Addr::UNSPECIFIED)
    }
}

pub type SharedState = Arc<RwLock<ServerState>>;
