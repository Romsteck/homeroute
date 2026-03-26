use tokio::process::Command;
use tracing::{debug, warn};

pub struct SshClient {
    target: String,
}

#[derive(Debug)]
pub struct ExecResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl SshClient {
    pub fn new(user: &str, server: &str) -> Self {
        Self {
            target: format!("{user}@{server}"),
        }
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn from_config(config: &crate::config::Config) -> Self {
        let user = config.prod_user.as_deref().unwrap_or("romain");
        let server = config.prod_server.as_deref().unwrap_or("10.0.0.20");
        Self::new(user, server)
    }

    pub async fn exec(&self, cmd: &str) -> Result<ExecResult, String> {
        debug!(target = %self.target, cmd, "SSH exec");
        let output = Command::new("ssh")
            .args([
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=10",
                &self.target,
                cmd,
            ])
            .output()
            .await
            .map_err(|e| format!("SSH exec failed: {e}"))?;

        let result = ExecResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };

        if !result.success {
            warn!(
                target = %self.target,
                cmd,
                stderr = %result.stderr.trim(),
                "SSH command failed"
            );
        }
        Ok(result)
    }

    pub async fn exec_in_container(
        &self,
        container: &str,
        cmd: &str,
    ) -> Result<ExecResult, String> {
        let inner = format!(
            "PID=$(machinectl show {container} -p Leader --value) && nsenter -t $PID -m -u -i -n -p -- bash -c {}",
            shell_quote(cmd)
        );
        self.exec(&format!("sudo bash -c {}", shell_quote(&inner))).await
    }

    pub async fn scp(&self, local_path: &str, remote_path: &str) -> Result<(), String> {
        debug!(local = local_path, remote = remote_path, "SCP transfer");
        let output = Command::new("scp")
            .args([
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=10",
                local_path,
                &format!("{}:{}", self.target, remote_path),
            ])
            .output()
            .await
            .map_err(|e| format!("SCP failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("SCP failed: {stderr}"));
        }
        Ok(())
    }

    pub async fn copy_to_container(
        &self,
        container: &str,
        host_path: &str,
        container_path: &str,
    ) -> Result<(), String> {
        self.exec(&format!(
            "sudo machinectl copy-to {container} {host_path} {container_path}"
        ))
        .await
        .and_then(|r| {
            if r.success {
                Ok(())
            } else {
                Err(format!("machinectl copy-to failed: {}", r.stderr.trim()))
            }
        })
    }

    pub async fn rsync(&self, local_path: &str, remote_path: &str) -> Result<(), String> {
        debug!(local = local_path, remote = remote_path, "rsync transfer");
        let output = Command::new("rsync")
            .args([
                "-az",
                "--delete",
                local_path,
                &format!("{}:{}", self.target, remote_path),
            ])
            .output()
            .await
            .map_err(|e| format!("rsync failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("rsync failed: {stderr}"));
        }
        Ok(())
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
