use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use anyhow::Result;
use hr_tunnel::protocol::{ControlMessage, StreamHeader};
use quinn::Connection;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Shared state: the active QUIC connection from on-prem (if any).
pub type ActiveConnection = Arc<RwLock<Option<Connection>>>;

/// Accept incoming TCP connections on the relay port and forward them through the QUIC tunnel.
pub async fn run_tcp_relay(listener: TcpListener, active_conn: ActiveConnection) -> Result<()> {
    info!("TCP relay listening on {}", listener.local_addr()?);

    loop {
        let (tcp_stream, peer_addr) = match listener.accept().await {
            Ok(r) => r,
            Err(e) => {
                warn!("TCP accept error: {}", e);
                continue;
            }
        };

        let conn = active_conn.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_tcp_connection(tcp_stream, peer_addr, conn).await {
                debug!("Relay connection from {} error: {}", peer_addr, e);
            }
        });
    }
}

async fn handle_tcp_connection(
    mut tcp_stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    active_conn: ActiveConnection,
) -> Result<()> {
    // Get the active QUIC connection (fail if not connected)
    let conn = active_conn
        .read()
        .await
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No active tunnel connection"))?
        .clone();

    // Open a bidirectional QUIC stream
    let (mut quic_send, mut quic_recv) = conn.open_bi().await?;

    // Send StreamHeader with peer IP and current timestamp
    let header = StreamHeader {
        client_ip: peer_addr.ip(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    };
    quic_send.write_all(&header.encode()).await?;

    // Bidirectional copy between TCP and QUIC
    let (mut tcp_read, mut tcp_write) = tcp_stream.split();

    let client_to_server = tokio::io::copy(&mut tcp_read, &mut quic_send);
    let server_to_client = tokio::io::copy(&mut quic_recv, &mut tcp_write);

    tokio::select! {
        result = client_to_server => {
            if let Err(e) = result {
                debug!("TCP->QUIC copy error: {}", e);
            }
            let _ = quic_send.finish();
        }
        result = server_to_client => {
            if let Err(e) = result {
                debug!("QUIC->TCP copy error: {}", e);
            }
        }
    }

    Ok(())
}

/// Simple HTTP server that redirects all requests to HTTPS.
pub async fn run_http_redirect(port: u16) -> Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;

    let addr: SocketAddr = format!("[::]:{}", port).parse()?;
    let listener = TcpListener::bind(addr).await?;

    info!("HTTP redirect listening on {}", addr);

    loop {
        let (stream, _remote) = match listener.accept().await {
            Ok(r) => r,
            Err(e) => {
                warn!("HTTP redirect accept error: {}", e);
                continue;
            }
        };

        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            let service = service_fn(|req: hyper::Request<hyper::body::Incoming>| async move {
                let host = req
                    .headers()
                    .get("host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("localhost");
                let path = req
                    .uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/");
                let location = format!("https://{}{}", host, path);

                Ok::<_, std::convert::Infallible>(
                    hyper::Response::builder()
                        .status(301)
                        .header("Location", &location)
                        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
                        .unwrap(),
                )
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                let msg = e.to_string();
                if !msg.contains("connection closed") && !msg.contains("not connected") {
                    debug!("HTTP redirect error: {}", msg);
                }
            }
        });
    }
}

/// Handle the control stream for a tunnel connection (ping/pong, binary updates).
pub async fn handle_control_stream(conn: &Connection) {
    loop {
        match conn.accept_uni().await {
            Ok(mut recv) => {
                tokio::spawn(async move {
                    // Read length-prefixed control message
                    let mut len_buf = [0u8; 4];
                    if recv.read_exact(&mut len_buf).await.is_err() {
                        return;
                    }
                    let msg_len = u32::from_be_bytes(len_buf) as usize;
                    if msg_len > 1024 * 1024 {
                        warn!("Control message too large: {} bytes", msg_len);
                        return;
                    }

                    let mut json_buf = vec![0u8; msg_len];
                    if recv.read_exact(&mut json_buf).await.is_err() {
                        return;
                    }

                    let msg: ControlMessage = match serde_json::from_slice(&json_buf) {
                        Ok(m) => m,
                        Err(e) => {
                            debug!("Invalid control message: {}", e);
                            return;
                        }
                    };

                    match msg {
                        ControlMessage::BinaryUpdate { size, sha256 } => {
                            info!("Receiving binary update: {} bytes, sha256={}", size, sha256);
                            if let Err(e) = handle_binary_update(&mut recv, size, &sha256).await {
                                error!("Binary update failed: {}", e);
                            }
                        }
                        ControlMessage::Ping { ts } => {
                            debug!("Received ping ts={}", ts);
                        }
                        _ => {
                            debug!("Received control message: {:?}", msg);
                        }
                    }
                });
            }
            Err(e) => {
                debug!("Accept uni stream error: {}", e);
                break;
            }
        }
    }
}

/// Receive a binary update via QUIC, verify SHA256, replace the running binary, and restart.
async fn handle_binary_update(
    recv: &mut quinn::RecvStream,
    size: u64,
    expected_sha256: &str,
) -> Result<()> {
    const BINARY_PATH: &str = "/usr/local/bin/hr-cloud-relay";
    let tmp_path = "/tmp/hr-cloud-relay-update";

    // Read the binary data
    let mut file = tokio::fs::File::create(tmp_path).await?;
    let mut hasher = Sha256::new();
    let mut remaining = size;
    let mut buf = vec![0u8; 65536];

    while remaining > 0 {
        let to_read = std::cmp::min(remaining as usize, buf.len());
        let n = recv
            .read(&mut buf[..to_read])
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream ended before full binary received"))?;
        hasher.update(&buf[..n]);
        tokio::io::AsyncWriteExt::write_all(&mut file, &buf[..n]).await?;
        remaining -= n as u64;
    }

    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    // Verify SHA256
    let computed = format!("{:x}", hasher.finalize());
    if computed != expected_sha256 {
        let _ = tokio::fs::remove_file(tmp_path).await;
        anyhow::bail!(
            "SHA256 mismatch: expected {}, got {}",
            expected_sha256,
            computed
        );
    }
    info!("Binary SHA256 verified OK");

    // Make executable and replace
    tokio::fs::set_permissions(tmp_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .await?;
    tokio::fs::rename(tmp_path, BINARY_PATH).await?;
    info!("Binary replaced at {}", BINARY_PATH);

    // Restart the service (will terminate this process)
    info!("Restarting hr-cloud-relay service...");
    let _ = tokio::process::Command::new("systemctl")
        .args(["restart", "hr-cloud-relay"])
        .spawn();

    Ok(())
}
