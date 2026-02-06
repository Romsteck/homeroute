/// Lightweight user info for the web UI (shared between SSR and WASM).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WebUserInfo {
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
}

// ── Dashboard types ──────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DashboardData {
    pub interfaces: Vec<InterfaceInfo>,
    pub leases: Vec<LeaseInfo>,
    pub adblock: AdblockInfo,
    pub ddns: DdnsInfo,
    pub services: Vec<ServiceInfo>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub state: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LeaseInfo {
    pub hostname: Option<String>,
    pub ip: String,
    pub mac: String,
    pub expiry: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AdblockInfo {
    pub domain_count: usize,
    pub source_count: usize,
    pub enabled: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DdnsInfo {
    pub record_name: Option<String>,
    pub current_ipv6: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub state: String,
    pub priority: String,
    pub restart_count: u32,
    pub error: Option<String>,
}

// ── Profile types ───────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProfileData {
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub groups: Vec<String>,
    pub is_admin: bool,
}

// ── Servers types ───────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ServersData {
    pub servers: Vec<ServerEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub mac: Option<String>,
    pub interface: Option<String>,
    pub groups: Vec<String>,
}

// ── Adblock page types ──────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AdblockPageData {
    pub enabled: bool,
    pub domain_count: usize,
    pub source_count: usize,
    pub whitelist_count: usize,
    pub sources: Vec<AdblockSource>,
    pub whitelist: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AdblockSource {
    pub name: String,
    pub url: String,
}

// ── Adblock search result ──────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AdblockSearchResult {
    pub query: String,
    pub is_blocked: bool,
    pub results: Vec<String>,
}

// ── DNS/DHCP page types ─────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DnsDhcpData {
    pub dns: DnsConfigInfo,
    pub dhcp: DhcpConfigInfo,
    pub ipv6: Ipv6ConfigInfo,
    pub leases: Vec<LeaseInfo>,
    pub static_records: Vec<DnsRecord>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DnsConfigInfo {
    pub upstream_servers: Vec<String>,
    pub cache_size: usize,
    pub wildcard_domain: String,
    pub wildcard_ipv4: String,
    pub wildcard_ipv6: String,
    pub adblock_enabled: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DnsRecord {
    pub name: String,
    pub record_type: String,
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DhcpConfigInfo {
    pub enabled: bool,
    pub interface: String,
    pub range_start: String,
    pub range_end: String,
    pub lease_time_secs: u64,
    pub gateway: String,
    pub dns_server: String,
    pub domain: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Ipv6ConfigInfo {
    pub ra_enabled: bool,
    pub dhcpv6_range: String,
    pub dns_servers: Vec<String>,
}

// ── DDNS page types ─────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DdnsPageData {
    pub configured: bool,
    pub record_name: Option<String>,
    pub current_ipv6: Option<String>,
    pub zone_id_masked: Option<String>,
    pub proxied: bool,
    pub interface: String,
    pub cloudflare_ip: Option<String>,
    pub in_sync: bool,
    pub logs: Vec<String>,
}

// ── Certificates page types ─────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CertificatesData {
    pub initialized: bool,
    pub provider: String,
    pub base_domain: String,
    pub certificates: Vec<CertEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CertEntry {
    pub id: String,
    pub cert_type: String,
    pub domains: Vec<String>,
    pub issued_at: String,
    pub expires_at: String,
    pub days_until_expiry: i64,
    pub needs_renewal: bool,
    pub expired: bool,
}

// ── Firewall page types ─────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FirewallData {
    pub available: bool,
    pub enabled: bool,
    pub lan_interface: String,
    pub wan_interface: String,
    pub default_policy: String,
    pub lan_prefix: Option<String>,
    pub rules: Vec<FirewallRuleInfo>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FirewallRuleInfo {
    pub id: String,
    pub description: String,
    pub protocol: String,
    pub dest_port: u16,
    pub dest_port_end: u16,
    pub dest_address: String,
    pub source_address: String,
    pub enabled: bool,
}

// ── Network page types ──────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NetworkData {
    pub interfaces: Vec<NetworkIface>,
    pub ipv4_routes: Vec<RouteEntry>,
    pub ipv6_routes: Vec<RouteEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NetworkIface {
    pub name: String,
    pub state: String,
    pub mac: String,
    pub mtu: Option<u64>,
    pub addresses: Vec<AddrInfo>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AddrInfo {
    pub address: String,
    pub family: String,
    pub prefixlen: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RouteEntry {
    pub destination: String,
    pub gateway: Option<String>,
    pub device: String,
    pub metric: Option<u64>,
}

// ── ReverseProxy page types ─────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReverseProxyPageData {
    pub base_domain: String,
    pub hosts: Vec<ProxyHost>,
    pub proxy_running: bool,
    pub active_routes: usize,
    pub local_networks: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProxyHost {
    pub id: String,
    pub subdomain: Option<String>,
    pub custom_domain: Option<String>,
    pub target_host: String,
    pub target_port: u16,
    pub enabled: bool,
    pub local_only: bool,
    pub require_auth: bool,
}

// ── Applications page types ────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApplicationsPageData {
    pub applications: Vec<AppEntry>,
    pub base_domain: String,
    pub connected_count: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub container_name: String,
    pub enabled: bool,
    pub status: String,
    pub ipv6_address: String,
    pub code_server_enabled: bool,
    pub frontend_port: u16,
    pub frontend_auth_required: bool,
    pub frontend_local_only: bool,
    pub api_count: usize,
    // Live metrics (populated from agent, updated via WebSocket)
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub code_server_status: String,
    pub app_service_status: String,
    pub db_service_status: String,
}

// ── Traffic page types ─────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrafficPageData {
    pub total_requests: u64,
    pub total_bytes: u64,
    pub unique_devices: u64,
    pub top_domains: Vec<TopDomain>,
    pub dns_categories: Vec<DnsCategory>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TopDomain {
    pub domain: String,
    pub total_queries: u64,
    pub category: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DnsCategory {
    pub category: String,
    pub total_queries: u64,
}

// ── Users page types ───────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsersPageData {
    pub users: Vec<UserEntry>,
    pub groups: Vec<GroupEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UserEntry {
    pub username: String,
    pub displayname: String,
    pub email: String,
    pub groups: Vec<String>,
    pub disabled: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GroupEntry {
    pub id: String,
    pub name: String,
    pub built_in: bool,
    pub member_count: usize,
}

// ── Updates page types ─────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UpdatesPageData {
    pub last_check: Option<String>,
    pub apt_packages: Vec<AptPackage>,
    pub snap_packages: Vec<SnapPackage>,
    pub security_count: usize,
    pub kernel_reboot_needed: bool,
    pub services_to_restart: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AptPackage {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
    pub is_security: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SnapPackage {
    pub name: String,
    pub new_version: String,
    pub revision: String,
    pub publisher: String,
}

// ── Energy page types ──────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EnergyPageData {
    pub cpu_model: String,
    pub temperature: Option<f64>,
    pub frequency_current: Option<f64>,
    pub frequency_min: Option<f64>,
    pub frequency_max: Option<f64>,
    pub cpu_usage: Option<f64>,
    pub current_mode: String,
    pub schedule_enabled: bool,
    pub schedule_night_start: String,
    pub schedule_night_end: String,
    pub auto_select_enabled: bool,
    pub auto_select_interface: Option<String>,
}

// ── Settings page types ────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SettingsPageData {
    pub base_domain: String,
    pub api_port: u16,
    pub data_dir: String,
    pub acme_email: Option<String>,
    pub acme_staging: bool,
    pub ddns_cron: String,
}

// ── WoL page types ──────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WolData {
    pub servers: Vec<WolServer>,
    pub schedules: Vec<WolSchedule>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WolServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub mac: Option<String>,
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WolSchedule {
    pub id: String,
    pub server_id: String,
    pub server_name: String,
    pub action: String,
    pub cron: String,
    pub description: String,
    pub enabled: bool,
    pub last_run: Option<String>,
}
