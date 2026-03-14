// container_watcher.rs — Container Health Watcher
//
// Periodically checks the status of all registered prod containers.
// If a container's agent has not sent a heartbeat for >2 minutes
// (and the container is not freshly deploying/pending), it attempts
// to auto-recover by:
//   1. machinectl terminate <name>     — clear stale machined registration
//   2. ip link delete vb-<name>        — clear stale veth if present
//   3. systemctl restart systemd-nspawn@<name>.service — restart container
//
// Rate limit: max 3 recovery attempts per container per hour.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hr_common::events::{AgentStatusEvent, EventBus};
use hr_registry::AgentRegistry;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

// ── Constants ─────────────────────────────────────────────────────────────────

/// How often to check all containers.
const CHECK_INTERVAL_SECS: u64 = 60;

/// How long since the last heartbeat before we consider a container dead.
const HEARTBEAT_TIMEOUT_SECS: i64 = 120;

/// Max auto-recovery attempts per container within the rolling window.
const MAX_ATTEMPTS_PER_HOUR: usize = 3;

/// Rolling window for rate limiting (1 hour).
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

// ── ContainerWatcher ──────────────────────────────────────────────────────────

pub struct ContainerWatcher {
    registry: Arc<AgentRegistry>,
    events: Arc<EventBus>,
    /// In-memory rate-limit tracker: container_name → timestamps of recent recoveries.
    recovery_log: Mutex<HashMap<String, Vec<Instant>>>,
}

impl ContainerWatcher {
    pub fn new(registry: Arc<AgentRegistry>, events: Arc<EventBus>) -> Self {
        Self {
            registry,
            events,
            recovery_log: Mutex::new(HashMap::new()),
        }
    }

    /// Spawn the watcher as a background tokio task.
    pub fn spawn(registry: Arc<AgentRegistry>, events: Arc<EventBus>) {
        let watcher = Arc::new(Self::new(registry, events));
        tokio::spawn(async move {
            watcher.run().await;
        });
    }

    async fn run(&self) {
        info!("Container health watcher started (interval={}s, heartbeat_timeout={}s, max_attempts={}/h)",
            CHECK_INTERVAL_SECS, HEARTBEAT_TIMEOUT_SECS, MAX_ATTEMPTS_PER_HOUR);

        // Initial delay so orchestrator can fully start before first check.
        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            self.check_all_containers().await;
            tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        }
    }

    async fn check_all_containers(&self) {
        let apps = self.registry.list_applications().await;
        let now = chrono::Utc::now();

        for app in &apps {
            // Only watch prod containers that are enabled and locally hosted.
            if app.environment != hr_registry::types::Environment::Production {
                continue;
            }
            if !app.enabled {
                continue;
            }
            // Skip containers that are deploying or in initial pending state
            // (they haven't connected yet — give them time).
            match app.status {
                hr_registry::types::AgentStatus::Deploying => continue,
                hr_registry::types::AgentStatus::Pending => continue,
                _ => {}
            }

            // Check if heartbeat is stale (or never received).
            let needs_recovery = match app.last_heartbeat {
                None => {
                    // Never connected — only flag if created more than the timeout ago.
                    let age = now.signed_duration_since(app.created_at).num_seconds();
                    age > HEARTBEAT_TIMEOUT_SECS * 2 // extra grace for first boot
                }
                Some(hb) => {
                    let age_secs = now.signed_duration_since(hb).num_seconds();
                    age_secs > HEARTBEAT_TIMEOUT_SECS
                }
            };

            if !needs_recovery {
                continue;
            }

            let hb_age = app.last_heartbeat
                .map(|hb| now.signed_duration_since(hb).num_seconds())
                .map(|s| format!("{}s ago", s))
                .unwrap_or_else(|| "never".to_string());

            warn!(
                container = %app.container_name,
                slug = %app.slug,
                status = ?app.status,
                last_heartbeat = %hb_age,
                "Container appears dead — checking rate limit before recovery attempt"
            );

            // Check rate limit.
            if !self.can_attempt_recovery(&app.container_name).await {
                warn!(
                    container = %app.container_name,
                    "Rate limit reached ({} attempts/h) — skipping recovery",
                    MAX_ATTEMPTS_PER_HOUR
                );
                continue;
            }

            // Attempt recovery.
            self.attempt_recovery(&app.container_name, &app.id, &app.slug).await;
        }
    }

    /// Returns true if we are allowed to attempt recovery for this container.
    async fn can_attempt_recovery(&self, container_name: &str) -> bool {
        let mut log = self.recovery_log.lock().await;
        let now = Instant::now();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

        let timestamps = log.entry(container_name.to_string()).or_default();

        // Evict entries outside the rolling window.
        timestamps.retain(|t| now.duration_since(*t) < window);

        if timestamps.len() >= MAX_ATTEMPTS_PER_HOUR {
            return false;
        }

        // Record this attempt.
        timestamps.push(now);
        true
    }

    async fn attempt_recovery(&self, container_name: &str, app_id: &str, slug: &str) {
        let veth = format!("vb-{container_name}");

        info!(
            container = container_name,
            veth = %veth,
            "Starting auto-recovery"
        );

        // Broadcast "recovering" status to WebSocket clients.
        let _ = self.events.agent_status.send(AgentStatusEvent {
            app_id: app_id.to_string(),
            slug: slug.to_string(),
            status: "recovering".to_string(),
            message: Some(format!("Auto-recovery: container {container_name} appears dead, restarting...")),
        });

        // Step 1: machinectl terminate (clear stale machined registration).
        let r1 = run_cmd("machinectl", &["terminate", container_name]).await;
        match &r1 {
            Ok(out) => info!(container = container_name, "machinectl terminate: {}", out.trim()),
            Err(e)  => warn!(container = container_name, "machinectl terminate failed (expected if not registered): {}", e),
        }

        // Brief pause to let machined settle.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Step 2: ip link delete vb-<name> (clear stale veth).
        let r2 = run_cmd("ip", &["link", "delete", &veth]).await;
        match &r2 {
            Ok(out) => info!(container = container_name, "ip link delete {}: {}", veth, out.trim()),
            Err(e)  => info!(container = container_name, "ip link delete {} failed (expected if not present): {}", veth, e),
        }

        // Step 3: systemctl restart systemd-nspawn@<name>.service
        let service = format!("systemd-nspawn@{container_name}.service");
        let r3 = run_cmd("systemctl", &["restart", &service]).await;
        match &r3 {
            Ok(_) => {
                info!(
                    container = container_name,
                    service = %service,
                    "Auto-recovery: systemd-nspawn service restarted successfully"
                );
                let _ = self.events.agent_status.send(AgentStatusEvent {
                    app_id: app_id.to_string(),
                    slug: slug.to_string(),
                    status: "pending".to_string(),
                    message: Some(format!("Auto-recovery: {container_name} restarted, waiting for agent...")),
                });
            }
            Err(e) => {
                error!(
                    container = container_name,
                    service = %service,
                    "Auto-recovery: failed to restart service: {}",
                    e
                );
                let _ = self.events.agent_status.send(AgentStatusEvent {
                    app_id: app_id.to_string(),
                    slug: slug.to_string(),
                    status: "error".to_string(),
                    message: Some(format!("Auto-recovery failed for {container_name}: {e}")),
                });
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to spawn {program}: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(format!(
            "exited with code {code}: {stderr}",
        ))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_up_to_max() {
        use hr_common::config::EnvConfig;
        use std::path::PathBuf;

        // Build a minimal registry just for the rate-limit test.
        let env = Arc::new(EnvConfig::load(None));
        let events = Arc::new(EventBus::new());
        let registry = Arc::new(AgentRegistry::new(
            PathBuf::from("/tmp/test-container-watcher-registry.json"),
            env,
            events.clone(),
        ));

        let watcher = ContainerWatcher::new(registry, events);
        let name = "hr-v2-test-prod";

        // Should allow up to MAX_ATTEMPTS_PER_HOUR.
        for i in 0..MAX_ATTEMPTS_PER_HOUR {
            assert!(
                watcher.can_attempt_recovery(name).await,
                "attempt {} should be allowed",
                i + 1
            );
        }

        // Next attempt should be denied.
        assert!(
            !watcher.can_attempt_recovery(name).await,
            "attempt {} should be denied (rate limit)",
            MAX_ATTEMPTS_PER_HOUR + 1
        );
    }

    #[tokio::test]
    async fn test_rate_limiter_different_containers_independent() {
        use hr_common::config::EnvConfig;
        use std::path::PathBuf;

        let env = Arc::new(EnvConfig::load(None));
        let events = Arc::new(EventBus::new());
        let registry = Arc::new(AgentRegistry::new(
            PathBuf::from("/tmp/test-container-watcher-registry2.json"),
            env,
            events.clone(),
        ));

        let watcher = ContainerWatcher::new(registry, events);

        // Exhaust container A.
        for _ in 0..MAX_ATTEMPTS_PER_HOUR {
            watcher.can_attempt_recovery("hr-v2-app-a-prod").await;
        }
        assert!(!watcher.can_attempt_recovery("hr-v2-app-a-prod").await);

        // Container B should still be allowed.
        assert!(watcher.can_attempt_recovery("hr-v2-app-b-prod").await);
    }
}
