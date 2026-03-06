use tokio::process::Command;
use tracing::{debug, warn};

pub struct ScanResult {
    pub os_upgradable: u32,
    pub os_security: u32,
    pub claude_cli_installed: Option<String>,
    pub claude_cli_latest: Option<String>,
    pub code_server_installed: Option<String>,
    pub code_server_latest: Option<String>,
    pub claude_ext_installed: Option<String>,
    pub claude_ext_latest: Option<String>,
    pub scan_error: Option<String>,
}

pub async fn run_scan(is_dev: bool) -> ScanResult {
    let apt_fut = scan_apt();

    if is_dev {
        let (apt, cli_inst, cli_lat, cs_inst, cs_lat, ext_inst, ext_lat) = tokio::join!(
            apt_fut,
            scan_cmd("runuser -u studio -- /usr/local/bin/claude --version 2>/dev/null | head -1"),
            scan_claude_cli_latest(),
            scan_cmd("/usr/local/bin/code-server --version 2>/dev/null | head -1"),
            scan_code_server_latest(),
            scan_cmd("ls /home/studio/.local/share/code-server/extensions/ 2>/dev/null | grep claude-code | sort -V | tail -1"),
            scan_claude_ext_latest(),
        );

        let (os_upgradable, os_security, scan_error) = apt;

        ScanResult {
            os_upgradable,
            os_security,
            claude_cli_installed: parse_version(&cli_inst.unwrap_or_default()),
            claude_cli_latest: cli_lat,
            code_server_installed: parse_version(&cs_inst.unwrap_or_default()),
            code_server_latest: cs_lat,
            claude_ext_installed: parse_ext_version(&ext_inst.unwrap_or_default()),
            claude_ext_latest: ext_lat,
            scan_error,
        }
    } else {
        let (os_upgradable, os_security, scan_error) = apt_fut.await;

        ScanResult {
            os_upgradable,
            os_security,
            claude_cli_installed: None,
            claude_cli_latest: None,
            code_server_installed: None,
            code_server_latest: None,
            claude_ext_installed: None,
            claude_ext_latest: None,
            scan_error,
        }
    }
}

async fn scan_apt() -> (u32, u32, Option<String>) {
    // Try apt-check first, fall back to `apt list --upgradable`
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
            // Format: "N;M" where N=total upgradable, M=security
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
    // Take the first whitespace-delimited token that looks like a version
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
    // Find last occurrence of a version pattern at end: digits.digits.digits
    let parts: Vec<&str> = s.rsplitn(2, '-').collect();
    if let Some(ver) = parts.first() {
        let ver = ver.trim();
        if ver.chars().next().map_or(false, |c| c.is_ascii_digit()) && ver.contains('.') {
            return Some(ver.to_string());
        }
    }
    None
}

const GCS_BASE: &str = "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases";

async fn scan_claude_cli_latest() -> Option<String> {
    let url = format!("{GCS_BASE}/latest");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        reqwest::get(&url),
    )
    .await;

    match result {
        Ok(Ok(resp)) => {
            let text = resp.text().await.ok()?;
            let text = text.trim().to_string();
            if text.is_empty() || text.len() > 20 { None } else { Some(text) }
        }
        _ => None,
    }
}

async fn scan_code_server_latest() -> Option<String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        reqwest::Client::new()
            .get("https://api.github.com/repos/coder/code-server/releases/latest")
            .header("User-Agent", "hr-agent")
            .send(),
    )
    .await;

    match result {
        Ok(Ok(resp)) => {
            let json: serde_json::Value = resp.json().await.ok()?;
            let tag = json.get("tag_name")?.as_str()?;
            Some(tag.strip_prefix('v').unwrap_or(tag).to_string())
        }
        _ => None,
    }
}

async fn scan_claude_ext_latest() -> Option<String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        reqwest::Client::new()
            .get("https://open-vsx.org/api/Anthropic/claude-code/latest")
            .header("User-Agent", "hr-agent")
            .send(),
    )
    .await;

    match result {
        Ok(Ok(resp)) => {
            let json: serde_json::Value = resp.json().await.ok()?;
            json.get("version")?.as_str().map(|s| s.to_string())
        }
        _ => None,
    }
}
