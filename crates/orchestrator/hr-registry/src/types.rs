use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

use crate::protocol::AgentMetrics;

/// Port that code-server listens on inside each container.
pub const CODE_SERVER_PORT: u16 = 13337;

/// Application environment.
/// All containers are now production. The Development variant is kept only
/// for backward-compatible deserialization of legacy JSON data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Production,
    /// Legacy — no longer created, treated identically to Production.
    Development,
}

/// Technology stack for a container application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppStack {
    #[default]
    NextJs,
    LeptosRust,
    AxumVite,
}

/// A registered application with its container and agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// Host this application belongs to ("local" for the main server).
    #[serde(default = "default_host_id")]
    pub host_id: String,
    /// Environment: development or production.
    #[serde(default)]
    pub environment: Environment,
    pub enabled: bool,
    pub container_name: String,
    /// Argon2 hash of the agent token.
    pub token_hash: String,
    /// IPv4 address reported by agent (for local DNS A records).
    #[serde(default)]
    pub ipv4_address: Option<Ipv4Addr>,
    pub status: AgentStatus,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub agent_version: Option<String>,
    pub created_at: DateTime<Utc>,

    /// Frontend endpoint configuration.
    pub frontend: FrontendEndpoint,

    /// Whether code-server IDE is enabled for this application.
    #[serde(default = "default_true")]
    pub code_server_enabled: bool,

    /// Stack technologique du container.
    #[serde(default)]
    pub stack: AppStack,

    /// Current metrics from agent (volatile, not persisted to disk).
    #[serde(skip_deserializing)]
    pub metrics: Option<AgentMetrics>,
}

impl Application {
    /// Return all domains this application serves: `{slug}.{base}`.
    pub fn domains(&self, base_domain: &str) -> Vec<String> {
        vec![format!("{}.{}", self.slug, base_domain)]
    }

    /// Return all (domain, port, auth_required, allowed_groups) tuples for agent routing.
    pub fn routes(&self, base_domain: &str) -> Vec<RouteInfo> {
        vec![RouteInfo {
            domain: format!("{}.{}", self.slug, base_domain),
            target_port: self.frontend.target_port,
            auth_required: self.frontend.auth_required,
            allowed_groups: self.frontend.allowed_groups.clone(),
        }]
    }

    /// Return the wildcard domain for this application's per-app certificate.
    /// e.g., `*.{slug}.{base_domain}`
    pub fn wildcard_domain(&self, base_domain: &str) -> String {
        format!("*.{}.{}", self.slug, base_domain)
    }
}

/// Route metadata for proxy registration at startup and agent-driven publishing.
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub domain: String,
    pub target_port: u16,
    pub auth_required: bool,
    pub allowed_groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendEndpoint {
    pub target_port: u16,
    #[serde(default)]
    pub auth_required: bool,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
    #[serde(default)]
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Pending,
    Deploying,
    Connected,
    Disconnected,
    Error,
}

/// Persisted registry state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryState {
    #[serde(default)]
    pub applications: Vec<Application>,
}

fn default_true() -> bool {
    true
}

fn default_host_id() -> String {
    "local".to_string()
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            applications: Vec::new(),
        }
    }
}

/// Request body for creating an application via the API.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateApplicationRequest {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub host_id: Option<String>,
    pub frontend: FrontendEndpoint,
    #[serde(default = "default_true")]
    pub code_server_enabled: bool,
    #[serde(default)]
    pub stack: AppStack,
}

/// Request body for updating an application.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateApplicationRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub frontend: Option<FrontendEndpoint>,
    #[serde(default)]
    pub code_server_enabled: Option<bool>,
    #[serde(default)]
    pub stack: Option<AppStack>,
}

// ── Agent Update Types ──────────────────────────────────────────

/// Request body for triggering agent updates.
#[derive(Debug, Clone, Deserialize)]
pub struct TriggerUpdateRequest {
    /// Specific agent IDs to update (None = all connected agents).
    #[serde(default)]
    pub agent_ids: Option<Vec<String>>,
}

/// Result of notifying a single agent about an update.
#[derive(Debug, Clone, Serialize)]
pub struct AgentNotifyResult {
    pub id: String,
    pub slug: String,
    pub status: String,
}

/// Result of skipping a single agent.
#[derive(Debug, Clone, Serialize)]
pub struct AgentSkipResult {
    pub id: String,
    pub slug: String,
    pub reason: String,
}

/// Result of triggering updates to a batch of agents.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateBatchResult {
    pub version: String,
    pub sha256: String,
    pub agents_notified: Vec<AgentNotifyResult>,
    pub agents_skipped: Vec<AgentSkipResult>,
}

/// Update status for a single agent.
#[derive(Debug, Clone, Serialize)]
pub struct AgentUpdateStatusInfo {
    pub id: String,
    pub slug: String,
    pub container_name: String,
    pub status: String,
    pub current_version: Option<String>,
    pub update_status: String,
    pub metrics_flowing: bool,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

/// Result of checking update status for all agents.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatusResult {
    pub expected_version: String,
    pub agents: Vec<AgentUpdateStatusInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_app(environment: Environment, code_server_enabled: bool) -> Application {
        Application {
            id: "test".into(),
            name: "Test".into(),
            slug: "myapp".into(),
            host_id: "local".into(),
            environment,
            enabled: true,
            container_name: "hr-myapp".into(),
            token_hash: String::new(),
            ipv4_address: None,
            status: AgentStatus::Pending,
            last_heartbeat: None,
            agent_version: None,
            created_at: Utc::now(),
            frontend: FrontendEndpoint {
                target_port: 3000,
                auth_required: false,
                allowed_groups: vec![],
                local_only: false,
            },
            code_server_enabled,
            stack: AppStack::NextJs,
            metrics: None,
        }
    }

    #[test]
    fn test_domains() {
        let app = make_test_app(Environment::Production, true);
        let domains = app.domains("example.com");
        assert_eq!(domains, vec!["myapp.example.com"]);
    }

    #[test]
    fn test_routes_production() {
        let app = make_test_app(Environment::Production, true);
        let routes = app.routes("example.com");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].domain, "myapp.example.com");
    }

    #[test]
    fn test_wildcard_domain() {
        let app = make_test_app(Environment::Production, true);
        assert_eq!(app.wildcard_domain("example.com"), "*.myapp.example.com");
    }

    #[test]
    fn test_serde_roundtrip() {
        let state = RegistryState::default();
        let json = serde_json::to_string(&state).unwrap();
        let parsed: RegistryState = serde_json::from_str(&json).unwrap();
        assert!(parsed.applications.is_empty());
    }
}
