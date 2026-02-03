//! Router Advertisement sender via raw ICMPv6 socket.
//!
//! Sends the GUA prefix from DHCPv6-PD with SLAAC enabled (A=1, M=1).
//! Android uses SLAAC; Windows/Linux can use DHCPv6 stateful.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::Duration;

use anyhow::Result;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::watch;
use tracing::{info, warn, error};

use crate::config::Ipv6Config;
use crate::pd_client::PrefixInfo;

/// Assign a GUA address (<prefix>::1) to the LAN interface.
async fn assign_lan_gua(interface: &str, prefix: &PrefixInfo) {
    let mut octets = prefix.prefix.octets();
    octets[15] = 1;
    let addr = Ipv6Addr::from(octets);
    let cidr = format!("{}/{}", addr, prefix.prefix_len);

    let output = tokio::process::Command::new("ip")
        .args(["-6", "addr", "add", &cidr, "dev", interface])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => info!("Assigned GUA {} to {}", cidr, interface),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.contains("File exists") {
                error!("Failed to assign GUA {} to {}: {}", cidr, interface, stderr);
            }
        }
        Err(e) => error!("Failed to run ip command: {}", e),
    }
}

/// Remove a GUA address from the LAN interface.
async fn remove_lan_gua(interface: &str, prefix: &PrefixInfo) {
    let mut octets = prefix.prefix.octets();
    octets[15] = 1;
    let addr = Ipv6Addr::from(octets);
    let cidr = format!("{}/{}", addr, prefix.prefix_len);

    let output = tokio::process::Command::new("ip")
        .args(["-6", "addr", "del", &cidr, "dev", interface])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => info!("Removed GUA {} from {}", cidr, interface),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.contains("Cannot assign") {
                warn!("Failed to remove GUA from {}: {}", interface, stderr);
            }
        }
        Err(e) => warn!("Failed to run ip command: {}", e),
    }
}

/// A prefix to include in the Router Advertisement.
struct PrefixOption {
    addr: Ipv6Addr,
    len: u8,
    valid_lifetime: u32,
    preferred_lifetime: u32,
}

/// Build an ICMPv6 Router Advertisement packet with multiple prefixes.
/// SLAAC is disabled (A=0), clients must use DHCPv6 stateful (M=1).
fn build_ra_packet(config: &Ipv6Config, prefixes: &[PrefixOption]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    // ICMPv6 header
    buf.push(134); // Type: Router Advertisement
    buf.push(0);   // Code
    buf.extend_from_slice(&[0, 0]); // Checksum (kernel computes for us)

    // RA fields
    buf.push(64);  // Cur Hop Limit
    // M=1 (Managed - use DHCPv6 for addresses), O=1 (Other - use DHCPv6 for DNS)
    let flags: u8 = 0x80 | 0x40; // M=1, O=1
    buf.push(flags);
    buf.extend_from_slice(&config.ra_lifetime_secs.to_be_bytes()[2..4]); // Router Lifetime (16-bit)
    buf.extend_from_slice(&0u32.to_be_bytes()); // Reachable Time
    buf.extend_from_slice(&0u32.to_be_bytes()); // Retrans Timer

    // Prefix Information Options
    for pfx in prefixes {
        buf.push(3);   // Type: Prefix Information
        buf.push(4);   // Length: 4 (in units of 8 bytes = 32 bytes)
        buf.push(pfx.len);
        buf.push(0xC0); // Flags: L=1 (on-link), A=1 (autonomous/SLAAC enabled for Android)
        buf.extend_from_slice(&pfx.valid_lifetime.to_be_bytes());
        buf.extend_from_slice(&pfx.preferred_lifetime.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // Reserved
        buf.extend_from_slice(&pfx.addr.octets());
    }

    // RDNSS Option (type=25) — Recursive DNS Server
    for dns_str in &config.dhcpv6_dns_servers {
        if let Ok(dns_ip) = dns_str.parse::<Ipv6Addr>() {
            buf.push(25);  // Type: RDNSS
            buf.push(3);   // Length: 3 (= 24 bytes: 8 header + 16 address)
            buf.extend_from_slice(&[0, 0]); // Reserved
            buf.extend_from_slice(&config.ra_lifetime_secs.to_be_bytes()); // Lifetime
            buf.extend_from_slice(&dns_ip.octets());
        }
    }

    buf
}


/// Collect the list of prefixes to advertise.
/// Only the GUA prefix from Starlink PD is advertised (no ULA).
fn collect_prefixes(_config: &Ipv6Config, gua: &Option<PrefixInfo>) -> Vec<PrefixOption> {
    let mut prefixes = Vec::with_capacity(1);

    // Only GUA prefix from PD - no ULA, DHCPv6 stateful handles addressing
    if let Some(pd) = gua {
        prefixes.push(PrefixOption {
            addr: pd.prefix,
            len: pd.prefix_len,
            valid_lifetime: pd.valid_lifetime,
            preferred_lifetime: pd.preferred_lifetime,
        });
    }

    prefixes
}

/// Build a deprecation packet: announces the old GUA prefix with lifetime=0.
fn build_deprecation_packet(config: &Ipv6Config, old_prefix: &PrefixInfo) -> Vec<u8> {
    // Deprecate old GUA prefix
    let prefixes = vec![PrefixOption {
        addr: old_prefix.prefix,
        len: old_prefix.prefix_len,
        valid_lifetime: 0,
        preferred_lifetime: 0,
    }];

    build_ra_packet(config, &prefixes)
}

/// Send periodic Router Advertisements with dynamic prefix support.
pub async fn run_ra_sender(
    config: Ipv6Config,
    mut prefix_rx: watch::Receiver<Option<PrefixInfo>>,
) -> Result<()> {
    if !config.ra_enabled {
        info!("Router Advertisements disabled");
        std::future::pending::<()>().await;
        return Ok(());
    }

    info!("Starting Router Advertisement sender (SLAAC+DHCPv6 mode, M=1, A=1)");

    let socket = Socket::new(Domain::IPV6, Type::RAW, Some(Protocol::ICMPV6))?;
    socket.set_multicast_hops_v6(255)?;

    if !config.interface.is_empty() {
        #[cfg(target_os = "linux")]
        socket.bind_device(Some(config.interface.as_bytes()))?;
    }

    socket.set_nonblocking(true)?;
    let socket = tokio::net::UdpSocket::from_std(socket.into())?;

    let dest = SocketAddrV6::new("ff02::1".parse().unwrap(), 0, 0, 0);
    let interval_secs = (config.ra_lifetime_secs / 3).max(200);

    info!("RA sender: sending every {}s to ff02::1", interval_secs);

    let mut last_gua: Option<PrefixInfo> = None;
    let lan_iface = config.interface.clone();

    // Assign GUA to LAN if prefix already available at startup
    {
        let initial = prefix_rx.borrow().clone();
        if let Some(ref info) = initial {
            assign_lan_gua(&lan_iface, info).await;
        }
    }

    loop {
        // Build and send RA with current prefixes
        let current_gua = prefix_rx.borrow().clone();
        let prefixes = collect_prefixes(&config, &current_gua);
        let ra_packet = build_ra_packet(&config, &prefixes);

        match socket.send_to(&ra_packet, std::net::SocketAddr::V6(dest)).await {
            Ok(_) => {
                let prefix_names: Vec<String> = prefixes.iter()
                    .map(|p| format!("{}/{}", p.addr, p.len))
                    .collect();
                info!("Sent RA ({} bytes, prefixes: {:?})", ra_packet.len(), prefix_names);
            }
            Err(e) => {
                warn!("Failed to send RA: {}", e);
            }
        }

        last_gua = current_gua;

        // Wait for either: periodic timer or prefix change
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_secs as u64)) => {}
            _ = prefix_rx.changed() => {
                let new_gua = prefix_rx.borrow().clone();

                // Manage GUA address on LAN interface
                match (&new_gua, &last_gua) {
                    (Some(new), None) => {
                        // New prefix: assign address
                        assign_lan_gua(&lan_iface, new).await;
                    }
                    (Some(new), Some(old)) if new.prefix != old.prefix => {
                        // Prefix changed: remove old, assign new
                        remove_lan_gua(&lan_iface, old).await;
                        assign_lan_gua(&lan_iface, new).await;
                    }
                    (None, Some(old)) => {
                        // Prefix withdrawn: remove address
                        remove_lan_gua(&lan_iface, old).await;
                    }
                    _ => {}
                }

                // If prefix was withdrawn, send deprecation for old prefix
                if new_gua.is_none() {
                    if let Some(ref old) = last_gua {
                        info!("GUA prefix withdrawn, sending deprecation RA");
                        let deprecation = build_deprecation_packet(&config, old);
                        let _ = socket.send_to(&deprecation, std::net::SocketAddr::V6(dest)).await;
                    }
                } else {
                    info!("GUA prefix changed, sending rapid RAs");
                }

                // RFC 4861 §6.2.4: send 3 rapid RAs on prefix change
                for i in 0..3 {
                    let gua = prefix_rx.borrow().clone();
                    let pfx = collect_prefixes(&config, &gua);
                    let pkt = build_ra_packet(&config, &pfx);
                    let _ = socket.send_to(&pkt, std::net::SocketAddr::V6(dest)).await;
                    if i < 2 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}
