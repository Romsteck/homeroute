//! Environment manager: lifecycle orchestration for HomeRoute environments.
//!
//! Manages environment records (dev, acc, prod), their tokens, and env-agent
//! WebSocket connections. Similar pattern to ContainerManager + AgentRegistry.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use hr_environment::protocol::EnvOrchestratorMessage;
use hr_environment::types::{EnvApp, EnvStatus, EnvType, EnvironmentRecord};

// ── Types ────────────────────────────────────────────────────────

/// Persisted state file structure.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct EnvironmentState {
    environments: Vec<StoredEnvRecord>,
}

/// On-disk environment record (includes token_hash, unlike the public EnvironmentRecord).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct StoredEnvRecord {
    #[serde(flatten)]
    pub record: EnvironmentRecord,
    pub token_hash: String,
}

// ── EnvironmentManager ───────────────────────────────────────────

pub struct EnvironmentManager {
    state_path: PathBuf,
    environments: Arc<RwLock<Vec<StoredEnvRecord>>>,
    /// Active WebSocket connections to env-agents, keyed by env_slug.
    connections: Arc<RwLock<HashMap<String, mpsc::Sender<EnvOrchestratorMessage>>>>,
}

impl EnvironmentManager {
    /// Create and load state from JSON file.
    pub fn new(state_path: PathBuf) -> Self {
        let environments = match std::fs::read_to_string(&state_path) {
            Ok(content) => {
                let state: EnvironmentState =
                    serde_json::from_str(&content).unwrap_or_default();
                info!(
                    count = state.environments.len(),
                    "Loaded environments from {}",
                    state_path.display()
                );
                state.environments
            }
            Err(_) => {
                info!("No environments state file, starting empty");
                Vec::new()
            }
        };

        Self {
            state_path,
            environments: Arc::new(RwLock::new(environments)),
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Derive the container name from an environment slug.
    pub fn container_name_for_env(slug: &str) -> String {
        format!("env-{}", slug)
    }

    /// List all environments (public records, no token hashes).
    pub async fn list_environments(&self) -> Vec<EnvironmentRecord> {
        let envs = self.environments.read().await;
        envs.iter().map(|e| e.record.clone()).collect()
    }

    /// Get a single environment by ID.
    pub async fn get_environment(&self, id: &str) -> Option<EnvironmentRecord> {
        let envs = self.environments.read().await;
        envs.iter()
            .find(|e| e.record.id == id)
            .map(|e| e.record.clone())
    }

    /// Get a single environment by slug.
    pub async fn get_by_slug(&self, slug: &str) -> Option<EnvironmentRecord> {
        let envs = self.environments.read().await;
        envs.iter()
            .find(|e| e.record.slug == slug)
            .map(|e| e.record.clone())
    }

    /// Create a new environment. Returns (record, plaintext_token).
    pub async fn create_environment(
        &self,
        name: String,
        slug: String,
        env_type: EnvType,
        host_id: String,
    ) -> anyhow::Result<(EnvironmentRecord, String)> {
        // Check slug uniqueness
        {
            let envs = self.environments.read().await;
            if envs.iter().any(|e| e.record.slug == slug) {
                anyhow::bail!("Environment with slug '{}' already exists", slug);
            }
        }

        let token_clear = generate_token();
        let token_hash = hash_token(&token_clear)?;

        let id = uuid::Uuid::new_v4().to_string();
        let container_name = format!("env-{}", slug);

        let record = EnvironmentRecord {
            id,
            name,
            slug,
            env_type,
            host_id,
            container_name,
            ipv4_address: None,
            status: EnvStatus::Pending,
            agent_connected: false,
            agent_version: None,
            last_heartbeat: None,
            apps: Vec::new(),
            created_at: Utc::now(),
        };

        let stored = StoredEnvRecord {
            record: record.clone(),
            token_hash,
        };

        {
            let mut envs = self.environments.write().await;
            envs.push(stored);
        }

        self.persist().await;
        info!(
            id = record.id,
            slug = record.slug,
            "Environment created (token returned)"
        );

        Ok((record, token_clear))
    }

    /// Delete an environment by ID.
    pub async fn delete_environment(&self, id: &str) -> anyhow::Result<()> {
        let mut envs = self.environments.write().await;
        let len_before = envs.len();
        envs.retain(|e| e.record.id != id);
        if envs.len() == len_before {
            anyhow::bail!("Environment not found: {id}");
        }
        drop(envs);
        self.persist().await;
        info!(id, "Environment deleted");
        Ok(())
    }

    /// Delete an environment by slug.
    pub async fn delete_environment_by_slug(&self, slug: &str) -> anyhow::Result<()> {
        let mut envs = self.environments.write().await;
        let len_before = envs.len();
        envs.retain(|e| e.record.slug != slug);
        if envs.len() == len_before {
            anyhow::bail!("Environment not found: {slug}");
        }
        drop(envs);

        // Remove connection if any
        self.connections.write().await.remove(slug);

        self.persist().await;
        info!(slug, "Environment deleted by slug");
        Ok(())
    }

    /// Update the status of an environment by ID.
    pub async fn update_environment_status(&self, id_or_slug: &str, status: EnvStatus) {
        let mut envs = self.environments.write().await;
        if let Some(env) = envs.iter_mut().find(|e| e.record.id == id_or_slug || e.record.slug == id_or_slug) {
            env.record.status = status;
        }
        drop(envs);
        self.persist().await;
    }

    /// Update agent connection state (called on WS connect/disconnect).
    pub async fn update_agent_connected(
        &self,
        env_slug: &str,
        connected: bool,
        version: Option<String>,
        ipv4: Option<Ipv4Addr>,
    ) {
        let mut envs = self.environments.write().await;
        if let Some(env) = envs.iter_mut().find(|e| e.record.slug == env_slug) {
            env.record.agent_connected = connected;
            if connected {
                env.record.status = EnvStatus::Running;
                env.record.last_heartbeat = Some(Utc::now());
                if let Some(v) = version {
                    env.record.agent_version = Some(v);
                }
                if let Some(ip) = ipv4 {
                    env.record.ipv4_address = Some(ip);
                }
            } else {
                env.record.status = EnvStatus::Disconnected;
            }
        }
        drop(envs);
        self.persist().await;
    }

    /// Update the app list for an environment (from env-agent AppDiscovery).
    pub async fn update_apps(&self, env_slug: &str, apps: Vec<EnvApp>) {
        let mut envs = self.environments.write().await;
        if let Some(env) = envs.iter_mut().find(|e| e.record.slug == env_slug) {
            env.record.apps = apps;
            env.record.last_heartbeat = Some(Utc::now());
        }
        drop(envs);
        self.persist().await;
    }

    /// Register a WebSocket connection for an env-agent.
    pub async fn register_connection(
        &self,
        env_slug: &str,
        tx: mpsc::Sender<EnvOrchestratorMessage>,
    ) -> anyhow::Result<()> {
        let mut conns = self.connections.write().await;
        conns.insert(env_slug.to_string(), tx);
        info!(env_slug, "Env-agent connection registered");
        Ok(())
    }

    /// Unregister a WebSocket connection and mark env as disconnected.
    pub async fn unregister_connection(&self, env_slug: &str) {
        {
            let mut conns = self.connections.write().await;
            conns.remove(env_slug);
        }
        self.update_agent_connected(env_slug, false, None, None).await;
        info!(env_slug, "Env-agent connection unregistered");
    }

    /// Update heartbeat for an env-agent (from Heartbeat message).
    pub async fn update_heartbeat(&self, env_slug: &str, _apps_running: u32, _apps_total: u32) {
        let mut envs = self.environments.write().await;
        if let Some(env) = envs.iter_mut().find(|e| e.record.slug == env_slug) {
            env.record.last_heartbeat = Some(Utc::now());
        }
        // No persist on heartbeat (too frequent), just update in-memory
    }

    /// Send a message to an env-agent via its WebSocket channel.
    pub async fn send_to_env(
        &self,
        env_slug: &str,
        msg: EnvOrchestratorMessage,
    ) -> anyhow::Result<()> {
        let conns = self.connections.read().await;
        if let Some(tx) = conns.get(env_slug) {
            tx.send(msg)
                .await
                .map_err(|_| anyhow::anyhow!("Env-agent channel closed for {env_slug}"))?;
            Ok(())
        } else {
            anyhow::bail!("No connection for env-agent {env_slug}")
        }
    }

    /// Check if an env-agent is connected.
    pub async fn is_env_connected(&self, env_slug: &str) -> bool {
        let conns = self.connections.read().await;
        conns.contains_key(env_slug)
    }

    /// Get the version of a specific app in a specific environment.
    pub async fn get_app_version(&self, env_slug: &str, app_slug: &str) -> Option<String> {
        let envs = self.environments.read().await;
        let env = envs.iter().find(|e| e.record.slug == env_slug)?;
        let app = env.record.apps.iter().find(|a| a.slug == app_slug)?;
        app.version.clone()
    }

    /// Verify a token for a given env_slug. Returns the env_id if valid.
    pub async fn verify_token(&self, env_slug: &str, token: &str) -> Option<String> {
        let envs = self.environments.read().await;
        for env in envs.iter() {
            if env.record.slug == env_slug && verify_token(token, &env.token_hash) {
                return Some(env.record.id.clone());
            }
        }
        None
    }

    /// List environments running on a specific host.
    pub async fn list_by_host(&self, host_id: &str) -> Vec<EnvironmentRecord> {
        let envs = self.environments.read().await;
        envs.iter()
            .filter(|e| e.record.host_id == host_id)
            .map(|e| e.record.clone())
            .collect()
    }

    /// Get capacity info for a host (env count, running count, list).
    pub async fn get_host_capacity(&self, host_id: &str) -> serde_json::Value {
        let envs = self.list_by_host(host_id).await;
        serde_json::json!({
            "host_id": host_id,
            "env_count": envs.len(),
            "running_count": envs.iter().filter(|e| matches!(e.status, EnvStatus::Running)).count(),
            "environments": envs.iter().map(|e| serde_json::json!({
                "slug": e.slug,
                "name": e.name,
                "env_type": e.env_type,
                "status": e.status,
                "agent_connected": e.agent_connected,
                "apps_count": e.apps.len(),
            })).collect::<Vec<_>>(),
        })
    }

    /// Select the best host for a new environment (multi-host ready).
    /// Currently always returns "medion". Will be expanded for load balancing
    /// across multiple hosts once host-agents report capacity metrics.
    pub async fn select_host(&self, _env_type: EnvType) -> String {
        // TODO: When multi-host is enabled, query host-agents for capacity
        // and select the best host based on available resources.
        // For now, all environments run on the single production host.
        "medion".to_string()
    }

    /// Monitor env-agent heartbeats and mark stale connections as disconnected.
    pub async fn run_heartbeat_monitor(&self) {
        let stale_threshold = std::time::Duration::from_secs(120);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let now = Utc::now();
            let mut changed = false;
            {
                let mut envs = self.environments.write().await;
                for env in envs.iter_mut() {
                    if env.record.agent_connected {
                        if let Some(last_hb) = env.record.last_heartbeat {
                            let elapsed = (now - last_hb).to_std().unwrap_or_default();
                            if elapsed > stale_threshold {
                                warn!(
                                    env_slug = env.record.slug,
                                    elapsed_secs = elapsed.as_secs(),
                                    "Env-agent heartbeat stale, marking disconnected"
                                );
                                env.record.agent_connected = false;
                                env.record.status = EnvStatus::Disconnected;
                                changed = true;
                            }
                        }
                    }
                }
            }
            if changed {
                self.persist().await;
            }
        }
    }

    /// Persist state to JSON file.
    async fn persist(&self) {
        let envs = self.environments.read().await;
        let state = EnvironmentState {
            environments: envs.clone(),
        };
        let json = match serde_json::to_string_pretty(&state) {
            Ok(j) => j,
            Err(e) => {
                error!("Failed to serialize environments state: {e}");
                return;
            }
        };
        if let Err(e) = tokio::fs::write(&self.state_path, json).await {
            error!(
                path = %self.state_path.display(),
                "Failed to persist environments state: {e}"
            );
        }
    }
}

// ── Token utilities ──────────────────────────────────────────────

fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn hash_token(token: &str) -> anyhow::Result<String> {
    use argon2::password_hash::SaltString;
    use argon2::{Argon2, PasswordHasher};
    use rand_core::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash failed: {e}"))?;
    Ok(hash.to_string())
}

fn verify_token(token: &str, hash: &str) -> bool {
    use argon2::password_hash::PasswordHash;
    use argon2::{Argon2, PasswordVerifier};

    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(token.as_bytes(), &parsed)
        .is_ok()
}
