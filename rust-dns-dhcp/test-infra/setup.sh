#!/bin/bash
set -euo pipefail

echo "=== Setting up LXC test infrastructure ==="

# Cleanup any previous test artifacts
echo "[1/8] Cleaning up previous test artifacts..."
lxc delete dns-client --force 2>/dev/null || true
lxc delete dns-server --force 2>/dev/null || true
lxc network delete testbr0 2>/dev/null || true
rm -rf /tmp/test-dns-dhcp-data

# Create isolated network bridge
echo "[2/8] Creating isolated network bridge testbr0..."
lxc network create testbr0 \
    ipv4.address=192.168.99.254/24 \
    ipv4.nat=true \
    ipv6.address=none \
    dns.mode=none \
    ipv4.dhcp=false

# Create data directory
echo "[3/8] Creating test data directory..."
mkdir -p /tmp/test-dns-dhcp-data/adblock
mkdir -p /tmp/test-dns-dhcp-data/log

# Launch dns-server container
echo "[4/8] Launching dns-server container..."
lxc launch ubuntu:24.04 dns-server --network testbr0
sleep 10

# Configure dns-server with static IP
echo "[5/8] Configuring dns-server..."
lxc exec dns-server -- bash -c 'cat > /etc/netplan/10-static.yaml << NETEOF
network:
  version: 2
  ethernets:
    eth0:
      addresses: [192.168.99.1/24]
      routes:
        - to: default
          via: 192.168.99.254
      nameservers:
        addresses: [1.1.1.1]
NETEOF
chmod 600 /etc/netplan/10-static.yaml
netplan apply'

sleep 3

# Mount binary and data into dns-server
lxc config device add dns-server rust-dns-dhcp disk \
    source=/opt/homeroute/rust-dns-dhcp/target/release \
    path=/opt/rust-dns-dhcp readonly=true

lxc config device add dns-server data disk \
    source=/tmp/test-dns-dhcp-data \
    path=/var/lib/server-dashboard

# Get dns-client MAC before launch for static lease
echo "[6/8] Launching dns-client container..."
lxc launch ubuntu:24.04 dns-client --network testbr0
sleep 10

# Get MAC address of dns-client
CLIENT_MAC=$(lxc exec dns-client -- ip link show eth0 | grep 'link/ether' | awk '{print $2}')
echo "dns-client MAC: $CLIENT_MAC"

# Write test config with actual MAC
echo "[7/8] Writing test configuration..."
cat > /tmp/test-dns-dhcp-data/dns-dhcp-config.json << CFGEOF
{
  "dns": {
    "listen_addresses": ["192.168.99.1"],
    "port": 53,
    "upstream_servers": ["1.1.1.1", "8.8.8.8"],
    "upstream_timeout_ms": 3000,
    "cache_size": 100,
    "local_domain": "test.lab",
    "wildcard_ipv4": "192.168.99.1",
    "wildcard_ipv6": "",
    "static_records": [
      { "name": "static-host.test.lab", "type": "A", "value": "192.168.99.42", "ttl": 300 }
    ],
    "expand_hosts": true,
    "query_log_path": "/var/lib/server-dashboard/queries.log"
  },
  "dhcp": {
    "enabled": true,
    "interface": "eth0",
    "range_start": "192.168.99.10",
    "range_end": "192.168.99.200",
    "netmask": "255.255.255.0",
    "gateway": "192.168.99.254",
    "dns_server": "192.168.99.1",
    "domain": "test.lab",
    "default_lease_time_secs": 300,
    "authoritative": true,
    "lease_file": "/var/lib/server-dashboard/dhcp-leases",
    "static_leases": [
      { "mac": "$CLIENT_MAC", "ip": "192.168.99.50", "hostname": "testclient" }
    ]
  },
  "ipv6": {
    "enabled": false,
    "ra_enabled": false,
    "ra_prefix": "",
    "ra_lifetime_secs": 0,
    "ra_managed_flag": false,
    "ra_other_flag": false,
    "dhcpv6_enabled": false,
    "dhcpv6_dns_servers": [],
    "interface": ""
  },
  "adblock": {
    "enabled": true,
    "block_response": "zero_ip",
    "api_port": 5380,
    "sources": [
      { "name": "StevenBlack", "url": "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts", "format": "hosts" }
    ],
    "whitelist": ["allowed-ads.example.com"],
    "data_dir": "/var/lib/server-dashboard/adblock",
    "auto_update_hours": 0
  }
}
CFGEOF

# Configure dns-client with temporary external DNS (for apt-get)
echo "[8/9] Configuring dns-client networking (temporary DNS for setup)..."
lxc exec dns-client -- bash -c 'cat > /etc/netplan/10-dhcp.yaml << NETEOF
network:
  version: 2
  ethernets:
    eth0:
      dhcp4: false
      addresses: [192.168.99.100/24]
      routes:
        - to: default
          via: 192.168.99.254
      nameservers:
        addresses: [1.1.1.1, 8.8.8.8]
NETEOF
chmod 600 /etc/netplan/10-dhcp.yaml
netplan apply'

sleep 3

# Install dig and other tools in dns-client
echo "[9/9] Installing test tools in dns-client..."
lxc exec dns-client -- bash -c 'apt-get update -qq && apt-get install -y -qq dnsutils isc-dhcp-client curl jq iputils-ping > /dev/null 2>&1'

# Now switch DNS to point at our test server
lxc exec dns-client -- bash -c 'cat > /etc/netplan/10-dhcp.yaml << NETEOF
network:
  version: 2
  ethernets:
    eth0:
      dhcp4: false
      addresses: [192.168.99.100/24]
      routes:
        - to: default
          via: 192.168.99.254
      nameservers:
        addresses: [192.168.99.1]
NETEOF
chmod 600 /etc/netplan/10-dhcp.yaml
netplan apply'

sleep 2

echo ""
echo "=== LXC test infrastructure ready ==="
echo "dns-server: 192.168.99.1 (static)"
echo "dns-client: 192.168.99.100 (temporary, will use DHCP)"
echo "dns-client MAC: $CLIENT_MAC"
echo "Network: testbr0 (192.168.99.0/24, isolated)"
echo ""
echo "To start the DNS/DHCP server:"
echo "  lxc exec dns-server -- /opt/rust-dns-dhcp/rust-dns-dhcp"
