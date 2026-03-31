use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;
use tracing::debug;
use serde::{Serialize, Deserialize};

/// Port where the env-agent internal proxy listens inside each container.
const ENV_PROXY_PORT: u16 = 80;

/// Cached environment routing: one wildcard route per environment.
/// `*.{env_slug}.{base_domain}` → `container_ip:80` (env-agent internal proxy).
pub struct EnvRouteCache {
    routes: RwLock<HashMap<String, EnvEntry>>,
}

/// One entry per connected environment.
struct EnvEntry {
    container_ip: IpAddr,
    code_server_port: u16,
    /// "dev", "acc", or "prod" — used for auth decisions.
    env_type: String,
}

/// Data used to update the cache (from IPC ListEnvironments response).
#[derive(Debug, Deserialize)]
pub struct EnvRouteSummary {
    pub slug: String,
    pub ipv4_address: Option<String>,
    pub agent_connected: bool,
    /// Environment type as stored in EnvironmentRecord (e.g., "development", "production").
    #[serde(default)]
    pub env_type: Option<String>,
    #[serde(default)]
    pub apps: Vec<EnvAppSummary>,
}

#[derive(Debug, Deserialize)]
pub struct EnvAppSummary {
    pub slug: String,
    pub port: u16,
    pub running: bool,
}

/// A system route entry for the API (one wildcard per env).
#[derive(Debug, Serialize)]
pub struct SystemRoute {
    pub domain: String,
    pub target: String,
    pub environment: String,
    pub status: String,
    pub route_type: String,
    pub apps_count: usize,
}

impl EnvRouteCache {
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
        }
    }

    /// Resolve a wildcard env route: `{sub}.{env_slug}.{base_domain}`.
    /// - studio/code subdomains → container_ip:8443 (direct to code-server, supports WS)
    /// - all other subdomains → container_ip:80 (env-agent internal proxy)
    /// Returns (ip, port, env_type) where env_type is "dev", "acc", or "prod".
    pub fn resolve_env(&self, sub: &str, env_slug: &str) -> Option<(IpAddr, u16, String)> {
        let routes = self.routes.read().ok()?;
        let entry = routes.get(env_slug)?;
        let port = if sub == "studio" || sub == "code" {
            entry.code_server_port
        } else {
            ENV_PROXY_PORT
        };
        Some((entry.container_ip, port, entry.env_type.clone()))
    }

    /// Update the cache from environment data.
    pub fn update(&self, envs: Vec<EnvRouteSummary>) {
        let mut new_routes = HashMap::new();

        for env in envs {
            if !env.agent_connected {
                continue;
            }
            let ip = match env.ipv4_address.as_deref().and_then(|s| s.parse::<IpAddr>().ok()) {
                Some(ip) => ip,
                None => continue,
            };

            // Normalize env_type from serde format ("development"→"dev", etc.)
            let env_type = match env.env_type.as_deref() {
                Some("development") => "dev".to_string(),
                Some("acceptance") => "acc".to_string(),
                Some("production") => "prod".to_string(),
                Some(other) => other.to_string(),
                None => "dev".to_string(), // safe default: require auth
            };
            new_routes.insert(env.slug.clone(), EnvEntry { container_ip: ip, code_server_port: 8443, env_type });
        }

        debug!(envs = new_routes.len(), "env route cache updated");

        let mut routes = self.routes.write().unwrap();
        *routes = new_routes;
    }

    /// Get a snapshot of all wildcard routes for the API.
    pub fn snapshot(&self, base_domain: &str, envs: &[EnvRouteSummary]) -> Vec<SystemRoute> {
        let routes = self.routes.read().unwrap();
        let mut result = Vec::new();

        for (env_slug, entry) in routes.iter() {
            let apps_count = envs.iter()
                .find(|e| &e.slug == env_slug)
                .map(|e| e.apps.len())
                .unwrap_or(0);

            result.push(SystemRoute {
                domain: format!("*.{}.{}", env_slug, base_domain),
                target: format!("{}:{}", entry.container_ip, ENV_PROXY_PORT),
                environment: env_slug.clone(),
                status: "running".to_string(),
                route_type: "wildcard".to_string(),
                apps_count,
            });
        }

        result.sort_by(|a, b| a.domain.cmp(&b.domain));
        result
    }
}
