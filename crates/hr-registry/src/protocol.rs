use serde::{Deserialize, Serialize};

// ── Messages from Agent → Registry ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// Initial authentication when connecting.
    #[serde(rename = "auth")]
    Auth {
        token: String,
        service_name: String,
        version: String,
    },
    /// Periodic health report.
    #[serde(rename = "heartbeat")]
    Heartbeat {
        uptime_secs: u64,
        connections_active: u32,
    },
    /// Agent acknowledges a config push.
    #[serde(rename = "config_ack")]
    ConfigAck { config_version: u64 },
    /// Agent reports an error.
    #[serde(rename = "error")]
    Error { message: String },
}

// ── Messages from Registry → Agent ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RegistryMessage {
    /// Response to Auth.
    #[serde(rename = "auth_result")]
    AuthResult {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Full configuration push.
    #[serde(rename = "config")]
    Config {
        config_version: u64,
        ipv6_address: String,
        routes: Vec<AgentRoute>,
        ca_pem: String,
        homeroute_auth_url: String,
    },
    /// Partial update: IPv6 changed (prefix rotation).
    #[serde(rename = "ipv6_update")]
    Ipv6Update { ipv6_address: String },
    /// Partial update: certificates renewed.
    #[serde(rename = "cert_update")]
    CertUpdate { routes: Vec<AgentRoute> },
    /// Agent should self-update.
    #[serde(rename = "update_available")]
    UpdateAvailable {
        version: String,
        download_url: String,
        sha256: String,
    },
    /// Graceful shutdown request.
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// A single route the agent must serve (one per domain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoute {
    pub domain: String,
    pub target_port: u16,
    pub cert_pem: String,
    pub key_pem: String,
    pub auth_required: bool,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_message_serde() {
        let msg = AgentMessage::Auth {
            token: "abc".into(),
            service_name: "test".into(),
            version: "0.1.0".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"auth"#));
        let parsed: AgentMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            AgentMessage::Auth { token, .. } => assert_eq!(token, "abc"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_registry_message_serde() {
        let msg = RegistryMessage::AuthResult {
            success: true,
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: RegistryMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            RegistryMessage::AuthResult { success, .. } => assert!(success),
            _ => panic!("wrong variant"),
        }
    }
}
