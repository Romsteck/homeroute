use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, warn, error};

const BACKUP_SERVER_IP: &str = "10.0.0.20";
const BACKUP_SERVER_USER: &str = "romain";
const BACKUP_SCRIPT: &str = "/backup/scripts/backup-homeroute.sh";
const WAKE_URL: &str = "http://10.0.0.254:4000/api/hosts/877bcb76-4fb8-4164-940c-707201adf9bc/wake";
const SLEEP_URL: &str = "http://10.0.0.254:4000/api/hosts/877bcb76-4fb8-4164-940c-707201adf9bc/sleep";

/// Wake the backup server via HomeRoute API
async fn wake_backup_server(client: &reqwest::Client) -> Result<()> {
    info!("Waking backup server via HomeRoute API...");
    let resp = client
        .post(WAKE_URL)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send wake request")?;

    if resp.status().is_success() {
        info!("Wake signal sent successfully");
    } else {
        warn!("Wake request returned status: {}", resp.status());
    }
    Ok(())
}

/// Sleep the backup server via HomeRoute API
async fn sleep_backup_server(client: &reqwest::Client) -> Result<()> {
    info!("Sending backup server to sleep via HomeRoute API...");
    let resp = client
        .post(SLEEP_URL)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send sleep request")?;

    if resp.status().is_success() {
        info!("Sleep signal sent successfully");
    } else {
        warn!("Sleep request returned status: {}", resp.status());
    }
    Ok(())
}

/// Wait for backup server SSH availability (max 3 minutes)
async fn wait_for_ssh(timeout_secs: u64) -> Result<()> {
    info!("Waiting for backup server SSH availability at {}...", BACKUP_SERVER_IP);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        // Try TCP connect to SSH port 22
        let connect = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect(format!("{}:22", BACKUP_SERVER_IP)),
        )
        .await;

        match connect {
            Ok(Ok(_)) => {
                info!("Backup server SSH port is reachable");
                // Extra wait for sshd to be fully ready
                tokio::time::sleep(Duration::from_secs(5)).await;
                return Ok(());
            }
            _ => {
                if tokio::time::Instant::now() >= deadline {
                    anyhow::bail!(
                        "Backup server at {} did not become available within {}s",
                        BACKUP_SERVER_IP,
                        timeout_secs
                    );
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
                info!("Still waiting for backup server SSH...");
            }
        }
    }
}

/// Run the backup script on the backup server via SSH
async fn run_backup_script() -> Result<String> {
    info!(
        "Running backup script {}@{}:{}",
        BACKUP_SERVER_USER, BACKUP_SERVER_IP, BACKUP_SCRIPT
    );

    let output = tokio::process::Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "ConnectTimeout=30",
            "-o", "BatchMode=yes",
            &format!("{}@{}", BACKUP_SERVER_USER, BACKUP_SERVER_IP),
            BACKUP_SCRIPT,
        ])
        .output()
        .await
        .context("Failed to execute SSH command")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stdout.is_empty() {
        info!("Backup stdout: {}", stdout.trim());
    }
    if !stderr.is_empty() {
        warn!("Backup stderr: {}", stderr.trim());
    }

    if output.status.success() {
        info!("Backup script completed successfully");
        Ok(stdout)
    } else {
        anyhow::bail!(
            "Backup script failed with exit code {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            stdout,
            stderr
        )
    }
}

/// Main backup pipeline:
/// 1. Wake backup server
/// 2. Wait for SSH availability (max 3 min)
/// 3. Run backup script
/// 4. Sleep backup server
/// 5. Log result
pub async fn run_backup() -> Result<()> {
    info!("=== Starting automated backup pipeline ===");

    let client = reqwest::Client::new();

    // Step 1: Wake backup server
    wake_backup_server(&client).await?;

    // Step 2: Wait for SSH (max 3 minutes = 180s)
    match wait_for_ssh(180).await {
        Ok(()) => {}
        Err(e) => {
            error!("Backup server did not come online: {}", e);
            // Attempt sleep even on failure (server may have partially woken)
            let _ = sleep_backup_server(&client).await;
            return Err(e);
        }
    }

    // Step 3: Run backup script
    let backup_result = run_backup_script().await;

    // Step 4: Sleep backup server (always, regardless of backup success)
    if let Err(e) = sleep_backup_server(&client).await {
        warn!("Failed to sleep backup server: {}", e);
    }

    // Step 5: Log result
    match backup_result {
        Ok(output) => {
            info!("=== Backup pipeline completed successfully ===");
            info!("Output summary: {}", output.lines().last().unwrap_or("(no output)"));
            Ok(())
        }
        Err(e) => {
            error!("=== Backup pipeline FAILED: {} ===", e);
            Err(e)
        }
    }
}
