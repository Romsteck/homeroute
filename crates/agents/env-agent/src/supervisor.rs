//! App supervisor — manages app processes via systemd services.
//!
//! Each app runs as a systemd service: {slug}.service
//! In dev environments, an optional watch service ({slug}-watch.service)
//! rebuilds the app on file changes.
//! The supervisor can start, stop, restart, and check health.

use anyhow::{anyhow, Result};
use hr_environment::config::EnvAgentAppConfig;
use hr_environment::types::EnvType;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

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
    pub stack: hr_environment::types::AppStackType,
    pub port: u16,
    pub status: AppProcessStatus,
    /// Status of the watch service (dev only, None in acc/prod).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_status: Option<AppProcessStatus>,
    pub has_db: bool,
    pub version: Option<String>,
}

/// Manages app processes within the environment via systemd services.
pub struct AppSupervisor {
    apps: Vec<EnvAgentAppConfig>,
    env_type: EnvType,
    apps_path: String,
    db_path: String,
}

impl AppSupervisor {
    /// Create a new supervisor from the list of configured apps.
    pub fn new(apps: Vec<EnvAgentAppConfig>, env_type: EnvType, apps_path: String, db_path: String) -> Self {
        Self {
            apps,
            env_type,
            db_path,
            apps_path,
        }
    }

    /// Whether this is a development environment (watch services enabled).
    fn is_dev(&self) -> bool {
        self.env_type == EnvType::Development
    }

    /// Systemd watch service name for a given slug.
    fn watch_service_name(slug: &str) -> String {
        format!("{}-watch.service", slug)
    }

    /// Check if an app exists in the supervisor config.
    pub fn has_app(&self, slug: &str) -> bool {
        self.apps.iter().any(|a| a.slug == slug)
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

    /// Generate systemd unit files for all apps.
    /// Creates {slug}.service (run) and {slug}-watch.service (dev watch) for each app.
    pub fn generate_service_units(&self) -> Result<()> {
        let unit_dir = Path::new("/etc/systemd/system");

        for app in &self.apps {
            let working_dir = format!("{}/{}", self.apps_path, app.slug);

            // Create .dataverse/app.db symlink → env-agent DB for legacy app compatibility
            if app.has_db {
                let dataverse_dir = format!("{}/.dataverse", working_dir);
                let db_file = format!("{}/{}.db", self.db_path, app.slug);
                let _ = std::fs::create_dir_all(&dataverse_dir);
                let link_path = format!("{}/app.db", dataverse_dir);
                // Remove existing file/link and create fresh symlink
                let _ = std::fs::remove_file(&link_path);
                if let Err(e) = std::os::unix::fs::symlink(&db_file, &link_path) {
                    warn!(slug = %app.slug, error = %e, "failed to create .dataverse symlink");
                }
            }

            // --- Git init (if git_repo is configured and .git doesn't exist) ---
            if let Some(ref git_url) = app.git_repo {
                let git_dir = format!("{}/.git", working_dir);
                if !Path::new(&git_dir).exists() {
                    info!(slug = %app.slug, "initializing git repository");
                    let git = |args: &[&str]| {
                        std::process::Command::new("git")
                            .args(args)
                            .current_dir(&working_dir)
                            .output()
                    };
                    // Ensure safe.directory is set to avoid ownership issues
                    let _ = std::process::Command::new("git")
                        .args(["config", "--global", "--add", "safe.directory", &working_dir])
                        .output();
                    let _ = git(&["init"]);
                    let _ = git(&["remote", "add", "origin", git_url]);
                    let _ = git(&["config", "user.name", "env-agent"]);
                    let _ = git(&["config", "user.email", "env-agent@homeroute.local"]);

                    // Write .gitignore if not present
                    let gitignore_path = format!("{}/.gitignore", working_dir);
                    if !Path::new(&gitignore_path).exists() {
                        let _ = std::fs::write(&gitignore_path, GITIGNORE_CONTENT);
                    }

                    // Determine branch from remote, default to main
                    let branch = std::process::Command::new("git")
                        .args(["ls-remote", "--symref", "origin", "HEAD"])
                        .current_dir(&working_dir)
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .and_then(|s| {
                            s.lines()
                                .find(|l| l.contains("ref:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .map(|r| r.trim_start_matches("refs/heads/").to_string())
                        })
                        .unwrap_or_else(|| "main".to_string());
                    let _ = git(&["branch", "-m", &branch]);

                    // Initial commit + push
                    let _ = git(&["add", "-A"]);
                    if let Ok(output) = git(&["commit", "-m", &format!("initial commit: {}", app.slug)]) {
                        if output.status.success() {
                            let _ = git(&["push", "--force", "-u", "origin", &branch]);
                            info!(slug = %app.slug, branch = %branch, "Git initialized with initial commit");
                        }
                    }
                }
            }

            // --- Main service: {slug}.service ---
            let run_unit = format!(
                "[Unit]\n\
                 Description={name} app service\n\
                 After=network.target\n\
                 \n\
                 [Service]\n\
                 Type=simple\n\
                 WorkingDirectory={working_dir}\n\
                 ExecStart=/bin/bash -c \"{run_command}\"\n\
                 Restart=on-failure\n\
                 RestartSec=3\n\
                 Environment=NODE_ENV=production\n\
                 Environment=PORT={port}\n\
                 Environment=DATABASE_URL={db_path}/{slug}.db\n\
                 Environment=DATABASE_PATH={db_path}/{slug}.db\n\
                 Environment=DB_PATH={db_path}/{slug}.db\n\
                 Environment=STATIC_DIR=client/dist\n\
                 \n\
                 [Install]\n\
                 WantedBy=multi-user.target\n",
                name = app.name,
                slug = app.slug,
                working_dir = working_dir,
                run_command = app.run_command,
                port = app.port,
                db_path = self.db_path,
            );

            let svc_path = unit_dir.join(Self::service_name(&app.slug));
            write_unit_if_changed(&svc_path, &run_unit)?;

            // --- Watch service: {slug}-watch.service (dev only) ---
            if self.is_dev() {
                let watch_cmd = app
                    .watch_command
                    .as_deref()
                    .unwrap_or_else(|| app.stack.default_watch_command());

                let watch_unit = format!(
                    "[Unit]\n\
                     Description={name} watch (dev rebuild)\n\
                     After={slug}.service\n\
                     BindsTo={slug}.service\n\
                     \n\
                     [Service]\n\
                     Type=simple\n\
                     WorkingDirectory={working_dir}\n\
                     ExecStart=/bin/bash -c \"{watch_cmd}\"\n\
                     Restart=on-failure\n\
                     RestartSec=5\n\
                     Environment=NODE_ENV=development\n\
                     \n\
                     [Install]\n\
                     WantedBy=multi-user.target\n",
                    name = app.name,
                    slug = app.slug,
                    working_dir = working_dir,
                    watch_cmd = watch_cmd,
                );

                let watch_path = unit_dir.join(Self::watch_service_name(&app.slug));
                write_unit_if_changed(&watch_path, &watch_unit)?;
            }

            info!(slug = %app.slug, "service units generated");
        }

        Ok(())
    }

    /// Start an app's systemd service (+ watch service in dev).
    pub async fn start_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        debug!(slug, "starting app service");
        run_systemctl(&["start", &svc]).await?;
        if self.is_dev() {
            let watch_svc = Self::watch_service_name(slug);
            if service_exists(&watch_svc).await {
                debug!(slug, "starting watch service");
                if let Err(e) = run_systemctl(&["start", &watch_svc]).await {
                    warn!(slug, error = %e, "failed to start watch service (non-fatal)");
                }
            }
        }
        Ok(())
    }

    /// Stop an app's systemd service (+ watch service in dev).
    pub async fn stop_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        if self.is_dev() {
            let watch_svc = Self::watch_service_name(slug);
            if service_exists(&watch_svc).await {
                debug!(slug, "stopping watch service");
                let _ = run_systemctl(&["stop", &watch_svc]).await;
            }
        }
        let svc = Self::service_name(slug);
        debug!(slug, "stopping app service");
        run_systemctl(&["stop", &svc]).await
    }

    /// Restart an app's systemd service (+ watch service in dev).
    pub async fn restart_app(&self, slug: &str) -> Result<()> {
        let _ = self.find_app(slug)?;
        let svc = Self::service_name(slug);
        debug!(slug, "restarting app service");
        run_systemctl(&["restart", &svc]).await?;
        if self.is_dev() {
            let watch_svc = Self::watch_service_name(slug);
            if service_exists(&watch_svc).await {
                debug!(slug, "restarting watch service");
                if let Err(e) = run_systemctl(&["restart", &watch_svc]).await {
                    warn!(slug, error = %e, "failed to restart watch service (non-fatal)");
                }
            }
        }
        Ok(())
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

    /// Query the status of an app's watch service (dev only).
    pub async fn watch_status(&self, slug: &str) -> Option<AppProcessStatus> {
        if !self.is_dev() {
            return None;
        }
        let watch_svc = Self::watch_service_name(slug);
        if !service_exists(&watch_svc).await {
            return None;
        }
        let output = Command::new("systemctl")
            .args(["is-active", &watch_svc])
            .output()
            .await
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(parse_active_state(&stdout))
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
            let watch_status = self.watch_status(&app.slug).await;
            result.push(AppProcessInfo {
                slug: app.slug.clone(),
                name: app.name.clone(),
                stack: app.stack,
                port: app.port,
                status,
                watch_status,
                has_db: app.has_db,
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

/// Write a unit file only if the content has changed.
/// Default .gitignore for app repositories.
const GITIGNORE_CONTENT: &str = "\
node_modules/
target/
.next/
dist/
*.db
.dataverse/
CLAUDE.md
.claude/
.mcp.json
bin/
.pnpm-store/
";

fn write_unit_if_changed(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        if let Ok(existing) = std::fs::read_to_string(path) {
            if existing == content {
                return Ok(());
            }
        }
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// Check if a systemd service unit file exists.
async fn service_exists(service_name: &str) -> bool {
    let path = format!("/etc/systemd/system/{}", service_name);
    Path::new(&path).exists()
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
            watch_command: None,
            test_command: None,
            description: None,
            git_repo: None,
        }];
        let supervisor = AppSupervisor::new(apps, EnvType::Development, "/apps".to_string(), "/opt/env-agent/data/db".to_string());
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
