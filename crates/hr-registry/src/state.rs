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

use hr_acme::{AcmeManager, WildcardType};
use hr_common::config::EnvConfig;
use hr_common::events::{AgentMetricsEvent, AgentStatusEvent, AgentUpdateEvent, AgentUpdateStatus, EventBus};
use hr_dns::AppDnsStore;
use hr_firewall::config::FirewallRule;
use hr_firewall::FirewallEngine;
use hr_lxd::LxdClient;

use crate::cloudflare;
use crate::protocol::{AgentMetrics, AgentRoute, PowerPolicy, RegistryMessage, ServiceAction, ServiceState, ServiceType};
use crate::types::{
    AgentNotifyResult, AgentSkipResult, AgentStatus, AgentUpdateStatusInfo,
    Application, CreateApplicationRequest, RegistryState, UpdateApplicationRequest,
    UpdateBatchResult, UpdateStatusResult,
};

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
    acme: Arc<AcmeManager>,
    firewall: Option<Arc<FirewallEngine>>,
    env: Arc<EnvConfig>,
    events: Arc<EventBus>,
    /// Shared DNS store for application domains → IPv6 addresses
    app_dns_store: AppDnsStore,
}

impl AgentRegistry {
    /// Load or create the registry state from disk.
    pub fn new(
        state_path: PathBuf,
        acme: Arc<AcmeManager>,
        firewall: Option<Arc<FirewallEngine>>,
        env: Arc<EnvConfig>,
        events: Arc<EventBus>,
        app_dns_store: AppDnsStore,
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
            acme,
            firewall,
            env,
            events,
            app_dns_store,
        }
    }

    // ── Application CRUD ────────────────────────────────────────

    /// Create a new application: generates token, saves record immediately,
    /// then deploys LXC container + agent in a background task.
    /// Returns the application (status=deploying) and the cleartext token.
    pub async fn create_application(
        self: &Arc<Self>,
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
            ipv4_address: None,
            status: AgentStatus::Deploying,
            last_heartbeat: None,
            agent_version: None,
            created_at: Utc::now(),
            frontend: req.frontend,
            apis: req.apis,
            code_server_enabled: req.code_server_enabled,
            services: req.services,
            power_policy: req.power_policy,
            metrics: None,
            cert_ids: vec![],
            cloudflare_record_ids: vec![],
        };

        // Store in state immediately so the UI can see the app
        {
            let mut state = self.state.write().await;
            state.applications.push(app.clone());
        }
        self.persist().await?;

        info!(app = app.slug, container = container_name, "Application created, starting background deploy");

        // Spawn background deploy task
        let registry = Arc::clone(self);
        let token_for_deploy = token_clear.clone();
        let slug = app.slug.clone();
        let app_id = id.clone();
        tokio::spawn(async move {
            registry.run_deploy_background(&app_id, &slug, &container_name, &token_for_deploy).await;
        });

        Ok((app, token_clear))
    }

    /// Background deployment: creates LXC container, deploys agent, emits progress events.
    async fn run_deploy_background(
        &self,
        app_id: &str,
        slug: &str,
        container_name: &str,
        token: &str,
    ) {
        let emit = |message: &str| {
            let _ = self.events.agent_status.send(AgentStatusEvent {
                app_id: app_id.to_string(),
                slug: slug.to_string(),
                status: "deploying".to_string(),
                message: Some(message.to_string()),
            });
        };

        emit("Creation du conteneur LXC...");

        // Create the LXC container
        if let Err(e) = LxdClient::create_container(container_name).await {
            error!(container = container_name, "LXC creation failed: {e}");
            emit(&format!("Erreur: {e}"));
            self.set_app_status(app_id, AgentStatus::Error).await;
            // Remove the app from state on failure
            self.remove_failed_app(app_id).await;
            return;
        }

        // Deploy hr-agent into the container
        if let Err(e) = self.deploy_agent(container_name, slug, token, &emit).await {
            error!(container = container_name, "Agent deploy failed: {e}");
            emit(&format!("Erreur: {e}"));
            self.set_app_status(app_id, AgentStatus::Error).await;
            // Cleanup container on failure
            let _ = LxdClient::delete_container(container_name).await;
            self.remove_failed_app(app_id).await;
            return;
        }

        // Update status to pending only if agent hasn't already connected
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                if app.status == AgentStatus::Deploying {
                    app.status = AgentStatus::Pending;
                }
            }
        }
        let _ = self.persist().await;

        let _ = self.events.agent_status.send(AgentStatusEvent {
            app_id: app_id.to_string(),
            slug: slug.to_string(),
            status: "pending".to_string(),
            message: Some("Deploiement termine".to_string()),
        });

        info!(app = slug, container = container_name, "Background deploy complete");
    }

    /// Set an application's status and persist.
    async fn set_app_status(&self, app_id: &str, status: AgentStatus) {
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.status = status;
            }
        }
        let _ = self.persist().await;
    }

    /// Remove a failed application from state (cleanup after deploy failure).
    async fn remove_failed_app(&self, app_id: &str) {
        {
            let mut state = self.state.write().await;
            state.applications.retain(|a| a.id != app_id);
        }
        let _ = self.persist().await;
    }

    /// Update application endpoints/auth. Pushes new config to connected agent.
    pub async fn update_application(&self, id: &str, req: UpdateApplicationRequest) -> Result<Option<Application>> {
        let base_domain = self.env.base_domain.clone();
        let mut state = self.state.write().await;
        let Some(app) = state.applications.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };

        // Capture old domains before modification
        let old_domains = app.domains(&base_domain);

        if let Some(name) = req.name {
            app.name = name;
        }
        if let Some(frontend) = req.frontend {
            app.frontend = frontend;
        }
        if let Some(apis) = req.apis {
            app.apis = apis;
        }
        if let Some(code_server_enabled) = req.code_server_enabled {
            app.code_server_enabled = code_server_enabled;
        }
        if let Some(services) = req.services {
            app.services = services;
        }
        if let Some(power_policy) = req.power_policy {
            app.power_policy = power_policy;
        }

        // Get new domains after modification
        let new_domains = app.domains(&base_domain);

        let app = app.clone();
        drop(state);

        self.persist().await?;

        // Sync DNS if domains changed
        if old_domains != new_domains {
            // Identify added and removed domains
            let removed: Vec<_> = old_domains
                .iter()
                .filter(|d| !new_domains.contains(d))
                .cloned()
                .collect();
            let added: Vec<_> = new_domains
                .iter()
                .filter(|d| !old_domains.contains(d))
                .cloned()
                .collect();

            // Remove old local DNS records
            if !removed.is_empty() {
                if let Err(e) = self.remove_local_dns_records(&removed) {
                    warn!("Failed to remove old DNS records: {e}");
                }
            }
            // Add/update new local DNS records
            if let Some(ipv6) = app.ipv6_address {
                if let Err(e) = self.sync_local_dns_records(&new_domains, ipv6, app.ipv4_address) {
                    warn!("Failed to sync new DNS records: {e}");
                }
            }

            // Sync Cloudflare DNS records
            if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
                if let Some(ipv6) = app.ipv6_address {
                    let ipv6_str = ipv6.to_string();

                    // Create CF records for added domains
                    for domain in &added {
                        match cloudflare::upsert_aaaa_record(token, zone_id, domain, &ipv6_str, true).await {
                            Ok(rid) => {
                                let mut state = self.state.write().await;
                                if let Some(app) = state.applications.iter_mut().find(|a| a.id == id) {
                                    app.cloudflare_record_ids.push(rid);
                                }
                                drop(state);
                                let _ = self.persist().await;
                            }
                            Err(e) => warn!(domain, "CF upsert failed: {e}"),
                        }
                    }

                    // Delete CF records for removed domains
                    for domain in &removed {
                        match cloudflare::delete_record_by_name(token, zone_id, domain).await {
                            Ok(Some(rid)) => {
                                let mut state = self.state.write().await;
                                if let Some(app) = state.applications.iter_mut().find(|a| a.id == id) {
                                    app.cloudflare_record_ids.retain(|r| r != &rid);
                                }
                                drop(state);
                                let _ = self.persist().await;
                            }
                            Ok(None) => {} // Record didn't exist
                            Err(e) => warn!(domain, "CF delete failed: {e}"),
                        }
                    }
                }
            }
        }

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

        // Remove from DNS store
        self.remove_app_dns(&app).await;

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

        // Remove local DNS records
        let domains = app.domains(&self.env.base_domain);
        if let Err(e) = self.remove_local_dns_records(&domains) {
            warn!("Failed to remove local DNS records: {e}");
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
    /// If the agent reports its actual IPv6/IPv4 addresses, we use those.
    pub async fn on_agent_connected(
        &self,
        app_id: &str,
        tx: mpsc::Sender<RegistryMessage>,
        agent_version: String,
        reported_ipv6: Option<String>,
        reported_ipv4: Option<String>,
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

        // Update status and IP addresses if reported by agent
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.status = AgentStatus::Connected;
                app.agent_version = Some(agent_version);
                app.last_heartbeat = Some(now);

                // Use agent's reported IPv6 if available (more accurate than computed)
                if let Some(ref ipv6_str) = reported_ipv6 {
                    if let Ok(addr) = ipv6_str.parse() {
                        let old_addr = app.ipv6_address;
                        app.ipv6_address = Some(addr);
                        if old_addr != Some(addr) {
                            info!(app_id, ipv6 = ipv6_str, "Updated app IPv6 from agent report");
                        }
                    }
                }

                // Use agent's reported IPv4 if available
                if let Some(ref ipv4_str) = reported_ipv4 {
                    if let Ok(addr) = ipv4_str.parse() {
                        let old_addr = app.ipv4_address;
                        app.ipv4_address = Some(addr);
                        if old_addr != Some(addr) {
                            info!(app_id, ipv4 = ipv4_str, "Updated app IPv4 from agent report");
                        }
                    }
                }
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

        // Update the shared DNS store with app domains → IPv6
        {
            let state = self.state.read().await;
            if let Some(app) = state.applications.iter().find(|a| a.id == app_id) {
                self.update_app_dns(app).await;
            }
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

        // Remove app domains from DNS store and update status
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.status = AgentStatus::Disconnected;
                // Clone app for DNS removal (outside the write lock)
            }
        }
        {
            let state = self.state.read().await;
            if let Some(app) = state.applications.iter().find(|a| a.id == app_id) {
                self.remove_app_dns(app).await;
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

    /// Background task: check for ACME wildcard certificates needing renewal and push updates.
    pub async fn run_cert_renewal(self: &Arc<Self>) {
        // Check every 6 hours
        let interval = std::time::Duration::from_secs(6 * 3600);
        loop {
            tokio::time::sleep(interval).await;

            info!("Checking ACME wildcard certificates for renewal...");

            let certs = match self.acme.certificates_needing_renewal() {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to check cert renewal: {e}");
                    continue;
                }
            };

            if certs.is_empty() {
                info!("No certificates need renewal");
                continue;
            }

            // Renew certificates that need it
            let mut renewed = 0;
            for cert in &certs {
                info!(wildcard = cert.id, expires = %cert.expires_at, "Renewing certificate...");
                if let Err(e) = self.acme.request_wildcard(cert.wildcard_type).await {
                    warn!(wildcard = cert.id, "Failed to renew wildcard cert: {e}");
                } else {
                    renewed += 1;
                    info!(wildcard = cert.id, "Certificate renewed successfully");
                }
            }

            if renewed > 0 {
                info!(renewed, "Certificates renewed, pushing updates to all agents");
                // Push updated certs to all connected agents
                self.push_cert_updates().await;
            }
        }
    }

    /// Push certificate updates to all connected agents.
    /// Called after wildcard certificate renewal.
    pub async fn push_cert_updates(&self) {
        let state = self.state.read().await;
        let apps: Vec<_> = state.applications.clone();
        drop(state);

        for app in apps {
            self.push_config_to_agent(&app).await;
        }
    }

    // ── Prefix change handling ──────────────────────────────────

    /// Called when the delegated IPv6 prefix changes. Recalculates all agent
    /// addresses, updates Cloudflare and firewall, pushes updates to agents.
    pub async fn on_prefix_changed(&self, prefix_str: Option<String>) {
        let Some(prefix_str) = prefix_str else {
            // Prefix withdrawn — clear all agent addresses and DNS store
            let mut state = self.state.write().await;
            for app in &mut state.applications {
                app.ipv6_address = None;
            }
            // Clear DNS store
            self.app_dns_store.write().await.clear();
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
                        match cloudflare::upsert_aaaa_record(token, zone_id, domain, &ipv6_str, true).await {
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

        // Update DNS store for all apps with their new IPv6 addresses
        for app in &state.applications {
            self.update_app_dns(app).await;
        }
    }

    // ── DNS store helpers ────────────────────────────────────────

    /// Update the shared DNS store with an application's domains.
    /// Called when an agent connects or its IPv6 address changes.
    async fn update_app_dns(&self, app: &Application) {
        let Some(ipv6) = app.ipv6_address else {
            return;
        };
        if !app.enabled {
            return;
        }

        let base_domain = &self.env.base_domain;
        let domains = app.domains(base_domain);

        let mut store = self.app_dns_store.write().await;
        for domain in domains {
            store.insert(domain.to_lowercase(), ipv6);
        }
        info!(app = app.slug, ipv6 = %ipv6, "Updated DNS store with app domains");
    }

    /// Remove an application's domains from the shared DNS store.
    /// Called when an agent disconnects or is removed.
    async fn remove_app_dns(&self, app: &Application) {
        let base_domain = &self.env.base_domain;
        let domains = app.domains(base_domain);

        let mut store = self.app_dns_store.write().await;
        for domain in &domains {
            store.remove(&domain.to_lowercase());
        }
        info!(app = app.slug, "Removed app domains from DNS store");
    }

    // ── Local DNS static records (synced with Cloudflare) ────────

    /// Add/update static DNS records (A and AAAA) for an application's domains.
    /// These are stored in dns-dhcp-config.json and visible in the DNS page.
    /// - AAAA records: IPv6 addresses (same as Cloudflare) for IPv6 clients
    /// - A records: IPv4 addresses for IPv4-only clients (local DNS only)
    fn sync_local_dns_records(
        &self,
        domains: &[String],
        ipv6: std::net::Ipv6Addr,
        ipv4: Option<std::net::Ipv4Addr>,
    ) -> Result<()> {
        let config_path = &self.env.dns_dhcp_config_path;
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read DNS config: {:?}", config_path))?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;

        let static_records = config["dns"]["static_records"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Invalid DNS config: missing static_records"))?;

        for domain in domains {
            // Remove existing records with same name (both A and AAAA)
            static_records.retain(|r| r["name"].as_str() != Some(domain.as_str()));

            // Add AAAA record pointing to agent IPv6 (same as Cloudflare)
            static_records.push(serde_json::json!({
                "name": domain,
                "type": "AAAA",
                "value": ipv6.to_string(),
                "ttl": 60
            }));

            // Add A record pointing to agent IPv4 (local DNS only, for v4 clients)
            if let Some(v4) = ipv4 {
                static_records.push(serde_json::json!({
                    "name": domain,
                    "type": "A",
                    "value": v4.to_string(),
                    "ttl": 60
                }));
            }
        }

        std::fs::write(config_path, serde_json::to_string_pretty(&config)?)?;
        info!(domains = ?domains, ipv6 = %ipv6, ipv4 = ?ipv4, "Synced local DNS records (A + AAAA)");
        Ok(())
    }

    /// Remove DNS records for specified domains.
    fn remove_local_dns_records(&self, domains: &[String]) -> Result<()> {
        let config_path = &self.env.dns_dhcp_config_path;
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read DNS config: {:?}", config_path))?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;

        let static_records = config["dns"]["static_records"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Invalid DNS config: missing static_records"))?;

        for domain in domains {
            static_records.retain(|r| r["name"].as_str() != Some(domain.as_str()));
        }

        std::fs::write(config_path, serde_json::to_string_pretty(&config)?)?;
        info!(domains = ?domains, "Removed local DNS records");
        Ok(())
    }

    /// Migrate existing applications' DNS records to static_records.
    /// Called once at startup to populate DNS records for apps that existed before this feature.
    pub async fn migrate_dns_records(&self) -> Result<()> {
        let config_path = &self.env.dns_dhcp_config_path;
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read DNS config: {:?}", config_path))?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;

        let static_records = config["dns"]["static_records"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Invalid DNS config: missing static_records"))?;

        let state = self.state.read().await;
        let base_domain = &self.env.base_domain;
        let mut added = 0;

        for app in &state.applications {
            if !app.enabled {
                continue;
            }
            // Skip apps without IPv6 address
            let Some(ipv6) = app.ipv6_address else {
                continue;
            };

            let domains = app.domains(base_domain);
            for domain in domains {
                // Remove existing records (handles migration from old A-only to A+AAAA)
                static_records.retain(|r| r["name"].as_str() != Some(&domain));

                // Add AAAA record pointing to agent IPv6 (same as Cloudflare)
                static_records.push(serde_json::json!({
                    "name": domain,
                    "type": "AAAA",
                    "value": ipv6.to_string(),
                    "ttl": 60
                }));

                // Add A record pointing to agent IPv4 (local DNS only)
                if let Some(ipv4) = app.ipv4_address {
                    static_records.push(serde_json::json!({
                        "name": domain,
                        "type": "A",
                        "value": ipv4.to_string(),
                        "ttl": 60
                    }));
                }

                added += 1;
            }
        }

        if added > 0 {
            std::fs::write(config_path, serde_json::to_string_pretty(&config)?)?;
            info!(added, "Migrated DNS records for existing applications");
        }

        Ok(())
    }

    // ── Internal helpers ────────────────────────────────────────

    /// Deploy the hr-agent binary and config into an LXC container.
    /// `emit` is called with progress messages for real-time UI updates.
    async fn deploy_agent(
        &self,
        container: &str,
        service_name: &str,
        token: &str,
        emit: impl Fn(&str),
    ) -> Result<()> {
        let agent_binary = PathBuf::from("/opt/homeroute/data/agent-binaries/hr-agent");
        if !agent_binary.exists() {
            anyhow::bail!(
                "Agent binary not found at {}. Build it first with: cargo build --release -p hr-agent",
                agent_binary.display()
            );
        }

        // Push binary
        emit("Deploiement du binaire agent...");
        LxdClient::push_file(container, &agent_binary, "usr/local/bin/hr-agent").await?;
        LxdClient::exec(container, &["chmod", "+x", "/usr/local/bin/hr-agent"]).await?;

        // Generate config TOML
        emit("Configuration de l'agent...");
        let api_port = self.env.api_port;
        let config_content = format!(
            r#"homeroute_address = "10.0.0.254"
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

        // Enable and start agent
        emit("Demarrage de l'agent...");
        LxdClient::exec(container, &["systemctl", "daemon-reload"]).await?;
        LxdClient::exec(container, &["systemctl", "enable", "--now", "hr-agent"]).await?;

        // Wait for network connectivity before installing packages
        emit("Attente de la connectivite reseau...");
        LxdClient::wait_for_network(container, 30).await?;

        // Install code-server dependencies with retry
        emit("Installation des dependances...");
        LxdClient::exec_with_retry(
            container,
            &["bash", "-c", "apt-get update -qq && apt-get install -y -qq curl"],
            3,
        )
        .await
        .with_context(|| format!("Failed to install curl in {container}"))?;

        emit("Installation de code-server...");
        LxdClient::exec_with_retry(
            container,
            &["bash", "-c", "curl -fsSL https://code-server.dev/install.sh | sh -s -- --method=standalone --prefix=/usr/local"],
            3,
        )
        .await
        .with_context(|| format!("Failed to install code-server in {container}"))?;

        // Attach a separate storage volume for the workspace (independent of boot disk)
        emit("Creation du volume workspace...");
        let vol_name = format!("{container}-workspace");
        LxdClient::attach_storage_volume(container, &vol_name, "/root/workspace")
            .await
            .with_context(|| format!("Failed to attach workspace volume for {container}"))?;

        // Configure code-server: no auth (forward-auth handles it), bind localhost
        emit("Configuration de code-server...");
        LxdClient::exec(container, &["mkdir", "-p", "/root/.config/code-server"]).await?;
        let cs_config = "bind-addr: 127.0.0.1:13337\nauth: none\ncert: false\n";
        let tmp_cs_config = PathBuf::from(format!("/tmp/cs-config-{service_name}.yaml"));
        tokio::fs::write(&tmp_cs_config, cs_config).await?;
        LxdClient::push_file(container, &tmp_cs_config, "root/.config/code-server/config.yaml").await?;
        let _ = tokio::fs::remove_file(&tmp_cs_config).await;

        // VS Code settings: dark theme, disable built-in AI features, disable auto port forwarding
        LxdClient::exec(container, &["mkdir", "-p", "/root/.local/share/code-server/User"]).await?;
        let cs_settings = r#"{
  "workbench.colorTheme": "Default Dark Modern",
  "chat.disableAIFeatures": true,
  "workbench.startupEditor": "none",
  "telemetry.telemetryLevel": "off",
  "remote.autoForwardPorts": false
}
"#;
        let tmp_cs_settings = PathBuf::from(format!("/tmp/cs-settings-{service_name}.json"));
        tokio::fs::write(&tmp_cs_settings, cs_settings).await?;
        LxdClient::push_file(container, &tmp_cs_settings, "root/.local/share/code-server/User/settings.json").await?;
        let _ = tokio::fs::remove_file(&tmp_cs_settings).await;

        // Create systemd service for code-server (opens /root/workspace by default)
        // Extension install runs as a one-shot service in the background to avoid blocking deploy
        let cs_unit = r#"[Unit]
Description=code-server IDE
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/code-server --bind-addr 127.0.0.1:13337 /root/workspace
Restart=always
RestartSec=5
Environment=HOME=/root

# Ensure all child processes (extensions, LSP, file watchers) are killed on stop
KillMode=control-group
KillSignal=SIGTERM
TimeoutStopSec=10

[Install]
WantedBy=multi-user.target
"#;
        let tmp_cs_unit = PathBuf::from(format!("/tmp/cs-unit-{service_name}.service"));
        tokio::fs::write(&tmp_cs_unit, cs_unit).await?;
        LxdClient::push_file(container, &tmp_cs_unit, "etc/systemd/system/code-server.service").await?;
        let _ = tokio::fs::remove_file(&tmp_cs_unit).await;

        // One-shot service to install/update Claude Code extension on every boot
        // Uninstalls first to ensure latest version is always fetched
        let cs_setup_unit = r#"[Unit]
Description=code-server Claude Code extension updater
After=network-online.target code-server.service
Wants=network-online.target

[Service]
Type=oneshot
ExecStartPre=-/usr/local/bin/code-server --uninstall-extension Anthropic.claude-code
ExecStart=/usr/local/bin/code-server --install-extension Anthropic.claude-code
RemainAfterExit=true
Environment=HOME=/root

[Install]
WantedBy=multi-user.target
"#;
        let tmp_cs_setup = PathBuf::from(format!("/tmp/cs-setup-{service_name}.service"));
        tokio::fs::write(&tmp_cs_setup, cs_setup_unit).await?;
        LxdClient::push_file(container, &tmp_cs_setup, "etc/systemd/system/code-server-setup.service").await?;
        let _ = tokio::fs::remove_file(&tmp_cs_setup).await;

        emit("Demarrage de code-server...");
        LxdClient::exec(container, &["systemctl", "daemon-reload"]).await?;
        LxdClient::exec(container, &["systemctl", "enable", "--now", "code-server"]).await?;
        LxdClient::exec(container, &["systemctl", "enable", "--now", "code-server-setup"]).await?;
        info!(container, "code-server installed and started");

        info!(container, "Agent deployed");
        Ok(())
    }

    /// Create DNS records, add firewall rule, push full config with wildcard certs.
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

        // Get wildcard certificates for this app's domains
        let domains = app.domains(&base_domain);
        let routes = app.routes(&base_domain);
        let mut agent_routes = Vec::new();

        for route_info in &routes {
            // Determine which wildcard to use based on domain
            let wildcard_type = WildcardType::from_domain(&route_info.domain);

            let (cert_pem, key_pem) = self
                .acme
                .get_cert_pem(wildcard_type)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get wildcard cert for {}: {e}", route_info.domain))?;

            agent_routes.push(AgentRoute {
                domain: route_info.domain.clone(),
                target_port: route_info.target_port,
                cert_pem,
                key_pem,
                auth_required: route_info.auth_required,
                allowed_groups: route_info.allowed_groups.clone(),
            });
        }
        // No longer tracking per-domain cert_ids (using wildcards)
        app.cert_ids = vec![];

        // Create Cloudflare AAAA records
        let ipv6_str = app
            .ipv6_address
            .map(|a| a.to_string())
            .unwrap_or_default();

        if !ipv6_str.is_empty() {
            if let (Some(token), Some(zone_id)) = (&self.env.cf_api_token, &self.env.cf_zone_id) {
                let mut record_ids = Vec::new();
                for domain in &domains {
                    match cloudflare::upsert_aaaa_record(token, zone_id, domain, &ipv6_str, true)
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

        // Sync local DNS records (A + AAAA with agent IPv4 and IPv6)
        if let Some(ipv6) = app.ipv6_address {
            if let Err(e) = self.sync_local_dns_records(&domains, ipv6, app.ipv4_address) {
                warn!("Failed to sync local DNS records: {e}");
            }
        }

        // Let's Encrypt certs are trusted by default - no CA PEM needed
        let ca_pem = String::new();

        let auth_url = format!("http://10.0.0.254:{}/api/auth/forward-check", self.env.api_port);
        let dashboard_url = format!("https://hr.{}", self.env.base_domain);

        // Push full config
        tx.send(RegistryMessage::Config {
            config_version: Utc::now().timestamp() as u64,
            ipv6_address: ipv6_str,
            routes: agent_routes,
            ca_pem,
            homeroute_auth_url: auth_url,
            dashboard_url,
            services: app.services.clone(),
            power_policy: app.power_policy.clone(),
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
            // Get wildcard cert for this domain
            let wildcard_type = WildcardType::from_domain(&route_info.domain);

            let (cert_pem, key_pem) = match self.acme.get_cert_pem(wildcard_type).await {
                Ok(pems) => pems,
                Err(e) => {
                    warn!(domain = route_info.domain, "Failed to get wildcard cert: {e}");
                    continue;
                }
            };

            agent_routes.push(AgentRoute {
                domain: route_info.domain.clone(),
                target_port: route_info.target_port,
                cert_pem,
                key_pem,
                auth_required: route_info.auth_required,
                allowed_groups: route_info.allowed_groups.clone(),
            });
        }

        // Let's Encrypt certs are trusted by default - no CA PEM needed
        let ca_pem = String::new();

        let auth_url = format!(
            "http://10.0.0.254:{}/api/auth/forward-check",
            self.env.api_port
        );
        let dashboard_url = format!("https://hr.{}", self.env.base_domain);

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
                dashboard_url,
                services: app.services.clone(),
                power_policy: app.power_policy.clone(),
            })
            .await;
    }

    // ── Service control & metrics ──────────────────────────────────

    /// Send a service start/stop command to a connected agent.
    pub async fn send_service_command(
        &self,
        app_id: &str,
        service_type: ServiceType,
        action: ServiceAction,
    ) -> Result<bool> {
        let conns = self.connections.read().await;
        let Some(conn) = conns.get(app_id) else {
            return Ok(false);
        };

        conn.tx
            .send(RegistryMessage::ServiceCommand {
                service_type,
                action,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send command to agent"))?;

        info!(
            app_id,
            service_type = ?service_type,
            action = ?action,
            "Service command sent to agent"
        );
        Ok(true)
    }

    /// Update power policy for an application and push to connected agent.
    pub async fn update_power_policy(&self, app_id: &str, policy: PowerPolicy) -> Result<bool> {
        // Update in state
        {
            let mut state = self.state.write().await;
            let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) else {
                return Ok(false);
            };
            app.power_policy = policy.clone();
        }
        self.persist().await?;

        // Push to connected agent
        let conns = self.connections.read().await;
        if let Some(conn) = conns.get(app_id) {
            let _ = conn
                .tx
                .send(RegistryMessage::PowerPolicyUpdate(policy))
                .await;
            info!(app_id, "Power policy update sent to agent");
        }

        Ok(true)
    }

    /// Handle metrics received from an agent: update in-memory state and broadcast to WebSocket.
    pub async fn handle_metrics(&self, app_id: &str, metrics: AgentMetrics) {
        // Convert ServiceState to string for broadcast
        let code_server_status = format!("{:?}", metrics.code_server_status).to_lowercase();
        let app_status = format!("{:?}", metrics.app_status).to_lowercase();
        let db_status = format!("{:?}", metrics.db_status).to_lowercase();

        // Update in-memory metrics (not persisted)
        {
            let mut state = self.state.write().await;
            if let Some(app) = state.applications.iter_mut().find(|a| a.id == app_id) {
                app.metrics = Some(metrics.clone());
            }
        }

        // Broadcast to WebSocket
        let _ = self.events.agent_metrics.send(AgentMetricsEvent {
            app_id: app_id.to_string(),
            code_server_status,
            app_status,
            db_status,
            memory_bytes: metrics.memory_bytes,
            cpu_percent: metrics.cpu_percent,
            code_server_idle_secs: metrics.code_server_idle_secs,
            app_idle_secs: metrics.app_idle_secs,
        });
    }

    /// Handle service state changed event from agent (broadcasts to WebSocket).
    pub fn handle_service_state_changed(
        &self,
        app_id: &str,
        service_type: ServiceType,
        new_state: ServiceState,
    ) {
        use hr_common::events::ServiceCommandEvent;

        let action = match new_state {
            ServiceState::Running => "started",
            ServiceState::Stopped | ServiceState::ManuallyOff => "stopped",
            ServiceState::Starting => "starting",
            ServiceState::Stopping => "stopping",
        };

        let _ = self.events.service_command.send(ServiceCommandEvent {
            app_id: app_id.to_string(),
            service_type: format!("{:?}", service_type).to_lowercase(),
            action: action.to_string(),
            success: true,
        });
    }

    // ── Agent Update ────────────────────────────────────────────────

    /// Trigger update to specified agents (or all connected if None).
    /// Sends `UpdateAvailable` message to each agent with the current binary info.
    pub async fn trigger_update(
        &self,
        agent_ids: Option<Vec<String>>,
    ) -> Result<UpdateBatchResult> {
        use ring::digest::{Context, SHA256};
        use std::io::Read;

        // Read current binary and compute SHA256
        let binary_path = Path::new("/opt/homeroute/data/agent-binaries/hr-agent");
        if !binary_path.exists() {
            anyhow::bail!("Agent binary not found at {}", binary_path.display());
        }

        let metadata = std::fs::metadata(binary_path)?;
        let modified = metadata
            .modified()
            .map(|t| {
                let dt: DateTime<Utc> = t.into();
                dt.format("%Y%m%d-%H%M%S").to_string()
            })
            .unwrap_or_else(|_| "unknown".to_string());

        let mut file = std::fs::File::open(binary_path)?;
        let mut context = Context::new(&SHA256);
        let mut buffer = [0u8; 8192];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }
        let sha256 = hex::encode(context.finish().as_ref());

        let download_url = format!(
            "http://10.0.0.254:{}/api/applications/agents/binary",
            self.env.api_port
        );

        let state = self.state.read().await;
        let conns = self.connections.read().await;

        let mut notified = Vec::new();
        let mut skipped = Vec::new();

        // Determine which apps to target
        let target_ids: Vec<&str> = match &agent_ids {
            Some(ids) => ids.iter().map(|s| s.as_str()).collect(),
            None => conns.keys().map(|s| s.as_str()).collect(),
        };

        for app in &state.applications {
            if !target_ids.contains(&app.id.as_str()) {
                continue;
            }

            if let Some(conn) = conns.get(&app.id) {
                let msg = RegistryMessage::UpdateAvailable {
                    version: modified.clone(),
                    download_url: download_url.clone(),
                    sha256: sha256.clone(),
                };

                if conn.tx.send(msg).await.is_ok() {
                    notified.push(AgentNotifyResult {
                        id: app.id.clone(),
                        slug: app.slug.clone(),
                        status: "notified".to_string(),
                    });

                    // Emit event
                    let _ = self.events.agent_update.send(AgentUpdateEvent {
                        app_id: app.id.clone(),
                        slug: app.slug.clone(),
                        status: AgentUpdateStatus::Notified,
                        version: Some(modified.clone()),
                        error: None,
                    });

                    info!(app = app.slug, version = modified, "Update notification sent");
                } else {
                    skipped.push(AgentSkipResult {
                        id: app.id.clone(),
                        slug: app.slug.clone(),
                        reason: "send_failed".to_string(),
                    });
                }
            } else {
                skipped.push(AgentSkipResult {
                    id: app.id.clone(),
                    slug: app.slug.clone(),
                    reason: "not_connected".to_string(),
                });
            }
        }

        info!(
            notified = notified.len(),
            skipped = skipped.len(),
            version = modified,
            "Agent update triggered"
        );

        Ok(UpdateBatchResult {
            version: modified,
            sha256,
            agents_notified: notified,
            agents_skipped: skipped,
        })
    }

    /// Get update status for all agents: whether they're connected with the expected version.
    pub async fn get_update_status(&self) -> Result<UpdateStatusResult> {
        use ring::digest::{Context, SHA256};
        use std::io::Read;

        // Get expected version from current binary
        let binary_path = Path::new("/opt/homeroute/data/agent-binaries/hr-agent");
        let expected_version = if binary_path.exists() {
            std::fs::metadata(binary_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let dt: DateTime<Utc> = t.into();
                    dt.format("%Y%m%d-%H%M%S").to_string()
                })
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "no_binary".to_string()
        };

        let state = self.state.read().await;
        let conns = self.connections.read().await;
        let now = Utc::now();

        let agents: Vec<AgentUpdateStatusInfo> = state
            .applications
            .iter()
            .map(|app| {
                let is_connected = conns.contains_key(&app.id);
                let version_matches = app
                    .agent_version
                    .as_ref()
                    .map(|v| v == &expected_version)
                    .unwrap_or(false);
                let has_recent_heartbeat = app
                    .last_heartbeat
                    .map(|hb| now - hb < chrono::Duration::seconds(90))
                    .unwrap_or(false);

                let update_status = if !is_connected {
                    "disconnected"
                } else if version_matches {
                    "success"
                } else {
                    "pending"
                };

                AgentUpdateStatusInfo {
                    id: app.id.clone(),
                    slug: app.slug.clone(),
                    container_name: app.container_name.clone(),
                    status: if is_connected {
                        "connected"
                    } else {
                        "disconnected"
                    }
                    .to_string(),
                    current_version: app.agent_version.clone(),
                    update_status: update_status.to_string(),
                    metrics_flowing: is_connected && has_recent_heartbeat,
                    last_heartbeat: app.last_heartbeat,
                }
            })
            .collect();

        Ok(UpdateStatusResult {
            expected_version,
            agents,
        })
    }

    /// Fix a failed agent update via LXC exec (fallback mechanism).
    /// Downloads the binary directly in the container and restarts the agent.
    pub async fn fix_agent_via_lxc(&self, app_id: &str) -> Result<String> {
        let (container, slug) = {
            let state = self.state.read().await;
            let app = state
                .applications
                .iter()
                .find(|a| a.id == app_id)
                .ok_or_else(|| anyhow::anyhow!("Application not found: {}", app_id))?;
            (app.container_name.clone(), app.slug.clone())
        };

        let api_port = self.env.api_port;

        info!(container = container, slug = slug, "Fixing agent via LXC exec");

        // Download new binary directly in the container and restart
        let download_cmd = format!(
            "curl -fsSL http://10.0.0.254:{}/api/applications/agents/binary -o /usr/local/bin/hr-agent.new && \
             chmod +x /usr/local/bin/hr-agent.new && \
             mv /usr/local/bin/hr-agent.new /usr/local/bin/hr-agent && \
             systemctl restart hr-agent",
            api_port
        );

        let output = LxdClient::exec(&container, &["bash", "-c", &download_cmd]).await?;

        info!(container = container, "Agent fixed via LXC exec");

        // Emit event
        let _ = self.events.agent_update.send(AgentUpdateEvent {
            app_id: app_id.to_string(),
            slug: slug.to_string(),
            status: AgentUpdateStatus::Notified,
            version: None,
            error: None,
        });

        Ok(output)
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
