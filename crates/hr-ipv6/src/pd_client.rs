//! DHCPv6 Prefix Delegation client (RFC 8415).
//!
//! Runs on the WAN interface to obtain a delegated IPv6 prefix from upstream
//! (e.g. Starlink /56), selects a /64 subnet, and broadcasts the result via
//! a `watch` channel so the RA sender and firewall can react.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::Duration;

use anyhow::{Context, Result};
use rand::Rng;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::config::{Ipv6Config, PdState};

// ── DHCPv6 message types ────────────────────────────────────────────────────

const MSG_SOLICIT: u8 = 1;
const MSG_ADVERTISE: u8 = 2;
const MSG_REQUEST: u8 = 3;
const MSG_RENEW: u8 = 5;
const MSG_REBIND: u8 = 6;
const MSG_REPLY: u8 = 7;

// ── DHCPv6 option codes ─────────────────────────────────────────────────────

const OPT_CLIENTID: u16 = 1;
const OPT_SERVERID: u16 = 2;
const OPT_IA_PD: u16 = 25;
const OPT_IAPREFIX: u16 = 26;
const OPT_ELAPSED_TIME: u16 = 8;
const OPT_STATUS_CODE: u16 = 13;

// ── Public types ─────────────────────────────────────────────────────────────

/// Information about a delegated prefix, sent to RA/firewall via watch channel.
#[derive(Debug, Clone)]
pub struct PrefixInfo {
    pub prefix: Ipv6Addr,
    pub prefix_len: u8,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
}

pub type PrefixSender = watch::Sender<Option<PrefixInfo>>;
pub type PrefixWatch = watch::Receiver<Option<PrefixInfo>>;

// ── DHCPv6-PD state machine ─────────────────────────────────────────────────

#[derive(Debug)]
enum PdFsmState {
    Init,
    Soliciting,
    Requesting {
        server_duid: Vec<u8>,
        ia_pd_data: Vec<u8>,
    },
    Bound {
        state: PdState,
    },
    Renewing {
        state: PdState,
    },
    Rebinding {
        state: PdState,
    },
}

/// Run the DHCPv6-PD client. This function never returns under normal operation.
pub async fn run_pd_client(
    config: Ipv6Config,
    prefix_tx: PrefixSender,
) -> Result<()> {
    if !config.pd_enabled {
        info!("DHCPv6-PD client disabled");
        std::future::pending::<()>().await;
        return Ok(());
    }

    info!(
        "Starting DHCPv6-PD client on {} (hint /{}, subnet_id {})",
        config.pd_wan_interface, config.pd_prefix_hint_len, config.pd_subnet_id
    );

    let socket = create_dhcpv6_socket(&config.pd_wan_interface)?;

    let client_duid = generate_client_duid(&config.pd_wan_interface);
    let iaid: u32 = 1;

    // All-DHCP-Relay-Agents-and-Servers multicast
    let server_addr = SocketAddrV6::new("ff02::1:2".parse().unwrap(), 547, 0, 0);

    let mut fsm = PdFsmState::Init;
    let mut recv_buf = [0u8; 1500];

    loop {
        match fsm {
            PdFsmState::Init => {
                // Try to load persisted state
                if let Some(saved) = PdState::load() {
                    if saved.is_valid() {
                        info!(
                            "Loaded persisted PD state: {} (valid for {}s more)",
                            saved.delegated_prefix,
                            remaining_secs(&saved)
                        );
                        // Publish the saved prefix
                        let subnet_prefix = parse_prefix_str(&saved.selected_subnet);
                        if let Some((addr, len)) = subnet_prefix {
                            let _ = prefix_tx.send(Some(PrefixInfo {
                                prefix: addr,
                                prefix_len: len,
                                valid_lifetime: saved.valid_lifetime,
                                preferred_lifetime: saved.preferred_lifetime,
                            }));
                        }
                        fsm = PdFsmState::Renewing { state: saved };
                        continue;
                    }
                    info!("Persisted PD state expired, starting fresh SOLICIT");
                }
                fsm = PdFsmState::Soliciting;
            }

            PdFsmState::Soliciting => {
                let xid = random_xid();
                let solicit = build_solicit(&xid, &client_duid, iaid, config.pd_prefix_hint_len);

                match solicit_exchange(&socket, &solicit, &server_addr, &mut recv_buf, &xid).await {
                    Ok(advertise) => {
                        let server_duid = match extract_option(&advertise, OPT_SERVERID) {
                            Some(d) => d,
                            None => {
                                warn!("ADVERTISE missing Server DUID, retrying");
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            }
                        };
                        let ia_pd_data = match extract_option(&advertise, OPT_IA_PD) {
                            Some(d) => d,
                            None => {
                                warn!("ADVERTISE missing IA_PD, retrying");
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            }
                        };
                        info!("Received ADVERTISE from server");
                        fsm = PdFsmState::Requesting { server_duid, ia_pd_data };
                    }
                    Err(e) => {
                        warn!("SOLICIT failed: {}, retrying in 5s", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }

            PdFsmState::Requesting { ref server_duid, ref ia_pd_data } => {
                let xid = random_xid();
                let request = build_request(
                    &xid, &client_duid, server_duid, iaid, ia_pd_data,
                );

                let server_duid_clone = server_duid.clone();
                match request_exchange(&socket, &request, &server_addr, &mut recv_buf, &xid).await {
                    Ok(reply_opts) => {
                        match process_reply(&reply_opts, &config, &client_duid, &server_duid_clone, iaid) {
                            Ok(pd_state) => {
                                info!(
                                    "DHCPv6-PD BOUND: delegated {} → subnet {}",
                                    pd_state.delegated_prefix, pd_state.selected_subnet
                                );
                                publish_prefix(&prefix_tx, &pd_state);
                                if let Err(e) = pd_state.save() {
                                    warn!("Failed to persist PD state: {}", e);
                                }
                                fsm = PdFsmState::Bound { state: pd_state };
                            }
                            Err(e) => {
                                warn!("Failed to process REPLY: {}, restarting SOLICIT", e);
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                fsm = PdFsmState::Soliciting;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("REQUEST failed: {}, restarting SOLICIT", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        fsm = PdFsmState::Soliciting;
                    }
                }
            }

            PdFsmState::Bound { ref state } => {
                // Wait until T1 to renew
                let t1 = if state.t1 > 0 { state.t1 } else { state.valid_lifetime / 2 };
                let elapsed = now_secs().saturating_sub(state.obtained_at);
                let wait = (t1 as u64).saturating_sub(elapsed);

                if wait > 0 {
                    info!("PD BOUND: will renew in {}s (T1={})", wait, t1);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }

                fsm = PdFsmState::Renewing { state: state.clone() };
            }

            PdFsmState::Renewing { ref state } => {
                let xid = random_xid();
                let renew = build_renew(
                    &xid, &client_duid, &state.server_duid, iaid,
                    config.pd_prefix_hint_len,
                );

                match request_exchange(&socket, &renew, &server_addr, &mut recv_buf, &xid).await {
                    Ok(reply_opts) => {
                        match process_reply(&reply_opts, &config, &client_duid, &state.server_duid, iaid) {
                            Ok(new_state) => {
                                info!(
                                    "DHCPv6-PD RENEWED: {} → {}",
                                    new_state.delegated_prefix, new_state.selected_subnet
                                );
                                publish_prefix(&prefix_tx, &new_state);
                                if let Err(e) = new_state.save() {
                                    warn!("Failed to persist PD state: {}", e);
                                }
                                fsm = PdFsmState::Bound { state: new_state };
                            }
                            Err(e) => {
                                warn!("RENEW reply parse failed: {}, trying REBIND", e);
                                fsm = PdFsmState::Rebinding { state: state.clone() };
                            }
                        }
                    }
                    Err(e) => {
                        warn!("RENEW failed: {}, trying REBIND", e);
                        fsm = PdFsmState::Rebinding { state: state.clone() };
                    }
                }
            }

            PdFsmState::Rebinding { ref state } => {
                // Check if prefix has expired
                let elapsed = now_secs().saturating_sub(state.obtained_at);
                if elapsed >= state.valid_lifetime as u64 {
                    warn!("Prefix expired, withdrawing and restarting SOLICIT");
                    let _ = prefix_tx.send(None);
                    fsm = PdFsmState::Soliciting;
                    continue;
                }

                let xid = random_xid();
                let rebind = build_rebind(
                    &xid, &client_duid, iaid, config.pd_prefix_hint_len,
                );

                match request_exchange(&socket, &rebind, &server_addr, &mut recv_buf, &xid).await {
                    Ok(reply_opts) => {
                        // Extract server DUID from the rebind reply
                        let new_server_duid = extract_option_from_slice(&reply_opts, OPT_SERVERID)
                            .unwrap_or_else(|| state.server_duid.clone());
                        match process_reply(&reply_opts, &config, &client_duid, &new_server_duid, iaid) {
                            Ok(new_state) => {
                                info!("DHCPv6-PD REBOUND: {}", new_state.delegated_prefix);
                                publish_prefix(&prefix_tx, &new_state);
                                if let Err(e) = new_state.save() {
                                    warn!("Failed to persist PD state: {}", e);
                                }
                                fsm = PdFsmState::Bound { state: new_state };
                            }
                            Err(e) => {
                                warn!("REBIND reply failed: {}, waiting 10s", e);
                                tokio::time::sleep(Duration::from_secs(10)).await;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("REBIND failed: {}, waiting 10s before retry", e);
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                }
            }
        }
    }
}

// ── Socket creation ──────────────────────────────────────────────────────────

fn create_dhcpv6_socket(wan_interface: &str) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;

    #[cfg(target_os = "linux")]
    socket.bind_device(Some(wan_interface.as_bytes()))?;

    // Bind to DHCPv6 client port
    let bind_addr: std::net::SocketAddrV6 = "[::]:546".parse().unwrap();
    socket.bind(&bind_addr.into())?;
    socket.set_nonblocking(true)?;

    let socket = UdpSocket::from_std(socket.into())?;
    Ok(socket)
}

// ── DUID generation ──────────────────────────────────────────────────────────

fn generate_client_duid(interface: &str) -> Vec<u8> {
    // DUID-LL (type 3): link-layer address
    // Type(2) + HW type(2) + MAC(6) = 10 bytes
    let mac = read_interface_mac(interface).unwrap_or([0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);

    let mut duid = Vec::with_capacity(10);
    duid.extend_from_slice(&3u16.to_be_bytes()); // DUID-LL
    duid.extend_from_slice(&1u16.to_be_bytes()); // Ethernet
    duid.extend_from_slice(&mac);
    duid
}

fn read_interface_mac(interface: &str) -> Option<[u8; 6]> {
    let path = format!("/sys/class/net/{}/address", interface);
    let content = std::fs::read_to_string(path).ok()?;
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

// ── Packet builders ──────────────────────────────────────────────────────────

fn random_xid() -> [u8; 3] {
    let mut rng = rand::rng();
    [rng.random(), rng.random(), rng.random()]
}

fn build_solicit(xid: &[u8; 3], client_duid: &[u8], iaid: u32, hint_len: u8) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    // Header: type + xid
    buf.push(MSG_SOLICIT);
    buf.extend_from_slice(xid);

    // Client ID
    append_option(&mut buf, OPT_CLIENTID, client_duid);

    // Elapsed Time (0ms)
    append_option(&mut buf, OPT_ELAPSED_TIME, &[0, 0]);

    // IA_PD with IA_PREFIX hint
    let ia_pd = build_ia_pd(iaid, 0, 0, Some(hint_len));
    append_option(&mut buf, OPT_IA_PD, &ia_pd);

    buf
}

fn build_request(
    xid: &[u8; 3],
    client_duid: &[u8],
    server_duid: &[u8],
    iaid: u32,
    ia_pd_from_advertise: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    buf.push(MSG_REQUEST);
    buf.extend_from_slice(xid);

    append_option(&mut buf, OPT_CLIENTID, client_duid);
    append_option(&mut buf, OPT_SERVERID, server_duid);
    append_option(&mut buf, OPT_ELAPSED_TIME, &[0, 0]);

    // Echo IA_PD from ADVERTISE
    append_option(&mut buf, OPT_IA_PD, ia_pd_from_advertise);

    buf
}

fn build_renew(
    xid: &[u8; 3],
    client_duid: &[u8],
    server_duid: &[u8],
    iaid: u32,
    hint_len: u8,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    buf.push(MSG_RENEW);
    buf.extend_from_slice(xid);

    append_option(&mut buf, OPT_CLIENTID, client_duid);
    append_option(&mut buf, OPT_SERVERID, server_duid);
    append_option(&mut buf, OPT_ELAPSED_TIME, &[0, 0]);

    let ia_pd = build_ia_pd(iaid, 0, 0, Some(hint_len));
    append_option(&mut buf, OPT_IA_PD, &ia_pd);

    buf
}

fn build_rebind(
    xid: &[u8; 3],
    client_duid: &[u8],
    iaid: u32,
    hint_len: u8,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    buf.push(MSG_REBIND);
    buf.extend_from_slice(xid);

    append_option(&mut buf, OPT_CLIENTID, client_duid);
    append_option(&mut buf, OPT_ELAPSED_TIME, &[0, 0]);

    let ia_pd = build_ia_pd(iaid, 0, 0, Some(hint_len));
    append_option(&mut buf, OPT_IA_PD, &ia_pd);

    buf
}

/// Build IA_PD option data (without the outer option header).
/// Contains: IAID(4) + T1(4) + T2(4) + optional IA_PREFIX sub-option.
fn build_ia_pd(iaid: u32, t1: u32, t2: u32, hint_prefix_len: Option<u8>) -> Vec<u8> {
    let mut data = Vec::with_capacity(48);

    data.extend_from_slice(&iaid.to_be_bytes());
    data.extend_from_slice(&t1.to_be_bytes());
    data.extend_from_slice(&t2.to_be_bytes());

    // IA_PREFIX sub-option as hint
    if let Some(plen) = hint_prefix_len {
        let mut prefix_data = Vec::with_capacity(25);
        prefix_data.extend_from_slice(&0u32.to_be_bytes()); // preferred lifetime
        prefix_data.extend_from_slice(&0u32.to_be_bytes()); // valid lifetime
        prefix_data.push(plen);                              // prefix length
        prefix_data.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets()); // prefix (hint)

        // Sub-option header
        data.extend_from_slice(&OPT_IAPREFIX.to_be_bytes());
        data.extend_from_slice(&(prefix_data.len() as u16).to_be_bytes());
        data.extend_from_slice(&prefix_data);
    }

    data
}

fn append_option(buf: &mut Vec<u8>, code: u16, data: &[u8]) {
    buf.extend_from_slice(&code.to_be_bytes());
    buf.extend_from_slice(&(data.len() as u16).to_be_bytes());
    buf.extend_from_slice(data);
}

// ── Packet parsing ───────────────────────────────────────────────────────────

/// Extract an option from a DHCPv6 message payload (after the 4-byte header).
fn extract_option(msg: &[u8], option_code: u16) -> Option<Vec<u8>> {
    if msg.len() < 4 { return None; }
    extract_option_from_slice(&msg[4..], option_code)
}

/// Extract an option from raw option bytes.
fn extract_option_from_slice(data: &[u8], option_code: u16) -> Option<Vec<u8>> {
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let code = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + len > data.len() {
            break;
        }

        if code == option_code {
            return Some(data[offset..offset + len].to_vec());
        }

        offset += len;
    }
    None
}

/// Parse IA_PD option data to extract delegated prefix(es).
struct IaPdInfo {
    t1: u32,
    t2: u32,
    prefixes: Vec<IaPrefixInfo>,
}

struct IaPrefixInfo {
    preferred_lifetime: u32,
    valid_lifetime: u32,
    prefix_len: u8,
    prefix: Ipv6Addr,
}

fn parse_ia_pd(data: &[u8]) -> Option<IaPdInfo> {
    if data.len() < 12 {
        return None;
    }

    let _iaid = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let t1 = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let t2 = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

    let mut prefixes = Vec::new();
    let mut offset = 12;

    // Parse sub-options
    while offset + 4 <= data.len() {
        let code = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + len > data.len() {
            break;
        }

        if code == OPT_IAPREFIX && len >= 25 {
            let sub = &data[offset..offset + len];
            let preferred = u32::from_be_bytes([sub[0], sub[1], sub[2], sub[3]]);
            let valid = u32::from_be_bytes([sub[4], sub[5], sub[6], sub[7]]);
            let plen = sub[8];
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&sub[9..25]);
            prefixes.push(IaPrefixInfo {
                preferred_lifetime: preferred,
                valid_lifetime: valid,
                prefix_len: plen,
                prefix: Ipv6Addr::from(octets),
            });
        } else if code == OPT_STATUS_CODE && len >= 2 {
            let status = u16::from_be_bytes([data[offset], data[offset + 1]]);
            if status != 0 {
                let msg = if len > 2 {
                    String::from_utf8_lossy(&data[offset + 2..offset + len]).to_string()
                } else {
                    format!("status code {}", status)
                };
                warn!("IA_PD status error: {}", msg);
                return None;
            }
        }

        offset += len;
    }

    Some(IaPdInfo { t1, t2, prefixes })
}

// ── Exchange helpers ─────────────────────────────────────────────────────────

/// Send SOLICIT and wait for ADVERTISE with retransmission.
async fn solicit_exchange(
    socket: &UdpSocket,
    solicit: &[u8],
    dest: &SocketAddrV6,
    recv_buf: &mut [u8],
    xid: &[u8; 3],
) -> Result<Vec<u8>> {
    // RFC 8415 §15: SOL_TIMEOUT=1s, SOL_MAX_RT=120s
    let mut timeout = Duration::from_secs(1);
    let max_timeout = Duration::from_secs(120);

    for attempt in 0..10 {
        debug!("Sending SOLICIT (attempt {})", attempt + 1);
        socket.send_to(solicit, std::net::SocketAddr::V6(*dest)).await
            .context("Failed to send SOLICIT")?;

        match recv_with_timeout(socket, recv_buf, timeout).await {
            Ok(len) => {
                if len >= 4 && recv_buf[0] == MSG_ADVERTISE
                    && recv_buf[1] == xid[0] && recv_buf[2] == xid[1] && recv_buf[3] == xid[2]
                {
                    return Ok(recv_buf[..len].to_vec());
                }
                debug!("Received non-matching message type={} (expecting ADVERTISE)", recv_buf[0]);
            }
            Err(_) => {
                debug!("SOLICIT timeout ({}ms)", timeout.as_millis());
            }
        }

        // Exponential backoff with jitter
        timeout = (timeout * 2).min(max_timeout);
        let jitter = Duration::from_millis(rand::rng().random_range(0..500));
        timeout += jitter;
    }

    anyhow::bail!("SOLICIT: no ADVERTISE received after retries")
}

/// Send REQUEST/RENEW/REBIND and wait for REPLY with retransmission.
async fn request_exchange(
    socket: &UdpSocket,
    request: &[u8],
    dest: &SocketAddrV6,
    recv_buf: &mut [u8],
    xid: &[u8; 3],
) -> Result<Vec<u8>> {
    // RFC 8415 §15: REQ_TIMEOUT=1s, REQ_MAX_RT=30s, REQ_MAX_RC=10
    let mut timeout = Duration::from_secs(1);
    let max_timeout = Duration::from_secs(30);
    let msg_name = match request[0] {
        MSG_REQUEST => "REQUEST",
        MSG_RENEW => "RENEW",
        MSG_REBIND => "REBIND",
        _ => "UNKNOWN",
    };

    for attempt in 0..10 {
        debug!("Sending {} (attempt {})", msg_name, attempt + 1);
        socket.send_to(request, std::net::SocketAddr::V6(*dest)).await
            .context(format!("Failed to send {}", msg_name))?;

        match recv_with_timeout(socket, recv_buf, timeout).await {
            Ok(len) => {
                if len >= 4 && recv_buf[0] == MSG_REPLY
                    && recv_buf[1] == xid[0] && recv_buf[2] == xid[1] && recv_buf[3] == xid[2]
                {
                    return Ok(recv_buf[..len].to_vec());
                }
            }
            Err(_) => {
                debug!("{} timeout ({}ms)", msg_name, timeout.as_millis());
            }
        }

        timeout = (timeout * 2).min(max_timeout);
        let jitter = Duration::from_millis(rand::rng().random_range(0..500));
        timeout += jitter;
    }

    anyhow::bail!("{}: no REPLY received after retries", msg_name)
}

async fn recv_with_timeout(
    socket: &UdpSocket,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<usize> {
    match tokio::time::timeout(timeout, socket.recv_from(buf)).await {
        Ok(Ok((len, _src))) => Ok(len),
        Ok(Err(e)) => anyhow::bail!("recv error: {}", e),
        Err(_) => anyhow::bail!("timeout"),
    }
}

// ── Reply processing ─────────────────────────────────────────────────────────

fn process_reply(
    reply: &[u8],
    config: &Ipv6Config,
    client_duid: &[u8],
    server_duid: &[u8],
    iaid: u32,
) -> Result<PdState> {
    // Check for top-level status code
    if let Some(status_data) = extract_option(reply, OPT_STATUS_CODE) {
        if status_data.len() >= 2 {
            let code = u16::from_be_bytes([status_data[0], status_data[1]]);
            if code != 0 {
                let msg = if status_data.len() > 2 {
                    String::from_utf8_lossy(&status_data[2..]).to_string()
                } else {
                    format!("code {}", code)
                };
                anyhow::bail!("DHCPv6 status error: {}", msg);
            }
        }
    }

    let ia_pd_data = extract_option(reply, OPT_IA_PD)
        .context("REPLY missing IA_PD option")?;

    let ia_pd = parse_ia_pd(&ia_pd_data)
        .context("Failed to parse IA_PD")?;

    let prefix_info = ia_pd.prefixes.first()
        .context("IA_PD contains no IA_PREFIX")?;

    if prefix_info.valid_lifetime == 0 {
        anyhow::bail!("Delegated prefix has valid_lifetime=0");
    }

    let (subnet_addr, subnet_len) = select_subnet(
        prefix_info.prefix,
        prefix_info.prefix_len,
        config.pd_subnet_id,
    );

    let delegated_str = format!("{}/{}", prefix_info.prefix, prefix_info.prefix_len);
    let subnet_str = format!("{}/{}", subnet_addr, subnet_len);

    Ok(PdState {
        delegated_prefix: delegated_str,
        delegated_prefix_len: prefix_info.prefix_len,
        selected_subnet: subnet_str,
        server_duid: server_duid.to_vec(),
        client_duid: client_duid.to_vec(),
        iaid,
        t1: ia_pd.t1,
        t2: ia_pd.t2,
        valid_lifetime: prefix_info.valid_lifetime,
        preferred_lifetime: prefix_info.preferred_lifetime,
        obtained_at: now_secs(),
    })
}

// ── Subnet selection ─────────────────────────────────────────────────────────

/// Select a /64 subnet from a delegated prefix.
/// For a /56, there are 256 possible /64 subnets (bits 56-63 = byte 7).
fn select_subnet(delegated: Ipv6Addr, delegated_len: u8, subnet_id: u16) -> (Ipv6Addr, u8) {
    let mut octets = delegated.octets();

    // The subnet_id fills the bits between delegated_len and 64
    let host_bits = 64u8.saturating_sub(delegated_len);
    if host_bits > 0 && host_bits <= 16 {
        let byte_idx = (delegated_len / 8) as usize;
        let bit_offset = delegated_len % 8;

        if bit_offset == 0 && host_bits == 8 {
            // Common case: /56 → byte 7 selects the /64
            octets[byte_idx] = subnet_id as u8;
        } else {
            // General case: place subnet_id in the right bits
            let id = (subnet_id as u16) << (16 - host_bits as u16 - bit_offset as u16);
            let id_bytes = id.to_be_bytes();
            octets[byte_idx] |= id_bytes[0];
            if byte_idx + 1 < 16 {
                octets[byte_idx + 1] |= id_bytes[1];
            }
        }
    }

    (Ipv6Addr::from(octets), 64)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn publish_prefix(tx: &PrefixSender, state: &PdState) {
    if let Some((addr, len)) = parse_prefix_str(&state.selected_subnet) {
        let _ = tx.send(Some(PrefixInfo {
            prefix: addr,
            prefix_len: len,
            valid_lifetime: state.valid_lifetime,
            preferred_lifetime: state.preferred_lifetime,
        }));
    }
}

fn parse_prefix_str(s: &str) -> Option<(Ipv6Addr, u8)> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 2 { return None; }
    let addr: Ipv6Addr = parts[0].parse().ok()?;
    let len: u8 = parts[1].parse().ok()?;
    Some((addr, len))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn remaining_secs(state: &PdState) -> u64 {
    let expiry = state.obtained_at + state.valid_lifetime as u64;
    expiry.saturating_sub(now_secs())
}
