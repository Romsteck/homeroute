//! Agent registry: manages application lifecycle, agent connections,
//! and orchestrates DNS/firewall/CA/Cloudflare updates.

use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use hr_ca::CertificateAuthority;
use hr_common::config::EnvConfig;
use hr_common::events::EventBus;
use hr_firewall::config::FirewallRule;
use hr_firewall::FirewallEngine;
use hr_lxd::LxdClient;

use crate::cloudflare;
use crate::protocol::{AgentRoute, RegistryMessage};
use crate::types::*;

/// An active agent connection (in-memory only).
struct AgentConnection {
    tx: mpsc::Sender<RegistryMessage>,
    connected_at: DateTime<Utc>,
    last_heartbeat: DateTime<Utc>,
}

pub struct AgentRegistry {
    state: Arc<RwLock<RegistryState>>,
    state_path: PathBuf,
    connections: Arc<RwLock<HashMap<String, AgentConnection>>>,
    ca: Arc<CertificateAuthority>,
    firewall: Option<Arc<FirewallEngine>>,
    env: Arc<EnvConfig>,
    events: Arc<EventBus>,
}

impl AgentRegistry {
    /// Load or create the registry state from disk.
    pub fn new(
        state_path: PathBuf,
        ca: Arc<CertificateAuthority>,
        firewall: Option<Arc<FirewallEngine>>,
        env: Arc<EnvConfig>,
        events: Arc<EventBus>,
    ) -> Self {
        let state = match std::fs::read_to_string(&state_path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_else(|e| {
                    warn!("Failed to parse registry state, starting fresh: {e}");
                    RegistryState::default()
                })
            }
            Err(_) => RegistryState::default(),
        };

        info!(
            apps = state.applications.len(),
            "Loaded agent registry state"
        );

        Self {
            state: Arc::new(RwLock::new(state)),
            state_path,
            connections: Arc::new(RwLock::new(HashMap::new())),
            ca,
            firewall,
            env,
            events,
        }
    }

    // ── Application CRUD ────────────────────────────────────────

    /// Create a new application: generates token, creates LXC, deploys agent.
    /// Returns the application and the cleartext token (shown once to the user).
    pub async fn create_application(
        &self,
        req: CreateApplicationRequest,
    ) -> Result<(Application, String)> {
        // Generate token
        let token_clear = generate_token();
        let token_hash = hash_token(&token_clear)?;

        let id = uuid::Uuid::new_v4().to_string();
        let container_name = format!("hr-{}", req.slug);

        let suffix = {
            let mut state = self.state.write().await;
            let s = state.next_suffix;
            state.next_suffix += 1;
            s
        };

        let app = Application {
            id: id.clone(),
            name: req.name,
            slug: req.slug,
            enabled: true,
            container_name: container_name.clone(),
            token_hash,
            ipv6_suffix: suffix,
            ipv6_address: None,
            status: AgentStatus::Pending,
            last_heartbeat: None,
            agent_version: None,
            created_at: Utc::now(),
            frontend: req.frontend,
            apis: req.apis,
            cert_ids: vec![],
            cloudflare_record_ids: vec![],
        };

        // Create the LXC container
        LxdClient::create_container(&container_name)
            .await
            .with_context(|| format!("Failed to create LXC container {container_name}"))?;

        // Deploy hr-agent into the container
        if let Err(e) = self.deploy_agent(&container_name, &app.slug, &token_clear).await {
            // Cleanup on failure
            warn!("Agent deploy failed, deleting container: {e}");
            let _ = LxdClient::delete_container(&container_name).await;
            return Err(e);
        }

        // Store in state
        {
            let mut state = self.state.write().await;
            state.applications.push(app.clone());
        }
        self.persist().await?;

        info!(app = app.slug, container = container_name, "Application created");
        Ok((app, token_clear))
    }

    /// Update application endpoints/auth. Pushes new config to connected agent.
    pub async fn update_application(&self, id: &str, req: UpdateApplicationRequest) -> Result<Option<Application>> {
        let mut state = self.state.write().await;
        let Some(app) = state.applications.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };

        if let Some(name) = req.name {
            app.name = name;
        }
        if let Some(frontend) = req.frontend {
            app.frontend = frontend;
        }
        if let Some(apis) = req.apis {
            app.apis = apis;
        }

        let app = app.clone();
        drop(state);

        self.persist().await?;

        // Push new config to connected agent if any
        self.push_config_to_agent(&app).await;

        Ok(Some(app))
    }

    /// Remove an application: delete LXC, firewall rules, CF records, certs.
    pub async fn remove_application(&self, id: &str) -> Result<bool> {
        let app = {
            let mut state = self.state.write().await;
            let idx = state.applications.iter().position(|a| a.id == id);
            match idx {
                Some(i) => state.applications.remove(i),
                None => return Ok(false),
            }
        };

        // Send shutdown to agent if connected
        {
            let conns = self.connections.read().await;
            if let Some(conn) = conns.get(&app.id) {
                let _ = conn.tx.send(RegistryMessage::Shutdown).await;
            }
        }

        // Delete Cloudflare records
        if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
            for record_id in &app.cloudflare_record_ids {
                if let Err(e) = cloudflare::delete_record(token, zone_id, record_id).await {
                    warn!(record_id, "Failed to delete CF record: {e}");
                }
            }
        }

        // Remove firewall rule
        if let Some(fw) = &self.firewall {
            let rule_id = format!("agent-{}", app.id);
            let _ = fw.remove_rule(&rule_id).await;
        }

        // Delete LXC container
        if let Err(e) = LxdClient::delete_container(&app.container_name).await {
            warn!(container = app.container_name, "Failed to delete container: {e}");
        }

        self.persist().await?;
        info!(app = app.slug, "Application removed");
        Ok(true)
    }

    pub async fn list_applications(&self) -> Vec<Application> {
        self.state.read().await.applications.clone()
    }

    pub async fn toggle_application(&self, id: &str) -> Result<Option<bool>> {
        let mut state = self.state.write().await;
        let Some(app) = state.applications.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };
        app.enabled = !app.enabled;
        let enabled = app.enabled;
        drop(state);
        self.persist().await?;
        Ok(Some(enabled))
    }

    /// Regenerate the token for an application. Returns the new cleartext token.
    pub async fn regenerate_token(&self, id: &str) -> Result<Option<String>> {
        let token_clear = generate_token();
        let token_hash = hash_token(&token_clear)?;

        let mut state = self.state.write().await;
        let Some(app) = state.applications.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };
        app.token_hash = token_hash;
        drop(state);

        self.persist().await?;
        info!(app_id = id, "Token regenerated");
        Ok(Some(token_clear))
    }

    // ── Agent connection lifecycle ──────────────────────────────

    /// Authenticate an agent by token and service name.
    pub async fn authenticate(&self, token: &str, service_name: &str) -> Option<String> {
        let state = self.state.read().await;
        for app in &state.applications {
            if app.slug == service_name && verify_token(token, &app.token_hash) {
                return Some(app.id.clone());
            }
        }
        None
    }

    /// Called when an agent successfully connects and authenticates.
    /// Issues certs, creates DNS records, adds firewall rule, pushes config.
    pub async fn on_agent_connected(
        &self,
        app_id: &str,
        tx: mpsc::Sender<RegistryMessage>,
        agent_version: String,
    ) -> Result<()> {
        let now = Utc::now();

        // Store connection
        {
            let mut conns = self.connections.write().await;
            conns.insert(
                app_id.to_string(),
                AgentConnection {
                    tx: tx.clone(),
                    connected_at: now,
                    last_heartbeat: now,
                },
            );
        }

        // Update status
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.status = AgentStatus::Connected;
                app.agent_version = Some(agent_version);
                app.last_heartbeat = Some(now);
            }
        }

        // Issue certs, DNS, firewall, then push config
        if let Err(e) = self.provision_and_push(app_id, &tx).await {
            error!(app_id, "Failed to provision agent: {e}");
            let _ = tx
                .send(RegistryMessage::AuthResult {
                    success: false,
                    error: Some(format!("Provisioning failed: {e}")),
                })
                .await;
            return Err(e);
        }

        self.persist().await?;
        Ok(())
    }

    /// Called when an agent WebSocket disconnects.
    pub async fn on_agent_disconnected(&self, app_id: &str) {
        {
            let mut conns = self.connections.write().await;
            conns.remove(app_id);
        }
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.status = AgentStatus::Disconnected;
            }
        }
        let _ = self.persist().await;
        info!(app_id, "Agent disconnected");
    }

    /// Update heartbeat timestamp for an agent.
    pub async fn handle_heartbeat(&self, app_id: &str) {
        let now = Utc::now();
        {
            let mut conns = self.connections.write().await;
            if let Some(conn) = conns.get_mut(app_id) {
                conn.last_heartbeat = now;
            }
        }
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.last_heartbeat = Some(now);
            }
        }
    }

    /// Background task: check heartbeats and mark stale agents as disconnected.
    pub async fn run_heartbeat_monitor(self: &Arc<Self>) {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            let now = Utc::now();
            let stale_threshold = chrono::Duration::seconds(90);
            let mut stale_ids = Vec::new();

            {
                let conns = self.connections.read().await;
                for (id, conn) in conns.iter() {
                    if now - conn.last_heartbeat > stale_threshold {
                        stale_ids.push(id.clone());
                    }
                }
            }

            for id in stale_ids {
                warn!(app_id = id, "Agent heartbeat stale, marking disconnected");
                self.on_agent_disconnected(&id).await;
            }
        }
    }

    // ── Certificate renewal ───────────────────────────────────────

    /// Background task: check for certificates needing renewal and push updates.
    pub async fn run_cert_renewal(self: &Arc<Self>) {
        // Check every 6 hours
        let interval = std::time::Duration::from_secs(6 * 3600);
        loop {
            tokio::time::sleep(interval).await;

            info!("Checking agent certificates for renewal...");

            let certs = match self.ca.certificates_needing_renewal() {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to check cert renewal: {e}");
                    continue;
                }
            };

            if certs.is_empty() {
                continue;
            }

            // Find which apps have certs that need renewal
            let state = self.state.read().await;
            let mut apps_to_update = Vec::new();

            for cert in &certs {
                for app in &state.applications {
                    if app.cert_ids.contains(&cert.id) && !apps_to_update.contains(&app.id) {
                        apps_to_update.push(app.id.clone());
                    }
                }
                // Renew the cert
                if let Err(e) = self.ca.renew_certificate(&cert.id).await {
                    warn!(cert_id = cert.id, "Failed to renew cert: {e}");
                }
            }
            drop(state);

            info!(
                renewed = certs.len(),
                apps = apps_to_update.len(),
                "Certificates renewed"
            );

            // Push updated config to affected agents
            for app_id in apps_to_update {
                let state = self.state.read().await;
                if let Some(app) = state.applications.iter().find(|a| a.id == app_id) {
                    let app = app.clone();
                    drop(state);
                    self.push_config_to_agent(&app).await;
                }
            }
        }
    }

    // ── Prefix change handling ──────────────────────────────────

    /// Called when the delegated IPv6 prefix changes. Recalculates all agent
    /// addresses, updates Cloudflare and firewall, pushes updates to agents.
    pub async fn on_prefix_changed(&self, prefix_str: Option<String>) {
        let Some(prefix_str) = prefix_str else {
            // Prefix withdrawn — clear all agent addresses
            let mut state = self.state.write().await;
            for app in &mut state.applications {
                app.ipv6_address = None;
            }
            let _ = self.persist_inner(&state).await;
            return;
        };

        info!(prefix = prefix_str, "Prefix changed, updating all agents");

        let mut state = self.state.write().await;
        for app in &mut state.applications {
            let new_addr = compute_ipv6(&prefix_str, app.ipv6_suffix);
            let old_addr = app.ipv6_address;
            app.ipv6_address = Some(new_addr);

            // Update Cloudflare records
            if old_addr != Some(new_addr) {
                if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id)
                {
                    let base_domain = &self.env.base_domain;
                    let domains = app.domains(base_domain);
                    let ipv6_str = new_addr.to_string();

                    let mut new_record_ids = Vec::new();
                    for domain in &domains {
                        match cloudflare::upsert_aaaa_record(token, zone_id, domain, &ipv6_str, false).await {
                            Ok(rid) => new_record_ids.push(rid),
                            Err(e) => warn!(domain, "CF upsert failed: {e}"),
                        }
                    }
                    app.cloudflare_record_ids = new_record_ids;
                }

                // Update firewall rule
                if let Some(fw) = &self.firewall {
                    let rule_id = format!("agent-{}", app.id);
                    let _ = fw.remove_rule(&rule_id).await;
                    let _ = fw
                        .add_rule(FirewallRule {
                            id: rule_id,
                            description: format!("Agent: {}", app.slug),
                            protocol: "tcp".into(),
                            dest_port: 443,
                            dest_port_end: 0,
                            dest_address: format!("{}/128", new_addr),
                            source_address: String::new(),
                            enabled: true,
                        })
                        .await;
                }

                // Push to connected agent
                let conns = self.connections.read().await;
                if let Some(conn) = conns.get(&app.id) {
                    let _ = conn
                        .tx
                        .send(RegistryMessage::Ipv6Update {
                            ipv6_address: new_addr.to_string(),
                        })
                        .await;
                }
            }
        }

        let _ = self.persist_inner(&state).await;
    }

    // ── Internal helpers ────────────────────────────────────────

    /// Deploy the hr-agent binary and config into an LXC container.
    async fn deploy_agent(
        &self,
        container: &str,
        service_name: &str,
        token: &str,
    ) -> Result<()> {
        let agent_binary = PathBuf::from("/opt/homeroute/data/agent-binaries/hr-agent");
        if !agent_binary.exists() {
            anyhow::bail!(
                "Agent binary not found at {}. Build it first with: cargo build --release -p hr-agent",
                agent_binary.display()
            );
        }

        // Push binary
        LxdClient::push_file(container, &agent_binary, "usr/local/bin/hr-agent").await?;
        LxdClient::exec(container, &["chmod", "+x", "/usr/local/bin/hr-agent"]).await?;

        // Generate config TOML
        let api_port = self.env.api_port;
        let config_content = format!(
            r#"homeroute_address = "fd00:cafe::1"
homeroute_port = {api_port}
token = "{token}"
service_name = "{service_name}"
interface = "eth0"
"#
        );

        let tmp_config = PathBuf::from(format!("/tmp/hr-agent-{service_name}.toml"));
        tokio::fs::write(&tmp_config, &config_content).await?;
        LxdClient::push_file(container, &tmp_config, "etc/hr-agent.toml").await?;
        let _ = tokio::fs::remove_file(&tmp_config).await;

        // Push systemd unit
        let unit_content = r#"[Unit]
Description=HomeRoute Agent
After=network.target

[Service]
ExecStart=/usr/local/bin/hr-agent
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#;
        let tmp_unit = PathBuf::from(format!("/tmp/hr-agent-{service_name}.service"));
        tokio::fs::write(&tmp_unit, unit_content).await?;
        LxdClient::push_file(container, &tmp_unit, "etc/systemd/system/hr-agent.service").await?;
        let _ = tokio::fs::remove_file(&tmp_unit).await;

        // Enable and start
        LxdClient::exec(container, &["systemctl", "daemon-reload"]).await?;
        LxdClient::exec(container, &["systemctl", "enable", "--now", "hr-agent"]).await?;

        info!(container, "Agent deployed");
        Ok(())
    }

    /// Issue certs, create DNS records, add firewall rule, push full config.
    async fn provision_and_push(
        &self,
        app_id: &str,
        tx: &mpsc::Sender<RegistryMessage>,
    ) -> Result<()> {
        let base_domain = self.env.base_domain.clone();
        let mut state = self.state.write().await;
        let app = state
            .applications
            .iter_mut()
            .find(|a| a.id == app_id)
            .ok_or_else(|| anyhow::anyhow!("App not found: {app_id}"))?;

        // Compute IPv6 if we don't have one yet
        // For now we'll skip if no prefix is available — the agent will get Ipv6Update later
        // In a real deployment, we'd read from the prefix watch channel

        // Issue TLS certificates (one per domain)
        let domains = app.domains(&base_domain);
        let routes = app.routes(&base_domain);
        let mut agent_routes = Vec::new();
        let mut cert_ids = Vec::new();

        for route_info in &routes {
            let cert_info = self
                .ca
                .issue_certificate(vec![route_info.domain.clone()])
                .await
                .map_err(|e| anyhow::anyhow!("CA cert issue failed: {e}"))?;

            let cert_pem = tokio::fs::read_to_string(&cert_info.cert_path)
                .await
                .context("read cert PEM")?;
            let key_pem = tokio::fs::read_to_string(&cert_info.key_path)
                .await
                .context("read key PEM")?;

            cert_ids.push(cert_info.id.clone());

            agent_routes.push(AgentRoute {
                domain: route_info.domain.clone(),
                target_port: route_info.target_port,
                cert_pem,
                key_pem,
                auth_required: route_info.auth_required,
                allowed_groups: route_info.allowed_groups.clone(),
            });
        }
        app.cert_ids = cert_ids;

        // Create Cloudflare AAAA records
        let ipv6_str = app
            .ipv6_address
            .map(|a| a.to_string())
            .unwrap_or_default();

        if !ipv6_str.is_empty() {
            if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
                let mut record_ids = Vec::new();
                for domain in &domains {
                    match cloudflare::upsert_aaaa_record(token, zone_id, domain, &ipv6_str, false)
                        .await
                    {
                        Ok(rid) => record_ids.push(rid),
                        Err(e) => warn!(domain, "CF upsert failed (non-fatal): {e}"),
                    }
                }
                app.cloudflare_record_ids = record_ids;
            }

            // Add firewall rule
            if let Some(fw) = &self.firewall {
                let rule_id = format!("agent-{}", app.id);
                // Remove old rule if exists
                let _ = fw.remove_rule(&rule_id).await;
                fw.add_rule(FirewallRule {
                    id: rule_id,
                    description: format!("Agent: {}", app.slug),
                    protocol: "tcp".into(),
                    dest_port: 443,
                    dest_port_end: 0,
                    dest_address: format!("{}/128", ipv6_str),
                    source_address: String::new(),
                    enabled: true,
                })
                .await?;
            }
        }

        // Read CA root cert for the agent
        let ca_pem_path = format!("{}/root-ca.crt", self.env.ca_storage_path.display());
        let ca_pem = tokio::fs::read_to_string(&ca_pem_path)
            .await
            .unwrap_or_default();

        let auth_url = format!("http://[fd00:cafe::1]:{}/api/auth/forward-check", self.env.api_port);

        // Push full config
        tx.send(RegistryMessage::Config {
            config_version: Utc::now().timestamp() as u64,
            ipv6_address: ipv6_str,
            routes: agent_routes,
            ca_pem,
            homeroute_auth_url: auth_url,
        })
        .await
        .map_err(|_| anyhow::anyhow!("Failed to send config to agent"))?;

        Ok(())
    }

    /// Push updated config to a connected agent (after endpoint changes).
    async fn push_config_to_agent(&self, app: &Application) {
        let conns = self.connections.read().await;
        let Some(conn) = conns.get(&app.id) else {
            return;
        };

        let base_domain = &self.env.base_domain;
        let routes = app.routes(base_domain);
        let mut agent_routes = Vec::new();

        for route_info in &routes {
            // Re-read cert PEM for this domain
            let cert_info = match self
                .ca
                .issue_certificate(vec![route_info.domain.clone()])
                .await
            {
                Ok(ci) => ci,
                Err(e) => {
                    warn!(domain = route_info.domain, "Failed to issue cert: {e}");
                    continue;
                }
            };

            let cert_pem = tokio::fs::read_to_string(&cert_info.cert_path)
                .await
                .unwrap_or_default();
            let key_pem = tokio::fs::read_to_string(&cert_info.key_path)
                .await
                .unwrap_or_default();

            agent_routes.push(AgentRoute {
                domain: route_info.domain.clone(),
                target_port: route_info.target_port,
                cert_pem,
                key_pem,
                auth_required: route_info.auth_required,
                allowed_groups: route_info.allowed_groups.clone(),
            });
        }

        let ca_pem_path = format!("{}/root-ca.crt", self.env.ca_storage_path.display());
        let ca_pem = tokio::fs::read_to_string(&ca_pem_path)
            .await
            .unwrap_or_default();

        let auth_url = format!(
            "http://[fd00:cafe::1]:{}/api/auth/forward-check",
            self.env.api_port
        );

        let _ = conn
            .tx
            .send(RegistryMessage::Config {
                config_version: Utc::now().timestamp() as u64,
                ipv6_address: app
                    .ipv6_address
                    .map(|a| a.to_string())
                    .unwrap_or_default(),
                routes: agent_routes,
                ca_pem,
                homeroute_auth_url: auth_url,
            })
            .await;
    }

    /// Persist state to disk (atomic write).
    async fn persist(&self) -> Result<()> {
        let state = self.state.read().await;
        self.persist_inner(&state).await
    }

    async fn persist_inner(&self, state: &RegistryState) -> Result<()> {
        let json = serde_json::to_string_pretty(state)?;
        let tmp = self.state_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &json).await?;
        tokio::fs::rename(&tmp, &self.state_path).await?;
        Ok(())
    }
}

// ── Token helpers ───────────────────────────────────────────────

fn generate_token() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    hex::encode(bytes)
}

fn hash_token(token: &str) -> Result<String> {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::SaltString;
    use rand_core::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash failed: {e}"))?;
    Ok(hash.to_string())
}

fn verify_token(token: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordVerifier};
    use argon2::password_hash::PasswordHash;

    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(token.as_bytes(), &parsed)
        .is_ok()
}

// ── IPv6 helpers ────────────────────────────────────────────────

/// Combine a /64 prefix string with a host suffix to form a full IPv6 address.
fn compute_ipv6(prefix_str: &str, suffix: u16) -> Ipv6Addr {
    // Parse prefix — strip any /NN prefix length
    let prefix_clean = prefix_str.split('/').next().unwrap_or(prefix_str);

    // Parse as Ipv6Addr, fall back to unspecified
    let base: Ipv6Addr = prefix_clean.parse().unwrap_or(Ipv6Addr::UNSPECIFIED);
    let segments = base.segments();

    // Keep the first 4 segments (network /64) and set the host part
    Ipv6Addr::new(
        segments[0],
        segments[1],
        segments[2],
        segments[3],
        0,
        0,
        0,
        suffix,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_ipv6() {
        let addr = compute_ipv6("2a0d:3341:b5b1:7500::1/64", 5);
        assert_eq!(addr, "2a0d:3341:b5b1:7500::5".parse::<Ipv6Addr>().unwrap());
    }

    #[test]
    fn test_compute_ipv6_no_prefix_len() {
        let addr = compute_ipv6("2001:db8:abcd:1234::", 42);
        assert_eq!(
            addr,
            "2001:db8:abcd:1234::2a".parse::<Ipv6Addr>().unwrap()
        );
    }

    #[test]
    fn test_token_roundtrip() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        let hash = hash_token(&token).unwrap();
        assert!(verify_token(&token, &hash));
        assert!(!verify_token("wrong", &hash));
    }
}
