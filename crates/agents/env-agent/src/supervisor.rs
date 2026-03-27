//! App supervisor — manages app processes via systemd services.
//!
//! Each app runs as a systemd service: {slug}.service
//! The supervisor can start, stop, restart, and check health.

use anyhow::{anyhow, Result};
use hr_environment::config::EnvAgentAppConfig;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{debug, error, warn};

/// Status of an app process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppProcessStatus {
    Running,
    Stopped,
    Failed,
    Unknown,
}

impl std::fmt::Display for AppProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Information about an app process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProcessInfo {
    pub slug: String,
    pub name: String,
    pub port: u16,
    pub status: AppProcessStatus,
    pub version: Option<String>,
}

/// Manages app processes within the environment via systemd services.
pub struct AppSupervisor {
    apps: Vec<EnvAgentAppConfig>,
}

impl AppSupervisor {
    /// Create a new supervisor from the list of configured apps.
    pub fn new(apps: Vec<EnvAgentAppConfig>) -> Self {
        Self { apps }
    }

    /// Find an app config by slug, or return an error.
    fn find_app(&self, slug: &str) -> Result<&EnvAgentAppConfig> {
        self.apps
            .iter()
            .find(|a| a.slug == slug)
            .ok_or_else(|| anyhow!("app not found: {}", slug))
    }

    /// Systemd service name for a given slug.
    fn service_name(slug: &str) -> String {
        format!("{}.service", slug)
    }

    /// Start an app's systemd service.
    pub async fn start_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        debug!(slug, "starting app service");
        run_systemctl(&["start", &svc]).await
    }

    /// Stop an app's systemd service.
    pub async fn stop_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        debug!(slug, "stopping app service");
        run_systemctl(&["stop", &svc]).await
    }

    /// Restart an app's systemd service.
    pub async fn restart_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        debug!(slug, "restarting app service");
        run_systemctl(&["restart", &svc]).await
    }

    /// Query the status of an app's systemd service.
    pub async fn app_status(&self, slug: &str) -> Result<AppProcessStatus> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        let output = Command::new("systemctl")
            .args(["is-active", &svc])
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(parse_active_state(&stdout))
    }

    /// Return status info for all configured apps.
    pub async fn list_apps(&self) -> Vec<AppProcessInfo> {
        let mut result = Vec::with_capacity(self.apps.len());
        for app in &self.apps {
            let status = match self.app_status(&app.slug).await {
                Ok(s) => s,
                Err(e) => {
                    warn!(slug = %app.slug, error = %e, "failed to get app status");
                    AppProcessStatus::Unknown
                }
            };
            result.push(AppProcessInfo {
                slug: app.slug.clone(),
                name: app.name.clone(),
                port: app.port,
                status,
                version: None,
            });
        }
        result
    }

    /// Perform an HTTP health check against the app's health endpoint.
    ///
    /// Sends a minimal HTTP/1.1 GET request over a raw TCP connection
    /// (no reqwest dependency needed) and returns true if a 2xx response
    /// is received within a reasonable timeout.
    pub async fn health_check(&self, slug: &str) -> Result<bool> {
        let app = self.find_app(slug)?;
        let addr = format!("127.0.0.1:{}", app.port);
        let path = &app.health_path;

        let connect = TcpStream::connect(&addr);
        let mut stream = match tokio::time::timeout(std::time::Duration::from_secs(3), connect).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                debug!(slug, error = %e, "health check: connection refused");
                return Ok(false);
            }
            Err(_) => {
                debug!(slug, "health check: connection timed out");
                return Ok(false);
            }
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            path, app.port
        );
        if let Err(e) = stream.write_all(request.as_bytes()).await {
            debug!(slug, error = %e, "health check: write failed");
            return Ok(false);
        }

        let mut buf = vec![0u8; 1024];
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf)).await;
        match read {
            Ok(Ok(n)) if n > 0 => {
                let response = String::from_utf8_lossy(&buf[..n]);
                // Check for HTTP/1.x 2xx status line
                if let Some(status_line) = response.lines().next() {
                    if status_line.starts_with("HTTP/") {
                        if let Some(code_str) = status_line.split_whitespace().nth(1) {
                            if let Ok(code) = code_str.parse::<u16>() {
                                return Ok((200..300).contains(&code));
                            }
                        }
                    }
                }
                Ok(false)
            }
            _ => {
                debug!(slug, "health check: no response");
                Ok(false)
            }
        }
    }

    /// Retrieve recent log lines for an app's service.
    pub async fn logs(&self, slug: &str, lines: u32) -> Result<String> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        let output = Command::new("journalctl")
            .args([
                "-u",
                &svc,
                "--no-pager",
                "-n",
                &lines.to_string(),
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(slug, stderr = %stderr, "journalctl failed");
            return Err(anyhow!("journalctl failed: {}", stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Start all configured apps.
    pub async fn start_all(&self) {
        for app in &self.apps {
            if let Err(e) = self.start_app(&app.slug).await {
                error!(slug = %app.slug, error = %e, "failed to start app");
            }
        }
    }

    /// Stop all configured apps.
    pub async fn stop_all(&self) {
        for app in &self.apps {
            if let Err(e) = self.stop_app(&app.slug).await {
                error!(slug = %app.slug, error = %e, "failed to stop app");
            }
        }
    }
}

/// Run a systemctl command and return Ok(()) on success.
async fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl").args(args).output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "systemctl {} failed: {}",
            args.join(" "),
            stderr.trim()
        ));
    }
    Ok(())
}

/// Parse the output of `systemctl is-active` into an AppProcessStatus.
fn parse_active_state(state: &str) -> AppProcessStatus {
    match state {
        "active" => AppProcessStatus::Running,
        "inactive" | "deactivating" => AppProcessStatus::Stopped,
        "failed" => AppProcessStatus::Failed,
        _ => AppProcessStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_active_state() {
        assert_eq!(parse_active_state("active"), AppProcessStatus::Running);
        assert_eq!(parse_active_state("inactive"), AppProcessStatus::Stopped);
        assert_eq!(parse_active_state("failed"), AppProcessStatus::Failed);
        assert_eq!(parse_active_state("activating"), AppProcessStatus::Unknown);
        assert_eq!(parse_active_state(""), AppProcessStatus::Unknown);
    }

    #[test]
    fn test_service_name() {
        assert_eq!(AppSupervisor::service_name("trader"), "trader.service");
        assert_eq!(AppSupervisor::service_name("my-app"), "my-app.service");
    }

    #[test]
    fn test_find_app() {
        let apps = vec![EnvAgentAppConfig {
            slug: "trader".into(),
            name: "Trader".into(),
            stack: Default::default(),
            port: 3001,
            run_command: "./bin/trader".into(),
            build_command: None,
            health_path: "/api/health".into(),
            has_db: false,
        }];
        let supervisor = AppSupervisor::new(apps);
        assert!(supervisor.find_app("trader").is_ok());
        assert!(supervisor.find_app("nonexistent").is_err());
    }

    #[test]
    fn test_status_display() {
        assert_eq!(AppProcessStatus::Running.to_string(), "running");
        assert_eq!(AppProcessStatus::Stopped.to_string(), "stopped");
        assert_eq!(AppProcessStatus::Failed.to_string(), "failed");
        assert_eq!(AppProcessStatus::Unknown.to_string(), "unknown");
    }
}
