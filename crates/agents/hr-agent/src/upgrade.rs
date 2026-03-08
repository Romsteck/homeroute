use tokio::process::Command;
use tracing::{info, error};

pub async fn run_upgrade(category: &str) -> Result<(), String> {
    info!(category, "Starting upgrade");

    let cmd = match category {
        "apt" => "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get upgrade -y",
        "claude_cli" => concat!(
            "GCS=https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases",
            " && VERSION=$(curl -4 -fsSL $GCS/latest)",
            " && ARCH=$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')",
            " && curl -4 -fsSL -o /usr/local/bin/claude $GCS/$VERSION/linux-$ARCH/claude",
            " && chmod +x /usr/local/bin/claude",
        ),
        "code_server" => "curl -4 -fsSL https://code-server.dev/install.sh | sh -s -- --method=standalone --prefix=/usr/local && systemctl restart code-server",
        "claude_ext" => "/usr/local/bin/update-claude-ext.sh && systemctl restart code-server",
        _ => return Err(format!("Unknown upgrade category: {category}")),
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        Command::new("bash").args(["-c", cmd]).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            info!(category, "Upgrade completed successfully");
            Ok(())
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(category, stderr = %stderr, "Upgrade failed");
            Err(format!("Upgrade failed: {stderr}"))
        }
        Ok(Err(e)) => {
            error!(category, error = %e, "Upgrade command error");
            Err(format!("Command error: {e}"))
        }
        Err(_) => {
            error!(category, "Upgrade timed out (600s)");
            Err("Upgrade timed out".into())
        }
    }
}
