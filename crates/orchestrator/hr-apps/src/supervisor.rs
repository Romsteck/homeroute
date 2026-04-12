use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::port_registry::PortRegistry;
use crate::registry::AppRegistry;
use crate::types::{AppState, Application};

const HEALTH_INTERVAL: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_GRACE: Duration = Duration::from_secs(10);
const RESTART_RESET_AFTER: Duration = Duration::from_secs(300);
const MAX_RESTARTS_PER_MIN: u32 = 10;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// Status snapshot of a supervised app process.
#[derive(Debug, Clone)]
pub struct ProcessStatus {
    pub pid: Option<u32>,
    pub state: AppState,
    pub port: u16,
    pub uptime_secs: u64,
    pub restart_count: u32,
}

/// Internal record for one supervised process.
struct SupervisedProcess {
    port: u16,
    pid: Option<u32>,
    state: AppState,
    started_at: Option<Instant>,
    restart_count: u32,
    /// Timestamps of restarts within the current sliding window.
    restart_history: Vec<Instant>,
    /// Backoff for the next respawn attempt.
    next_backoff: Duration,
    /// Time of the last successful start (used to reset backoff).
    last_start: Option<Instant>,
    /// Background task that owns the child + watches it.
    runner: Option<JoinHandle<()>>,
    /// Background task running the health-check loop.
    health: Option<JoinHandle<()>>,
    /// Whether stop() was requested (suppresses respawn).
    stop_requested: bool,
}

impl SupervisedProcess {
    fn new(port: u16) -> Self {
        Self {
            port,
            pid: None,
            state: AppState::Stopped,
            started_at: None,
            restart_count: 0,
            restart_history: Vec::new(),
            next_backoff: Duration::from_secs(1),
            last_start: None,
            runner: None,
            health: None,
            stop_requested: false,
        }
    }

    fn status(&self) -> ProcessStatus {
        let uptime_secs = self.started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        ProcessStatus {
            pid: self.pid,
            state: self.state,
            port: self.port,
            uptime_secs,
            restart_count: self.restart_count,
        }
    }
}

type ProcessMap = Arc<RwLock<HashMap<String, Arc<Mutex<SupervisedProcess>>>>>;

/// Supervises HomeRoute application processes (Tokio-spawned, host-local).
#[derive(Clone)]
pub struct AppSupervisor {
    pub registry: AppRegistry,
    pub port_registry: PortRegistry,
    processes: ProcessMap,
    app_state_tx: tokio::sync::broadcast::Sender<hr_common::events::AppStateEvent>,
}

impl AppSupervisor {
    pub fn new(
        registry: AppRegistry,
        port_registry: PortRegistry,
        app_state_tx: tokio::sync::broadcast::Sender<hr_common::events::AppStateEvent>,
    ) -> Self {
        Self {
            registry,
            port_registry,
            processes: Arc::new(RwLock::new(HashMap::new())),
            app_state_tx,
        }
    }

    /// Start an app: assign port, spawn process, attach health loop.
    pub async fn start(&self, slug: &str) -> Result<()> {
        let mut app = self
            .registry
            .get(slug)
            .await
            .ok_or_else(|| anyhow!("app not found: {slug}"))?;

        if app.port == 0 {
            let port = self
                .port_registry
                .assign(slug)
                .await
                .with_context(|| format!("assigning port for {slug}"))?;
            app.port = port;
            self.registry.upsert(app.clone()).await.ok();
        }

        let proc_arc = self.get_or_create_process(slug, app.port).await;
        {
            let proc = proc_arc.lock().await;
            if matches!(proc.state, AppState::Running | AppState::Starting) {
                info!(app_slug = slug, "start: already running");
                return Ok(());
            }
        }

        if !port_is_free(app.port).await {
            warn!(app_slug = slug, port = app.port, "start: port not free");
            return Err(anyhow!("port {} is already in use", app.port));
        }

        self.spawn_runner(app, proc_arc.clone()).await;
        Ok(())
    }

    /// Stop an app: SIGTERM with grace period, then SIGKILL.
    pub async fn stop(&self, slug: &str) -> Result<()> {
        let proc_arc = match self.processes.read().await.get(slug).cloned() {
            Some(p) => p,
            None => {
                info!(app_slug = slug, "stop: not supervised");
                return Ok(());
            }
        };

        let (pid_opt, runner, health) = {
            let mut proc = proc_arc.lock().await;
            proc.stop_requested = true;
            (proc.pid, proc.runner.take(), proc.health.take())
        };

        if let Some(h) = health {
            h.abort();
        }

        if let Some(pid) = pid_opt {
            info!(app_slug = slug, pid, "stop: sending SIGTERM");
            send_signal(pid, Signal::SIGTERM);
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                if !pid_alive(pid) {
                    break;
                }
                sleep(Duration::from_millis(200)).await;
            }
            if pid_alive(pid) {
                warn!(app_slug = slug, pid, "stop: SIGTERM grace expired, SIGKILL");
                send_signal(pid, Signal::SIGKILL);
            }
        }

        if let Some(r) = runner {
            r.abort();
            let _ = r.await;
        }

        {
            let mut proc = proc_arc.lock().await;
            proc.state = AppState::Stopped;
            proc.pid = None;
            proc.started_at = None;
        }

        self.update_app_state(slug, AppState::Stopped).await;
        Ok(())
    }

    /// Restart an app (stop then start).
    pub async fn restart(&self, slug: &str) -> Result<()> {
        info!(app_slug = slug, "restart");
        self.stop(slug).await?;
        sleep(Duration::from_millis(200)).await;
        {
            let processes = self.processes.read().await;
            if let Some(proc_arc) = processes.get(slug) {
                let mut proc = proc_arc.lock().await;
                proc.stop_requested = false;
            }
        }
        self.start(slug).await
    }

    /// Status snapshot for one supervised app.
    pub async fn status(&self, slug: &str) -> Option<ProcessStatus> {
        let processes = self.processes.read().await;
        let proc_arc = processes.get(slug)?.clone();
        drop(processes);
        let proc = proc_arc.lock().await;
        Some(proc.status())
    }

    /// Start every app whose persisted state is `Running`.
    pub async fn start_all_running(&self) -> Result<()> {
        let apps = self.registry.list().await;
        for app in apps {
            if matches!(app.state, AppState::Running | AppState::Starting) {
                if let Err(e) = self.start(&app.slug).await {
                    error!(app_slug = %app.slug, error = %e, "start_all_running failed");
                }
            }
        }
        Ok(())
    }

    /// Stop every supervised app.
    pub async fn shutdown_all(&self) -> Result<()> {
        let slugs: Vec<String> = self.processes.read().await.keys().cloned().collect();
        for slug in slugs {
            if let Err(e) = self.stop(&slug).await {
                error!(app_slug = %slug, error = %e, "shutdown_all failed");
            }
        }
        Ok(())
    }

    async fn get_or_create_process(&self, slug: &str, port: u16) -> Arc<Mutex<SupervisedProcess>> {
        {
            let processes = self.processes.read().await;
            if let Some(p) = processes.get(slug) {
                return p.clone();
            }
        }
        let mut processes = self.processes.write().await;
        processes
            .entry(slug.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(SupervisedProcess::new(port))))
            .clone()
    }

    async fn update_app_state(&self, slug: &str, state: AppState) {
        if let Some(mut app) = self.registry.get(slug).await {
            if app.state != state {
                app.state = state;
                if let Err(e) = self.registry.upsert(app.clone()).await {
                    warn!(app_slug = slug, error = %e, "update_app_state persist failed");
                }
                // Emit real-time event
                let proc_status = {
                    let procs = self.processes.read().await;
                    procs.get(slug).map(|p| {
                        let p = p.try_lock();
                        p.ok().map(|p| p.status())
                    }).flatten()
                };
                let event = hr_common::events::AppStateEvent {
                    slug: slug.to_string(),
                    state: format!("{:?}", state).to_lowercase(),
                    pid: proc_status.as_ref().and_then(|s| s.pid),
                    port: app.port,
                    uptime_secs: proc_status.as_ref().map(|s| s.uptime_secs).unwrap_or(0),
                    restart_count: proc_status.as_ref().map(|s| s.restart_count).unwrap_or(0),
                };
                let _ = self.app_state_tx.send(event);
            }
        }
    }

    async fn spawn_runner(&self, app: Application, proc_arc: Arc<Mutex<SupervisedProcess>>) {
        let supervisor = self.clone();
        let slug = app.slug.clone();
        let handle = tokio::spawn(async move {
            supervisor.runner_loop(app, proc_arc).await;
        });
        if let Some(p) = self.processes.read().await.get(&slug) {
            let mut proc = p.lock().await;
            proc.runner = Some(handle);
        }
    }

    async fn runner_loop(&self, app: Application, proc_arc: Arc<Mutex<SupervisedProcess>>) {
        let slug = app.slug.clone();
        loop {
            {
                let proc = proc_arc.lock().await;
                if proc.stop_requested {
                    debug!(app_slug = %slug, "runner_loop: stop requested, exiting");
                    return;
                }
            }

            {
                let mut proc = proc_arc.lock().await;
                proc.state = AppState::Starting;
            }
            self.update_app_state(&slug, AppState::Starting).await;

            let child_res = spawn_child(&app).await;
            let mut child = match child_res {
                Ok(c) => c,
                Err(e) => {
                    error!(app_slug = %slug, error = %e, "spawn failed");
                    {
                        let mut proc = proc_arc.lock().await;
                        proc.state = AppState::Crashed;
                    }
                    self.update_app_state(&slug, AppState::Crashed).await;
                    if !self.bump_restart_and_should_continue(&proc_arc).await {
                        return;
                    }
                    let backoff = self.current_backoff(&proc_arc).await;
                    sleep(backoff).await;
                    continue;
                }
            };

            let pid = child.id();
            info!(app_slug = %slug, ?pid, "process spawned");

            if let Some(stdout) = child.stdout.take() {
                let s = slug.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        info!(app_slug = %s, level = "stdout", "{line}");
                    }
                });
            }
            if let Some(stderr) = child.stderr.take() {
                let s = slug.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        warn!(app_slug = %s, level = "stderr", "{line}");
                    }
                });
            }

            {
                let mut proc = proc_arc.lock().await;
                proc.pid = pid;
                proc.state = AppState::Running;
                proc.started_at = Some(Instant::now());
                proc.last_start = Some(Instant::now());
                if let Some(h) = proc.health.take() {
                    h.abort();
                }
                proc.health =
                    Some(self.spawn_health_loop(slug.clone(), app.port, app.health_path.clone()));
            }
            self.update_app_state(&slug, AppState::Running).await;

            let exit_status = child.wait().await;

            let stop_requested = {
                let mut proc = proc_arc.lock().await;
                if let Some(h) = proc.health.take() {
                    h.abort();
                }
                proc.pid = None;
                proc.started_at = None;
                proc.stop_requested
            };

            match exit_status {
                Ok(status) => {
                    info!(app_slug = %slug, code = ?status.code(), "process exited");
                }
                Err(e) => {
                    warn!(app_slug = %slug, error = %e, "process wait failed");
                }
            }

            if stop_requested {
                let mut proc = proc_arc.lock().await;
                proc.state = AppState::Stopped;
                self.update_app_state(&slug, AppState::Stopped).await;
                return;
            }

            {
                let mut proc = proc_arc.lock().await;
                proc.state = AppState::Crashed;
            }
            self.update_app_state(&slug, AppState::Crashed).await;

            if !self.bump_restart_and_should_continue(&proc_arc).await {
                error!(app_slug = %slug, "restart limit exceeded, giving up");
                return;
            }

            let backoff = self.current_backoff(&proc_arc).await;
            warn!(app_slug = %slug, backoff_ms = backoff.as_millis() as u64, "respawning after backoff");
            sleep(backoff).await;
        }
    }

    async fn bump_restart_and_should_continue(
        &self,
        proc_arc: &Arc<Mutex<SupervisedProcess>>,
    ) -> bool {
        let mut proc = proc_arc.lock().await;
        if let Some(last) = proc.last_start {
            if last.elapsed() >= RESTART_RESET_AFTER {
                proc.next_backoff = Duration::from_secs(1);
            }
        }
        proc.restart_count += 1;
        let now = Instant::now();
        proc.restart_history
            .retain(|t| now.duration_since(*t) < RESTART_WINDOW);
        proc.restart_history.push(now);
        if proc.restart_history.len() as u32 > MAX_RESTARTS_PER_MIN {
            proc.state = AppState::Crashed;
            return false;
        }
        true
    }

    async fn current_backoff(&self, proc_arc: &Arc<Mutex<SupervisedProcess>>) -> Duration {
        let mut proc = proc_arc.lock().await;
        let current = proc.next_backoff;
        let next_secs = (current.as_secs() * 2).min(30).max(1);
        proc.next_backoff = Duration::from_secs(next_secs);
        current
    }

    fn spawn_health_loop(&self, slug: String, port: u16, path: String) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut consecutive_failures = 0u32;
            loop {
                sleep(HEALTH_INTERVAL).await;
                let ok = http_health_check(port, &path).await;
                if ok {
                    if consecutive_failures > 0 {
                        info!(app_slug = %slug, port, "health recovered");
                    }
                    consecutive_failures = 0;
                } else {
                    consecutive_failures += 1;
                    if consecutive_failures >= 3 {
                        warn!(
                            app_slug = %slug,
                            port,
                            failures = consecutive_failures,
                            "health check failing"
                        );
                    }
                }
            }
        })
    }
}

async fn spawn_child(app: &Application) -> Result<Child> {
    let src_dir = app.src_dir();
    let env_file = app.env_file();

    let mut cmd = Command::new("/bin/bash");
    cmd.arg("-c").arg(&app.run_command);
    cmd.current_dir(&src_dir);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    cmd.env("PORT", app.port.to_string());
    cmd.env("DATABASE_PATH", app.db_path().to_string_lossy().to_string());
    cmd.env("DB_PATH", app.db_path().to_string_lossy().to_string());

    for (k, v) in &app.env_vars {
        cmd.env(k, v);
    }

    if env_file.exists() {
        match load_env_file(&env_file).await {
            Ok(vars) => {
                for (k, v) in vars {
                    cmd.env(k, v);
                }
            }
            Err(e) => {
                warn!(app_slug = %app.slug, path = %env_file.display(), error = %e, "env file load failed");
            }
        }
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("spawning {} in {}", app.slug, src_dir.display()))?;
    Ok(child)
}

async fn load_env_file(path: &PathBuf) -> Result<Vec<(String, String)>> {
    let bytes = tokio::fs::read(path).await?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_string();
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !k.is_empty() {
                out.push((k, v));
            }
        }
    }
    Ok(out)
}

async fn port_is_free(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).await.is_err()
}

async fn http_health_check(port: u16, path: &str) -> bool {
    let connect = TcpStream::connect(("127.0.0.1", port));
    let stream_res = tokio::time::timeout(HEALTH_TIMEOUT, connect).await;
    let mut stream = match stream_res {
        Ok(Ok(s)) => s,
        _ => return false,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        path, port
    );
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    let read = tokio::time::timeout(HEALTH_TIMEOUT, stream.read(&mut buf)).await;
    let n = match read {
        Ok(Ok(n)) if n > 0 => n,
        _ => return false,
    };
    let resp = String::from_utf8_lossy(&buf[..n]);
    let line = match resp.lines().next() {
        Some(l) => l,
        None => return false,
    };
    if !line.starts_with("HTTP/") {
        return false;
    }
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .map(|c| (200..400).contains(&c))
        .unwrap_or(false)
}

fn send_signal(pid: u32, sig: Signal) {
    if let Err(e) = kill(Pid::from_raw(pid as i32), sig) {
        debug!(pid, ?sig, error = %e, "kill failed");
    }
}

fn pid_alive(pid: u32) -> bool {
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn load_env_file_parses_simple_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        tokio::fs::write(&path, "FOO=bar\n# comment\nBAZ=\"quoted\"\n\nEMPTY=\n")
            .await
            .unwrap();
        let pb = path.to_path_buf();
        let vars = load_env_file(&pb).await.unwrap();
        assert!(vars.iter().any(|(k, v)| k == "FOO" && v == "bar"));
        assert!(vars.iter().any(|(k, v)| k == "BAZ" && v == "quoted"));
        assert!(vars.iter().any(|(k, v)| k == "EMPTY" && v.is_empty()));
        assert!(!vars.iter().any(|(k, _)| k == "# comment"));
    }
}
