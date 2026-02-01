#!/bin/bash
set -uo pipefail

echo "=== Tearing down LXC test infrastructure ==="

# Stop server if running
lxc exec dns-server -- bash -c 'kill $(pgrep -f rust-dns-dhcp) 2>/dev/null' 2>/dev/null || true
sleep 1

echo "[1/3] Deleting containers..."
lxc delete dns-client --force 2>/dev/null || true
lxc delete dns-server --force 2>/dev/null || true

echo "[2/3] Deleting network..."
lxc network delete testbr0 2>/dev/null || true

echo "[3/3] Cleaning up data..."
rm -rf /tmp/test-dns-dhcp-data

echo "=== Teardown complete ==="
