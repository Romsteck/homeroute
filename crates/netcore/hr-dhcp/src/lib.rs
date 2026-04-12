pub mod config;
pub mod lease_store;
pub mod options;
pub mod packet;
pub mod server;
pub mod state_machine;

pub use config::DhcpConfig;
pub use lease_store::LeaseStore;

use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DhcpState {
    pub config: config::DhcpConfig,
    pub lease_store: lease_store::LeaseStore,
    pub server_ip: Ipv4Addr,
}

pub type SharedDhcpState = Arc<RwLock<DhcpState>>;
