use serde::{Deserialize, Serialize};

use crate::types::AppStackType;

/// Configuration for the env-agent, loaded from /etc/env-agent.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvAgentConfig {
    /// Address of the HomeRoute orchestrator.
    pub homeroute_address: String,
    /// WebSocket port of the orchestrator.
    #[serde(default = "default_orchestrator_port")]
    pub homeroute_port: u16,
    /// Authentication token (64-char hex, verified by orchestrator).
    pub token: String,
    /// Environment slug (e.g., "dev", "prod", "acc").
    pub env_slug: String,
    /// Network interface inside the container.
    #[serde(default = "default_interface")]
    pub interface: String,
    /// Port for the MCP HTTP server.
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
    /// Port for code-server (studio).
    #[serde(default = "default_code_server_port")]
    pub code_server_port: u16,
    /// Base path where apps are stored.
    #[serde(default = "default_apps_path")]
    pub apps_path: String,
    /// Base path where databases are stored.
    #[serde(default = "default_db_path")]
    pub db_path: String,
    /// Apps managed by this env-agent.
    #[serde(default)]
    pub apps: Vec<EnvAgentAppConfig>,
}

/// Per-app configuration within the env-agent config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvAgentAppConfig {
    /// App slug (unique identifier).
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Technology stack.
    #[serde(default)]
    pub stack: AppStackType,
    /// Port the app listens on.
    #[serde(default = "default_app_port")]
    pub port: u16,
    /// Command to run the app.
    pub run_command: String,
    /// Build command (dev envs only).
    #[serde(default)]
    pub build_command: Option<String>,
    /// Health check path.
    #[serde(default = "default_health_path")]
    pub health_path: String,
    /// Whether this app has a Dataverse database.
    #[serde(default)]
    pub has_db: bool,
    /// Watch command for dev environments (rebuild on file changes).
    /// Falls back to `stack.default_watch_command()` if absent.
    #[serde(default)]
    pub watch_command: Option<String>,
    /// Test command.
    #[serde(default)]
    pub test_command: Option<String>,
    /// Brief description of the app (overrides stack-derived structure hint).
    #[serde(default)]
    pub description: Option<String>,
}

impl EnvAgentConfig {
    /// Load configuration from a TOML file.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Derive the environment type from the slug.
    pub fn env_type(&self) -> crate::types::EnvType {
        match self.env_slug.as_str() {
            "prod" | "production" => crate::types::EnvType::Production,
            "acc" | "acceptance" | "staging" => crate::types::EnvType::Acceptance,
            _ => crate::types::EnvType::Development,
        }
    }

    /// WebSocket URL to connect to the orchestrator.
    pub fn orchestrator_ws_url(&self) -> String {
        format!(
            "ws://{}:{}/envs/ws",
            self.homeroute_address, self.homeroute_port
        )
    }
}

fn default_orchestrator_port() -> u16 {
    4001
}

fn default_interface() -> String {
    "host0".to_string()
}

fn default_mcp_port() -> u16 {
    4010
}

fn default_code_server_port() -> u16 {
    8443
}

fn default_apps_path() -> String {
    "/apps".to_string()
}

fn default_db_path() -> String {
    "/opt/env-agent/data/db".to_string()
}

fn default_app_port() -> u16 {
    3000
}

fn default_health_path() -> String {
    "/api/health".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_deserialize() {
        let toml_str = r#"
            homeroute_address = "10.0.0.254"
            homeroute_port = 4001
            token = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
            env_slug = "dev"

            [[apps]]
            slug = "trader"
            name = "Trader"
            stack = "axum-vite"
            port = 3001
            run_command = "./bin/trader"
            has_db = true

            [[apps]]
            slug = "wallet"
            name = "Wallet"
            stack = "axum-vite"
            port = 3002
            run_command = "./bin/wallet"
            build_command = "cargo build --release"
            has_db = true
        "#;

        let config: EnvAgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.env_slug, "dev");
        assert_eq!(config.apps.len(), 2);
        assert_eq!(config.apps[0].slug, "trader");
        assert_eq!(config.apps[0].port, 3001);
        assert_eq!(config.apps[1].build_command, Some("cargo build --release".into()));
        assert_eq!(config.mcp_port, 4010); // default
        assert_eq!(config.db_path, "/opt/env-agent/data/db"); // default
    }

    #[test]
    fn test_ws_url() {
        let config = EnvAgentConfig {
            homeroute_address: "10.0.0.254".into(),
            homeroute_port: 4001,
            token: "test".into(),
            env_slug: "dev".into(),
            interface: "host0".into(),
            mcp_port: 4010,
            code_server_port: 8443,
            apps_path: "/apps".into(),
            db_path: "/opt/env-agent/data/db".into(),
            apps: vec![],
        };
        assert_eq!(config.orchestrator_ws_url(), "ws://10.0.0.254:4001/envs/ws");
    }
}
