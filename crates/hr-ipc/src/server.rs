use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info, warn};

use crate::types::{IpcRequest, IpcResponse};

/// Trait implemented by hr-netcore to dispatch IPC commands.
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, request: IpcRequest) -> impl std::future::Future<Output = IpcResponse> + Send;
}

/// Start the IPC Unix socket server.
/// Removes stale socket at startup.
pub async fn run_ipc_server<H: IpcHandler>(
    socket_path: &Path,
    handler: Arc<H>,
) -> Result<()> {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(socket_path)?;
    info!(path = %socket_path.display(), "IPC server listening");

    // Set socket permissions to 0660
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660));
    }

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let handler = handler.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, handler).await {
                        warn!("IPC connection error: {e:#}");
                    }
                });
            }
            Err(e) => {
                error!("IPC accept error: {e}");
            }
        }
    }
}

async fn handle_connection<H: IpcHandler>(
    stream: tokio::net::UnixStream,
    handler: Arc<H>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    buf_reader.read_line(&mut line).await?;

    if line.is_empty() {
        return Ok(());
    }

    let response = match serde_json::from_str::<IpcRequest>(line.trim()) {
        Ok(request) => handler.handle(request).await,
        Err(e) => IpcResponse::err(format!("Invalid request: {e}")),
    };

    let mut resp_line = serde_json::to_string(&response)?;
    resp_line.push('\n');
    writer.write_all(resp_line.as_bytes()).await?;
    writer.shutdown().await?;

    Ok(())
}
