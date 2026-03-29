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
}

/// Data used to update the cache (from IPC ListEnvironments response).
#[derive(Debug, Deserialize)]
pub struct EnvRouteSummary {
    pub slug: String,
    pub ipv4_address: Option<String>,
    pub agent_connected: bool,
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

    /// Resolve a wildcard env route: any `*.{env_slug}.{base_domain}` → container_ip:80.
    /// Returns (container_ip, proxy_port) if the env exists and is connected.
    pub fn resolve_env(&self, env_slug: &str) -> Option<(IpAddr, u16)> {
        let routes = self.routes.read().ok()?;
        let entry = routes.get(env_slug)?;
        Some((entry.container_ip, ENV_PROXY_PORT))
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

            new_routes.insert(env.slug.clone(), EnvEntry { container_ip: ip });
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
