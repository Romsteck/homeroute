use hr_adblock::AdblockEngine;
use hr_analytics::store::AnalyticsStore;
use hr_auth::AuthService;
use hr_acme::AcmeManager;
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_common::service_registry::SharedServiceRegistry;
use hr_dns::SharedDnsState;
use hr_dhcp::SharedDhcpState;
use hr_firewall::FirewallEngine;
use hr_proxy::{ProxyState, TlsManager};
use hr_registry::AgentRegistry;
use leptos::prelude::LeptosOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Combined state: API services + Leptos options.
/// Manual `FromRef` impls let both API handlers (extracting `ApiState`) and
/// Leptos (extracting `LeptosOptions`) coexist in the same router.
#[derive(Clone)]
pub struct AppState {
    pub api: ApiState,
    pub leptos_options: LeptosOptions,
}

impl axum::extract::FromRef<AppState> for ApiState {
    fn from_ref(state: &AppState) -> Self {
        state.api.clone()
    }
}

impl axum::extract::FromRef<AppState> for LeptosOptions {
    fn from_ref(state: &AppState) -> Self {
        state.leptos_options.clone()
    }
}

/// Shared application state for all API routes.
#[derive(Clone)]
pub struct ApiState {
    pub auth: Arc<AuthService>,
    pub acme: Arc<AcmeManager>,
    pub proxy: Arc<ProxyState>,
    pub tls_manager: Arc<TlsManager>,
    pub dns: SharedDnsState,
    pub dhcp: SharedDhcpState,
    pub adblock: Arc<RwLock<AdblockEngine>>,
    pub events: Arc<EventBus>,
    pub env: Arc<EnvConfig>,
    pub analytics: Arc<AnalyticsStore>,
    pub service_registry: SharedServiceRegistry,
    pub firewall: Option<Arc<FirewallEngine>>,
    pub registry: Option<Arc<AgentRegistry>>,

    /// Path to dns-dhcp-config.json
    pub dns_dhcp_config_path: PathBuf,
    /// Path to rust-proxy-config.json
    pub proxy_config_path: PathBuf,
    /// Path to reverseproxy-config.json
    pub reverseproxy_config_path: PathBuf,
}
