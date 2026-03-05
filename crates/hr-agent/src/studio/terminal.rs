use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::types::{SessionInfo, WsOutMessage};
use super::{StudioBridge, TerminalSession};

type WriterMap = HashMap<String, Arc<tokio::sync::Mutex<OwnedFd>>>;

const STUDIO_USER: &str = "studio";
const STUDIO_HOME: &str = "/home/studio";
const WORKSPACE_ROOT: &str = "/root/workspace";

/// Create a PTY pair with the given initial size.
/// Returns (master_fd, slave_fd).
fn create_pty(cols: u16, rows: u16) -> Result<(OwnedFd, OwnedFd), String> {
    unsafe {
        let mut master: libc::c_int = 0;
        let mut slave: libc::c_int = 0;

        if libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
            return Err("openpty() failed".to_string());
        }

        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        libc::ioctl(master, libc::TIOCSWINSZ, &ws);

        Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)))
    }
}

/// Resize a PTY by its master fd.
fn pty_resize(fd: &OwnedFd, cols: u16, rows: u16) {
    unsafe {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ, &ws);
    }
}

/// Run a tmux command as the studio user.
async fn tmux_as_studio(args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd_args = vec!["-u", STUDIO_USER, "--", "tmux"];
    cmd_args.extend_from_slice(args);
    tokio::process::Command::new("runuser")
        .args(&cmd_args)
        .output()
        .await
}

/// Build the PATH and env args for the studio user.
fn build_studio_env() -> Vec<String> {
    let mut extra_paths = vec![
        "/root/.cargo/bin".to_string(),
        "/root/.local/bin".to_string(),
    ];
    let nvm_dir = std::path::Path::new("/root/.nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(nvm_dir) {
        if let Some(entry) = entries.filter_map(|e| e.ok()).last() {
            extra_paths.insert(0, format!("{}/bin", entry.path().display()));
        }
    }
    let sys_path = std::env::var("PATH").unwrap_or_default();
    let full_path = format!("{}:{}", extra_paths.join(":"), sys_path);

    vec![
        format!("HOME={STUDIO_HOME}"),
        format!("PATH={full_path}"),
        "TERM=xterm-256color".to_string(),
        "LANG=C.UTF-8".to_string(),
        "LC_ALL=C.UTF-8".to_string(),
    ]
}

/// Spawn a terminal session with tmux + claude interactive.
pub async fn spawn_terminal(
    studio: &Arc<StudioBridge>,
    session_id: &str,
    resume_session: Option<&str>,
    model: Option<&str>,
    cols: u16,
    rows: u16,
    user: Option<&str>,
    out_tx: mpsc::Sender<WsOutMessage>,
) -> Result<(), String> {
    let tmux_name = format!("studio-{}", session_id);

    // Check if tmux session already exists (reconnection case)
    let tmux_exists = tmux_as_studio(&["has-session", "-t", &tmux_name])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tmux_exists {
        let mut claude_cmd = String::from("claude");
        if let Some(resume) = resume_session {
            claude_cmd.push_str(&format!(" --resume {}", resume));
        }
        if let Some(m) = model {
            claude_cmd.push_str(&format!(" --model {}", m));
        }

        let env_args = build_studio_env();
        let mut env_prefix = String::new();
        for arg in &env_args {
            env_prefix.push_str(arg);
            env_prefix.push(' ');
        }

        // Pass studio session ID so MCP tools can broadcast todos to the right tab
        env_prefix.push_str(&format!("STUDIO_SESSION_ID={} ", session_id));

        // Add token env var if user is using token auth
        if let Some(username) = user {
            if let Some(cred) = super::credentials::get_auth_status(username).await {
                if cred.method == "token" {
                    if let Some(ref token) = cred.token {
                        env_prefix.push_str(&format!("CLAUDE_CODE_OAUTH_TOKEN={} ", token));
                    }
                }
            }
        }

        let tmux_cmd = format!(
            "runuser -u {} -- env {} tmux -f /dev/null new-session -d -s {} -x {} -y {} \\; set status off \\; send-keys '{}' Enter",
            STUDIO_USER, env_prefix.trim(), tmux_name, cols, rows, claude_cmd
        );
        info!("Spawning tmux: {}", tmux_cmd);

        let output = tokio::process::Command::new("bash")
            .args(["-c", &tmux_cmd])
            .current_dir(WORKSPACE_ROOT)
            .output()
            .await
            .map_err(|e| format!("Failed to create tmux session: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tmux new-session failed: {stderr}"));
        }

        info!("Created tmux session {} for terminal {}", tmux_name, session_id);
    } else {
        info!("Reconnecting to existing tmux session {}", tmux_name);
    }

    // Create a real PTY with the correct initial size
    let (master_fd, slave_fd) = create_pty(cols, rows)?;

    // Pass the slave fd directly to the child process as stdin/stdout/stderr.
    // We use pre_exec to call setsid() so the child gets a new session and
    // the slave PTY becomes its controlling terminal via TIOCSCTTY.
    let slave_raw = slave_fd.as_raw_fd();
    let stdin_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_raw)) };
    let stdout_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_raw)) };
    let stderr_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_raw)) };

    let _child = unsafe {
        tokio::process::Command::new("runuser")
            .args(["-u", STUDIO_USER, "--", "tmux", "attach-session", "-t", &tmux_name])
            .stdin(std::process::Stdio::from(std::fs::File::from(std::os::fd::OwnedFd::from(stdin_fd))))
            .stdout(std::process::Stdio::from(std::fs::File::from(std::os::fd::OwnedFd::from(stdout_fd))))
            .stderr(std::process::Stdio::from(std::fs::File::from(std::os::fd::OwnedFd::from(stderr_fd))))
            .env("TERM", "xterm-256color")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .current_dir(WORKSPACE_ROOT)
            .pre_exec(|| {
                // Create a new session — makes this process the session leader
                // so the PTY slave becomes the controlling terminal.
                libc::setsid();
                // Explicitly acquire controlling terminal
                libc::ioctl(0, libc::TIOCSCTTY, 0);
                Ok(())
            })
            .spawn()
            .map_err(|e| format!("Failed to spawn tmux attach: {e}"))?
    };

    // Drop slave fd in parent — the child has its own copies
    drop(slave_fd);

    // Make master fd non-blocking for tokio
    unsafe {
        let flags = libc::fcntl(master_fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(master_fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    // Dup the master fd for the reader (the original stays for writing + resize)
    let reader_fd = unsafe {
        let fd = libc::dup(master_fd.as_raw_fd());
        if fd < 0 {
            return Err("Failed to dup master fd".to_string());
        }
        OwnedFd::from_raw_fd(fd)
    };

    let async_reader = tokio::io::unix::AsyncFd::new(reader_fd)
        .map_err(|e| format!("Failed to wrap master fd: {e}"))?;

    // Spawn reader task: PTY master -> base64 -> WsOutMessage
    let session_id_owned = session_id.to_string();
    let reader_handle = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match async_reader.readable().await {
                Ok(mut guard) => {
                    match guard.try_io(|inner| {
                        let n = unsafe {
                            libc::read(
                                inner.as_raw_fd(),
                                buf.as_mut_ptr() as *mut libc::c_void,
                                buf.len(),
                            )
                        };
                        if n < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(n as usize)
                        }
                    }) {
                        Ok(Ok(0)) => {
                            debug!("PTY EOF for {}", session_id_owned);
                            break;
                        }
                        Ok(Ok(n)) => {
                            use base64::Engine;
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                            if out_tx.send(WsOutMessage::TerminalData {
                                session_id: session_id_owned.clone(),
                                data: encoded,
                            }).await.is_err() {
                                break;
                            }
                        }
                        Ok(Err(e)) if e.raw_os_error() == Some(libc::EIO) => {
                            debug!("PTY EIO for {} (child exited)", session_id_owned);
                            break;
                        }
                        Ok(Err(e)) => {
                            warn!("PTY read error for {}: {e}", session_id_owned);
                            break;
                        }
                        Err(_would_block) => continue,
                    }
                }
                Err(e) => {
                    warn!("PTY readable error for {}: {e}", session_id_owned);
                    break;
                }
            }
        }
    });

    let reader_abort = reader_handle.abort_handle();

    // Spawn watchdog
    let studio_wd = Arc::clone(studio);
    let session_id_wd = session_id.to_string();
    let tmux_name_wd = tmux_name.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let alive = tmux_as_studio(&["has-session", "-t", &tmux_name_wd])
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);

            if !alive {
                info!("Terminal {} tmux died, cleaning up", session_id_wd);
                cleanup_writer(&session_id_wd).await;
                studio_wd.terminal_sessions.write().await.remove(&session_id_wd);
                studio_wd.broadcast_all(&WsOutMessage::TerminalDone {
                    session_id: session_id_wd,
                }).await;
                break;
            }

            if !studio_wd.is_terminal_active(&session_id_wd).await {
                break;
            }
        }
    });

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    studio.terminal_sessions.write().await.insert(
        session_id.to_string(),
        TerminalSession {
            tmux_name,
            reader_abort,
            created_at,
        },
    );

    // Store master fd for writing input and resizing
    TERMINAL_WRITERS
        .lock()
        .await
        .insert(session_id.to_string(), Arc::new(tokio::sync::Mutex::new(master_fd)));

    info!("Terminal session {} fully started", session_id);
    Ok(())
}

/// Global map of PTY master fds, keyed by session_id.
static TERMINAL_WRITERS: std::sync::LazyLock<tokio::sync::Mutex<WriterMap>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// Write input data (base64-encoded) to a terminal session's PTY master.
pub async fn write_input(session_id: &str, data: &str) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("Invalid base64: {e}"))?;

    let writers = TERMINAL_WRITERS.lock().await;
    let writer = writers.get(session_id)
        .ok_or_else(|| format!("No terminal writer for session {session_id}"))?;

    let fd = writer.lock().await;
    let raw_fd = fd.as_raw_fd();
    let mut written = 0;
    while written < bytes.len() {
        let n = unsafe {
            libc::write(raw_fd, bytes[written..].as_ptr() as *const libc::c_void, bytes.len() - written)
        };
        if n < 0 {
            return Err(format!("PTY write error: {}", std::io::Error::last_os_error()));
        }
        written += n as usize;
    }

    Ok(())
}

/// Resize a terminal session — both the PTY and tmux window.
pub async fn resize(session_id: &str, cols: u16, rows: u16, studio: &Arc<StudioBridge>) -> Result<(), String> {
    // Resize tmux window first
    let sessions = studio.terminal_sessions.read().await;
    if let Some(ts) = sessions.get(session_id) {
        let cols_str = cols.to_string();
        let rows_str = rows.to_string();
        let _ = tmux_as_studio(&[
            "resize-window", "-t", &ts.tmux_name,
            "-x", &cols_str,
            "-y", &rows_str,
        ]).await;
    }
    drop(sessions);

    // Resize the PTY master fd — this sends SIGWINCH to the child
    let writers = TERMINAL_WRITERS.lock().await;
    if let Some(writer) = writers.get(session_id) {
        let fd = writer.lock().await;
        pty_resize(&fd, cols, rows);
    }

    Ok(())
}

/// Clean up a terminal session's writer.
pub async fn cleanup_writer(session_id: &str) {
    TERMINAL_WRITERS.lock().await.remove(session_id);
}

/// Get terminal sessions as SessionInfo for the ListSessions response.
pub async fn list_terminal_sessions(studio: &Arc<StudioBridge>) -> Vec<SessionInfo> {
    let sessions = studio.terminal_sessions.read().await;
    sessions.iter().map(|(id, ts)| {
        SessionInfo {
            session_id: id.clone(),
            project: String::new(),
            last_modified: ts.created_at,
            message_count: 0,
            summary: String::new(),
            session_type: "cli".to_string(),
        }
    }).collect()
}
