use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info, warn};

/// Trait implemented by service handlers to dispatch IPC commands.
///
/// Generic over request/response types so the same server infrastructure
/// can serve hr-netcore, hr-edge, hr-orchestrator, etc.
pub trait IpcHandler<Req, Resp>: Send + Sync + 'static
where
    Req: DeserializeOwned + Send,
    Resp: Serialize + Send,
{
    fn handle(&self, request: Req) -> impl std::future::Future<Output = Resp> + Send;
}

/// Start a generic IPC Unix socket server.
/// Removes stale socket at startup.
pub async fn run_ipc_server<Req, Resp, H>(
    socket_path: &Path,
    handler: Arc<H>,
) -> Result<()>
where
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
    H: IpcHandler<Req, Resp>,
{
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
                    if let Err(e) = handle_connection::<Req, Resp, H>(stream, handler).await {
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

async fn handle_connection<Req, Resp, H>(
    stream: tokio::net::UnixStream,
    handler: Arc<H>,
) -> Result<()>
where
    Req: DeserializeOwned + Send,
    Resp: Serialize + Send,
    H: IpcHandler<Req, Resp>,
{
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    buf_reader.read_line(&mut line).await?;

    if line.is_empty() {
        return Ok(());
    }

    let request: Req = serde_json::from_str(line.trim())?;
    let response = handler.handle(request).await;

    let mut resp_line = serde_json::to_string(&response)?;
    resp_line.push('\n');
    writer.write_all(resp_line.as_bytes()).await?;
    writer.shutdown().await?;

    Ok(())
}
