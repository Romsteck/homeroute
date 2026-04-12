use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tracing::{debug, info, warn};

use crate::SharedDnsState;
use crate::packet::{self, RCODE_FORMERR};
use crate::resolver;

/// Run a DNS UDP server on the given address.
pub async fn run_udp_server(addr: SocketAddr, state: SharedDnsState) -> Result<()> {
    let socket = Arc::new(UdpSocket::bind(addr).await?);
    info!("DNS UDP server listening on {}", addr);

    let mut buf = [0u8; 4096];

    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                warn!("UDP recv error: {}", e);
                continue;
            }
        };

        let packet = buf[..len].to_vec();
        let socket = socket.clone();
        let state = state.clone();

        tokio::spawn(async move {
            let (mut response, edns_udp_size) =
                handle_dns_query_with_edns(&packet, &state, src).await;
            // Silently drop responses for malformed packets (empty = nothing parseable)
            if response.is_empty() {
                return;
            }
            // Use client's EDNS0 UDP payload size if available, else RFC 1035 limit (512)
            let max_udp = if edns_udp_size > 0 {
                (edns_udp_size as usize).min(4096)
            } else {
                512
            };
            packet::truncate_for_udp(&mut response, max_udp);
            if let Err(e) = socket.send_to(&response, src).await {
                debug!("Failed to send UDP response to {}: {}", src, e);
            }
        });
    }
}

/// Run a DNS TCP server on the given address.
pub async fn run_tcp_server(addr: SocketAddr, state: SharedDnsState) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("DNS TCP server listening on {}", addr);

    loop {
        let (stream, src) = match listener.accept().await {
            Ok(r) => r,
            Err(e) => {
                warn!("TCP accept error: {}", e);
                continue;
            }
        };

        let state = state.clone();
        tokio::spawn(async move {
            match handle_tcp_connection(stream, src, &state).await {
                Ok(()) => {}
                Err(e) => {
                    // Connection resets and broken pipes are normal for DNS TCP
                    // (client closes after receiving response). Only log
                    // unexpected errors.
                    let io_err = e.downcast_ref::<std::io::Error>();
                    let is_benign = io_err.map_or(false, |ie| {
                        matches!(
                            ie.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::BrokenPipe
                                | std::io::ErrorKind::UnexpectedEof
                                | std::io::ErrorKind::NotConnected
                        )
                    });
                    if !is_benign {
                        debug!("TCP connection error from {}: {}", src, e);
                    }
                }
            }
        });
    }
}

async fn handle_tcp_connection(
    mut stream: tokio::net::TcpStream,
    src: SocketAddr,
    state: &SharedDnsState,
) -> Result<()> {
    // TCP DNS: read 2-byte length prefix, then message
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u16::from_be_bytes(len_buf) as usize;

    if msg_len > 65535 || msg_len < 12 {
        return Ok(());
    }

    let mut query_buf = vec![0u8; msg_len];
    stream.read_exact(&mut query_buf).await?;

    let response = handle_dns_query(&query_buf, state, src).await;

    // Write length prefix + response in a single write (RFC 1035 TCP framing).
    // Sending them separately causes broken framing visible in tcpdump as
    // "[prefix length != length]" because the 2-byte prefix and payload land
    // in different TCP segments.
    let resp_len = response.len() as u16;
    let mut buf = Vec::with_capacity(2 + response.len());
    buf.extend_from_slice(&resp_len.to_be_bytes());
    buf.extend_from_slice(&response);
    stream.write_all(&buf).await?;
    stream.shutdown().await?;

    Ok(())
}

/// Handle a DNS query and return (response, client_edns_udp_size).
async fn handle_dns_query_with_edns(
    query_bytes: &[u8],
    state: &SharedDnsState,
    src: SocketAddr,
) -> (Vec<u8>, u16) {
    let edns_size = packet::peek_edns_udp_size(query_bytes);
    let response = handle_dns_query(query_bytes, state, src).await;
    (response, edns_size)
}

async fn handle_dns_query(query_bytes: &[u8], state: &SharedDnsState, src: SocketAddr) -> Vec<u8> {
    // Parse query
    let query = match packet::parse_query(query_bytes) {
        Ok(q) => q,
        Err(e) => {
            debug!("Failed to parse DNS query from {}: {}", src, e);
            // Return FORMERR if we can parse at least the header
            if query_bytes.len() >= 12 {
                let mut err_resp = query_bytes[..12].to_vec();
                // Set QR=1, RCODE=FORMERR
                err_resp[2] |= 0x80;
                err_resp[3] = (err_resp[3] & 0xF0) | RCODE_FORMERR;
                return err_resp;
            }
            return vec![];
        }
    };

    let start = std::time::Instant::now();

    // Resolve
    let result = resolver::resolve(&query, state).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    // Build response
    let response = packet::build_response(&query, &result.records, result.rcode);

    // Log query
    if !query.questions.is_empty() {
        let q = &query.questions[0];
        let state_read = state.read().await;
        if let Some(ref logger) = state_read.query_logger {
            logger.log(
                &q.name,
                &q.qtype.to_string(),
                &src.ip().to_string(),
                result.blocked,
                result.cached,
                elapsed_ms,
            );
        }
    }

    response
}
