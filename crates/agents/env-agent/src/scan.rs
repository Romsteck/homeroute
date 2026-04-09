use tokio::process::Command;
use tracing::{debug, warn};

pub struct ScanResult {
    pub os_upgradable: u32,
    pub os_security: u32,
    pub claude_cli_installed: Option<String>,
    pub code_server_installed: Option<String>,
    pub claude_ext_installed: Option<String>,
    pub scan_error: Option<String>,
}

pub async fn run_scan() -> ScanResult {
    let (apt, cli_inst, cs_inst, ext_inst) = tokio::join!(
        scan_apt(),
        scan_cmd("runuser -u studio -- /usr/local/bin/claude --version 2>/dev/null | head -1"),
        scan_cmd("/usr/local/bin/code-server --version 2>/dev/null | head -1"),
        scan_cmd("ls /home/studio/.local/share/code-server/extensions/ 2>/dev/null | grep claude-code | sort -V | tail -1"),
    );

    let (os_upgradable, os_security, scan_error) = apt;

    ScanResult {
        os_upgradable,
        os_security,
        claude_cli_installed: parse_version(&cli_inst.unwrap_or_default()),
        code_server_installed: parse_version(&cs_inst.unwrap_or_default()),
        claude_ext_installed: parse_ext_version(&ext_inst.unwrap_or_default()),
        scan_error,
    }
}

async fn scan_apt() -> (u32, u32, Option<String>) {
    let script = r#"
if [ -x /usr/lib/update-notifier/apt-check ]; then
    /usr/lib/update-notifier/apt-check 2>&1
else
    apt-get update -qq 2>/dev/null
    COUNT=$(apt list --upgradable 2>/dev/null | grep -c upgradable || true)
    SEC=$(apt list --upgradable 2>/dev/null | grep -i '\-security' | grep -c upgradable || true)
    echo "${COUNT};${SEC}"
fi
"#;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        Command::new("bash").args(["-c", script]).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let text = text.trim();
            if let Some((total, security)) = text.split_once(';') {
                let t = total.parse().unwrap_or(0);
                let s = security.parse().unwrap_or(0);
                (t, s, None)
            } else {
                debug!(output = %text, "apt scan unexpected format");
                (0, 0, Some(format!("apt scan unexpected: {text}")))
            }
        }
        Ok(Err(e)) => {
            warn!("apt scan failed: {e}");
            (0, 0, Some(format!("apt scan error: {e}")))
        }
        Err(_) => {
            warn!("apt scan timed out");
            (0, 0, Some("apt scan timed out".into()))
        }
    }
}

async fn scan_cmd(cmd: &str) -> Option<String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Command::new("bash").args(["-c", cmd]).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

/// Parse version from strings like "2.1.37 (Claude Code)" or "4.108.2 abc123..."
fn parse_version(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for token in s.split_whitespace() {
        if token.chars().next().map_or(false, |c| c.is_ascii_digit())
            && token.contains('.')
        {
            return Some(token.to_string());
        }
    }
    Some(s.split_whitespace().next()?.to_string())
}

/// Parse extension version from dir name like "anthropic.claude-code-2.1.69"
fn parse_ext_version(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.rsplitn(2, '-').collect();
    if let Some(ver) = parts.first() {
        let ver = ver.trim();
        if ver.chars().next().map_or(false, |c| c.is_ascii_digit()) && ver.contains('.') {
            return Some(ver.to_string());
        }
    }
    None
}
