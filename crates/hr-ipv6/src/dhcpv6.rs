//! DHCPv6 stateful server (RFC 8415).
//!
//! Assigns IPv6 addresses from the delegated GUA prefix received via PD.
//! Handles SOLICIT, REQUEST, RENEW, REBIND, RELEASE, and INFORMATION-REQUEST.

use std::collections::HashMap;
use std::net::{Ipv6Addr, SocketAddrV6};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn, error};

use crate::config::Ipv6Config;
use crate::pd_client::PrefixInfo;

// ── DHCPv6 message types ────────────────────────────────────────────────────

const MSG_SOLICIT: u8 = 1;
const MSG_ADVERTISE: u8 = 2;
const MSG_REQUEST: u8 = 3;
const MSG_CONFIRM: u8 = 4;
const MSG_RENEW: u8 = 5;
const MSG_REBIND: u8 = 6;
const MSG_REPLY: u8 = 7;
const MSG_RELEASE: u8 = 8;
const MSG_INFORMATION_REQUEST: u8 = 11;

// ── DHCPv6 option codes ─────────────────────────────────────────────────────

const OPT_CLIENTID: u16 = 1;
const OPT_SERVERID: u16 = 2;
const OPT_IA_NA: u16 = 3;      // Identity Association for Non-temporary Addresses
const OPT_IAADDR: u16 = 5;     // IA Address
const OPT_ORO: u16 = 6;        // Option Request Option
const OPT_ELAPSED_TIME: u16 = 8;
const OPT_STATUS_CODE: u16 = 13;
const OPT_DNS_SERVERS: u16 = 23;

// Status codes
const STATUS_SUCCESS: u16 = 0;
const STATUS_NO_ADDRS_AVAIL: u16 = 2;
const STATUS_NO_BINDING: u16 = 3;

// ── Lease storage ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Dhcpv6Lease {
    pub duid: Vec<u8>,           // Client DUID
    pub iaid: u32,               // IA_NA identifier
    pub address: Ipv6Addr,       // Assigned address
    pub hostname: Option<String>,
    pub mac: Option<String>,     // MAC extracted from link-local source
    pub created_at: u64,         // Unix timestamp
    pub valid_until: u64,        // Unix timestamp
    pub preferred_until: u64,    // Unix timestamp
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Dhcpv6LeaseStore {
    leases: HashMap<String, Dhcpv6Lease>,  // Key: hex-encoded DUID
    #[serde(skip)]
    next_addr_suffix: u64,
}

impl Dhcpv6LeaseStore {
    const FILE_PATH: &'static str = "/var/lib/server-dashboard/dhcpv6-leases.json";

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::FILE_PATH) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(Self::FILE_PATH, data)?;
        Ok(())
    }

    pub fn all_leases(&self) -> Vec<&Dhcpv6Lease> {
        self.leases.values().collect()
    }

    fn duid_key(duid: &[u8]) -> String {
        hex::encode(duid)
    }

    /// Find existing lease for this client.
    pub fn find_by_duid(&self, duid: &[u8]) -> Option<&Dhcpv6Lease> {
        self.leases.get(&Self::duid_key(duid))
    }

    /// Allocate or renew an address for the client.
    pub fn allocate(
        &mut self,
        duid: &[u8],
        iaid: u32,
        mac: Option<String>,
        prefix: &PrefixInfo,
        config: &Ipv6Config,
    ) -> Option<Dhcpv6Lease> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let key = Self::duid_key(duid);
        let lease_time = config.dhcpv6_lease_time;
        let preferred_time = lease_time / 2;

        // Try to get MAC from link-local or fall back to DUID extraction
        let effective_mac = mac.or_else(|| extract_mac_from_duid(duid));

        // Check for existing lease
        if let Some(existing) = self.leases.get_mut(&key) {
            // Renew: update times, keep same address
            existing.valid_until = now + lease_time as u64;
            existing.preferred_until = now + preferred_time as u64;
            existing.iaid = iaid;
            // Update MAC if we now have it
            if effective_mac.is_some() && existing.mac.is_none() {
                existing.mac = effective_mac;
            }
            return Some(existing.clone());
        }

        // Allocate new address
        let suffix = self.find_free_suffix(prefix, config)?;
        let address = self.make_address(prefix, suffix);

        let lease = Dhcpv6Lease {
            duid: duid.to_vec(),
            iaid,
            address,
            hostname: None,
            mac: effective_mac,
            created_at: now,
            valid_until: now + lease_time as u64,
            preferred_until: now + preferred_time as u64,
        };

        self.leases.insert(key, lease.clone());
        Some(lease)
    }

    /// Release a lease.
    pub fn release(&mut self, duid: &[u8]) -> bool {
        self.leases.remove(&Self::duid_key(duid)).is_some()
    }

    /// Purge expired leases.
    pub fn purge_expired(&mut self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let before = self.leases.len();
        self.leases.retain(|_, lease| lease.valid_until > now);
        before - self.leases.len()
    }

    fn find_free_suffix(&mut self, prefix: &PrefixInfo, config: &Ipv6Config) -> Option<u64> {
        let used: std::collections::HashSet<u64> = self.leases.values()
            .filter_map(|l| {
                let octets = l.address.octets();
                let suffix = u64::from_be_bytes([
                    octets[8], octets[9], octets[10], octets[11],
                    octets[12], octets[13], octets[14], octets[15],
                ]);
                // Only count if in current prefix
                let prefix_bytes = prefix.prefix.octets();
                if octets[..8] == prefix_bytes[..8] {
                    Some(suffix)
                } else {
                    None
                }
            })
            .collect();

        // Find first free address in range
        for suffix in config.dhcpv6_range_start..=config.dhcpv6_range_end {
            if !used.contains(&suffix) {
                return Some(suffix);
            }
        }
        None
    }

    fn make_address(&self, prefix: &PrefixInfo, suffix: u64) -> Ipv6Addr {
        let prefix_bytes = prefix.prefix.octets();
        let suffix_bytes = suffix.to_be_bytes();
        Ipv6Addr::from([
            prefix_bytes[0], prefix_bytes[1], prefix_bytes[2], prefix_bytes[3],
            prefix_bytes[4], prefix_bytes[5], prefix_bytes[6], prefix_bytes[7],
            suffix_bytes[0], suffix_bytes[1], suffix_bytes[2], suffix_bytes[3],
            suffix_bytes[4], suffix_bytes[5], suffix_bytes[6], suffix_bytes[7],
        ])
    }
}

// ── Server DUID ─────────────────────────────────────────────────────────────

fn server_duid() -> Vec<u8> {
    // DUID-LLT (Link-layer address plus time) - simplified
    vec![0x00, 0x03, 0x00, 0x01, 0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]
}

// ── Option parsing ──────────────────────────────────────────────────────────

fn extract_option(data: &[u8], option_code: u16) -> Option<Vec<u8>> {
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

fn parse_ia_na(data: &[u8]) -> Option<(u32, u32, u32)> {
    // IA_NA: IAID (4) + T1 (4) + T2 (4) + options
    if data.len() < 12 {
        return None;
    }
    let iaid = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let t1 = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let t2 = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    Some((iaid, t1, t2))
}

// ── Response building ───────────────────────────────────────────────────────

fn build_response(
    msg_type: u8,
    transaction_id: &[u8; 3],
    client_duid: &[u8],
    lease: Option<&Dhcpv6Lease>,
    config: &Ipv6Config,
    status: Option<(u16, &str)>,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    // Message type + Transaction ID
    buf.push(msg_type);
    buf.extend_from_slice(transaction_id);

    // Client ID
    buf.extend_from_slice(&OPT_CLIENTID.to_be_bytes());
    buf.extend_from_slice(&(client_duid.len() as u16).to_be_bytes());
    buf.extend_from_slice(client_duid);

    // Server ID
    let server_id = server_duid();
    buf.extend_from_slice(&OPT_SERVERID.to_be_bytes());
    buf.extend_from_slice(&(server_id.len() as u16).to_be_bytes());
    buf.extend_from_slice(&server_id);

    // IA_NA with address (if lease provided)
    if let Some(lease) = lease {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let valid_lifetime = lease.valid_until.saturating_sub(now) as u32;
        let preferred_lifetime = lease.preferred_until.saturating_sub(now) as u32;
        let t1 = preferred_lifetime / 2;
        let t2 = (preferred_lifetime * 4) / 5;

        // Build IA Address option
        let mut ia_addr = Vec::with_capacity(24);
        ia_addr.extend_from_slice(&lease.address.octets());
        ia_addr.extend_from_slice(&preferred_lifetime.to_be_bytes());
        ia_addr.extend_from_slice(&valid_lifetime.to_be_bytes());

        // Build IA_NA option
        let ia_na_len = 12 + 4 + ia_addr.len(); // IAID+T1+T2 + IAADDR option header + IAADDR
        buf.extend_from_slice(&OPT_IA_NA.to_be_bytes());
        buf.extend_from_slice(&(ia_na_len as u16).to_be_bytes());
        buf.extend_from_slice(&lease.iaid.to_be_bytes());
        buf.extend_from_slice(&t1.to_be_bytes());
        buf.extend_from_slice(&t2.to_be_bytes());

        // IAADDR sub-option
        buf.extend_from_slice(&OPT_IAADDR.to_be_bytes());
        buf.extend_from_slice(&(ia_addr.len() as u16).to_be_bytes());
        buf.extend_from_slice(&ia_addr);
    }

    // Status code (if error)
    if let Some((code, msg)) = status {
        let msg_bytes = msg.as_bytes();
        buf.extend_from_slice(&OPT_STATUS_CODE.to_be_bytes());
        buf.extend_from_slice(&((2 + msg_bytes.len()) as u16).to_be_bytes());
        buf.extend_from_slice(&code.to_be_bytes());
        buf.extend_from_slice(msg_bytes);
    }

    // DNS servers
    let dns_addrs: Vec<Ipv6Addr> = config
        .dhcpv6_dns_servers
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    if !dns_addrs.is_empty() {
        let data_len = dns_addrs.len() * 16;
        buf.extend_from_slice(&OPT_DNS_SERVERS.to_be_bytes());
        buf.extend_from_slice(&(data_len as u16).to_be_bytes());
        for addr in &dns_addrs {
            buf.extend_from_slice(&addr.octets());
        }
    }

    buf
}

// ── Main server ─────────────────────────────────────────────────────────────

pub async fn run_dhcpv6_server(
    config: Ipv6Config,
    prefix_rx: watch::Receiver<Option<PrefixInfo>>,
) -> Result<()> {
    if !config.dhcpv6_enabled {
        info!("DHCPv6 server disabled");
        std::future::pending::<()>().await;
        return Ok(());
    }

    // Create socket with socket2 to join multicast group
    let sock = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    #[cfg(unix)]
    sock.set_reuse_port(true)?;
    sock.set_nonblocking(true)?;

    // Bind to DHCPv6 server port
    let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 547, 0, 0);
    sock.bind(&bind_addr.into())?;

    // Join DHCPv6 multicast group (ff02::1:2) on LAN interface
    let dhcpv6_multicast: Ipv6Addr = "ff02::1:2".parse().unwrap();
    let if_index = get_interface_index(&config.interface).unwrap_or(0);
    if let Err(e) = sock.join_multicast_v6(&dhcpv6_multicast, if_index) {
        warn!("Failed to join DHCPv6 multicast group on {}: {}", config.interface, e);
    } else {
        info!("Joined DHCPv6 multicast group ff02::1:2 on {} (index {})", config.interface, if_index);
    }

    let socket = UdpSocket::from_std(sock.into())?;
    info!("DHCPv6 stateful server listening on port 547");

    let lease_store = Arc::new(RwLock::new(Dhcpv6LeaseStore::load()));
    info!("Loaded {} DHCPv6 leases", lease_store.read().await.leases.len());

    let mut buf = [0u8; 1500];

    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                warn!("DHCPv6 recv error: {}", e);
                continue;
            }
        };

        if len < 4 {
            continue;
        }

        let msg_type = buf[0];
        let transaction_id = [buf[1], buf[2], buf[3]];
        let options = &buf[4..len];

        // Extract client DUID
        let client_duid = match extract_option(options, OPT_CLIENTID) {
            Some(d) => d,
            None => {
                debug!("DHCPv6: no client DUID in message type {}", msg_type);
                continue;
            }
        };

        // Get current GUA prefix
        let current_prefix = prefix_rx.borrow().clone();

        // Extract MAC from source link-local address (EUI-64)
        let client_mac = if let std::net::SocketAddr::V6(v6) = src {
            extract_mac_from_link_local(v6.ip())
        } else {
            None
        };

        let response = match msg_type {
            MSG_SOLICIT => {
                debug!("DHCPv6 SOLICIT from {:?}", src);
                handle_solicit(&client_duid, options, client_mac.clone(), &current_prefix, &config, &lease_store).await
            }
            MSG_REQUEST | MSG_RENEW | MSG_REBIND => {
                debug!("DHCPv6 REQUEST/RENEW/REBIND from {:?}", src);
                handle_request(&client_duid, options, client_mac.clone(), &current_prefix, &config, &lease_store).await
            }
            MSG_RELEASE => {
                debug!("DHCPv6 RELEASE from {:?}", src);
                handle_release(&client_duid, &lease_store).await
            }
            MSG_CONFIRM => {
                debug!("DHCPv6 CONFIRM from {:?}", src);
                handle_confirm(&client_duid, options, &current_prefix, &config, &lease_store).await
            }
            MSG_INFORMATION_REQUEST => {
                debug!("DHCPv6 INFORMATION-REQUEST from {:?}", src);
                Some(build_response(MSG_REPLY, &transaction_id, &client_duid, None, &config, None))
            }
            _ => {
                debug!("DHCPv6: ignoring message type {}", msg_type);
                None
            }
        };

        if let Some(mut reply) = response {
            // Update transaction ID in response
            reply[1] = transaction_id[0];
            reply[2] = transaction_id[1];
            reply[3] = transaction_id[2];

            if let Err(e) = socket.send_to(&reply, src).await {
                warn!("Failed to send DHCPv6 reply to {}: {}", src, e);
            } else {
                debug!("Sent DHCPv6 reply to {} ({} bytes)", src, reply.len());
            }
        }
    }
}

async fn handle_solicit(
    client_duid: &[u8],
    options: &[u8],
    client_mac: Option<String>,
    prefix: &Option<PrefixInfo>,
    config: &Ipv6Config,
    lease_store: &Arc<RwLock<Dhcpv6LeaseStore>>,
) -> Option<Vec<u8>> {
    let prefix = prefix.as_ref()?;

    // Extract IA_NA
    let ia_na_data = extract_option(options, OPT_IA_NA)?;
    let (iaid, _, _) = parse_ia_na(&ia_na_data)?;

    // Allocate address
    let mut store = lease_store.write().await;
    let lease = store.allocate(client_duid, iaid, client_mac, prefix, config)?;
    let _ = store.save();

    info!("DHCPv6 ADVERTISE: {} for DUID {}", lease.address, hex::encode(client_duid));

    Some(build_response(
        MSG_ADVERTISE,
        &[0, 0, 0],
        client_duid,
        Some(&lease),
        config,
        None,
    ))
}

async fn handle_request(
    client_duid: &[u8],
    options: &[u8],
    client_mac: Option<String>,
    prefix: &Option<PrefixInfo>,
    config: &Ipv6Config,
    lease_store: &Arc<RwLock<Dhcpv6LeaseStore>>,
) -> Option<Vec<u8>> {
    let prefix = prefix.as_ref()?;

    // Extract IA_NA
    let ia_na_data = extract_option(options, OPT_IA_NA)?;
    let (iaid, _, _) = parse_ia_na(&ia_na_data)?;

    // Allocate/renew address
    let mut store = lease_store.write().await;
    let lease = store.allocate(client_duid, iaid, client_mac, prefix, config)?;
    let _ = store.save();

    info!("DHCPv6 REPLY: {} for DUID {}", lease.address, hex::encode(client_duid));

    Some(build_response(
        MSG_REPLY,
        &[0, 0, 0],
        client_duid,
        Some(&lease),
        config,
        None,
    ))
}

async fn handle_release(
    client_duid: &[u8],
    lease_store: &Arc<RwLock<Dhcpv6LeaseStore>>,
) -> Option<Vec<u8>> {
    let mut store = lease_store.write().await;
    if store.release(client_duid) {
        let _ = store.save();
        info!("DHCPv6 RELEASE: DUID {}", hex::encode(client_duid));
    }

    // Always reply with success for RELEASE
    Some(build_response(
        MSG_REPLY,
        &[0, 0, 0],
        client_duid,
        None,
        &Ipv6Config::default(),
        Some((STATUS_SUCCESS, "Release confirmed")),
    ))
}

async fn handle_confirm(
    client_duid: &[u8],
    _options: &[u8],
    prefix: &Option<PrefixInfo>,
    config: &Ipv6Config,
    lease_store: &Arc<RwLock<Dhcpv6LeaseStore>>,
) -> Option<Vec<u8>> {
    let store = lease_store.read().await;

    // Check if client has a valid lease in current prefix
    if let Some(lease) = store.find_by_duid(client_duid) {
        if let Some(pfx) = prefix {
            let lease_prefix = &lease.address.octets()[..8];
            let current_prefix = &pfx.prefix.octets()[..8];

            if lease_prefix == current_prefix {
                // Address is still valid
                return Some(build_response(
                    MSG_REPLY,
                    &[0, 0, 0],
                    client_duid,
                    None,
                    config,
                    Some((STATUS_SUCCESS, "Address confirmed")),
                ));
            }
        }
    }

    // Address no longer valid
    Some(build_response(
        MSG_REPLY,
        &[0, 0, 0],
        client_duid,
        None,
        config,
        Some((STATUS_NO_BINDING, "Address not on link")),
    ))
}

/// Get the shared lease store for API access.
pub fn get_lease_store_path() -> &'static str {
    Dhcpv6LeaseStore::FILE_PATH
}

/// Extract MAC address from an IPv6 link-local address (EUI-64 format).
/// Returns None if not a valid EUI-64 link-local address.
fn extract_mac_from_link_local(addr: &Ipv6Addr) -> Option<String> {
    let octets = addr.octets();
    // Check if it's a link-local address (fe80::/10)
    if octets[0] != 0xfe || (octets[1] & 0xc0) != 0x80 {
        return None;
    }
    // EUI-64 format has ff:fe in the middle (bytes 11-12)
    if octets[11] != 0xff || octets[12] != 0xfe {
        return None;
    }
    // Extract MAC: flip bit 7 of first byte, skip ff:fe
    let mac = [
        octets[8] ^ 0x02,  // Flip universal/local bit
        octets[9],
        octets[10],
        // Skip octets[11] and octets[12] (ff:fe)
        octets[13],
        octets[14],
        octets[15],
    ];
    Some(format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]))
}

/// Extract MAC address from a DHCPv6 DUID.
/// Supports DUID-LLT (type 1) and DUID-LL (type 3) which contain link-layer addresses.
fn extract_mac_from_duid(duid: &[u8]) -> Option<String> {
    if duid.len() < 4 {
        return None;
    }
    let duid_type = u16::from_be_bytes([duid[0], duid[1]]);
    let hw_type = u16::from_be_bytes([duid[2], duid[3]]);

    // Only Ethernet (hw_type 1) has 6-byte MAC
    if hw_type != 1 {
        return None;
    }

    let mac_bytes = match duid_type {
        1 => {
            // DUID-LLT: 2 type + 2 hw + 4 time + 6 MAC = 14 bytes
            if duid.len() >= 14 {
                Some(&duid[8..14])
            } else {
                None
            }
        }
        3 => {
            // DUID-LL: 2 type + 2 hw + 6 MAC = 10 bytes
            if duid.len() >= 10 {
                Some(&duid[4..10])
            } else {
                None
            }
        }
        _ => None,
    }?;

    Some(format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac_bytes[0], mac_bytes[1], mac_bytes[2],
        mac_bytes[3], mac_bytes[4], mac_bytes[5]))
}

/// Get the interface index from its name.
fn get_interface_index(name: &str) -> Option<u32> {
    if name.is_empty() {
        return None;
    }
    // Read /sys/class/net/<name>/ifindex
    let path = format!("/sys/class/net/{}/ifindex", name);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}
