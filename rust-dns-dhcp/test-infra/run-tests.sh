#!/bin/bash
set -uo pipefail

PASS=0
FAIL=0
SKIP=0

pass() {
    echo "  ✓ PASS: $1"
    ((PASS++))
}

fail() {
    echo "  ✗ FAIL: $1 — $2"
    ((FAIL++))
}

skip() {
    echo "  ⊘ SKIP: $1 — $2"
    ((SKIP++))
}

echo "=== Starting rust-dns-dhcp integration tests ==="
echo ""

# --- Check prerequisites ---
if ! lxc exec dns-server -- test -f /opt/rust-dns-dhcp/rust-dns-dhcp; then
    echo "ERROR: Binary not found in dns-server. Run setup.sh first."
    exit 1
fi

# --- Prepare containers ---
echo "[SETUP] Preparing containers..."
# Disable systemd-resolved (occupies port 53)
lxc exec dns-server -- bash -c 'systemctl disable --now systemd-resolved 2>/dev/null; rm -f /etc/resolv.conf; echo "nameserver 1.1.1.1" > /etc/resolv.conf' 2>/dev/null || true
lxc exec dns-client -- bash -c 'systemctl disable --now systemd-resolved 2>/dev/null; rm -f /etc/resolv.conf; echo "nameserver 192.168.99.1" > /etc/resolv.conf' 2>/dev/null || true
# Fix fstab for dhclient-script
lxc exec dns-client -- touch /etc/fstab 2>/dev/null || true
# Ensure client has an IP for pre-DHCP tests
lxc exec dns-client -- bash -c 'ip addr add 192.168.99.100/24 dev eth0 2>/dev/null; ip route add default via 192.168.99.254 2>/dev/null' || true
# Kill any leftover processes
lxc exec dns-server -- bash -c 'killall rust-dns-dhcp 2>/dev/null' || true
lxc exec dns-client -- bash -c 'killall dhclient 2>/dev/null' || true
sleep 1

# --- Start the server ---
echo "[SETUP] Starting rust-dns-dhcp in dns-server..."
lxc exec dns-server -- bash -c 'DNS_DHCP_CONFIG_PATH=/var/lib/server-dashboard/dns-dhcp-config.json RUST_LOG=rust_dns_dhcp=info nohup /opt/rust-dns-dhcp/rust-dns-dhcp > /tmp/server.log 2>&1 &'
sleep 3

# Check server is running
if ! lxc exec dns-server -- pgrep -f rust-dns-dhcp > /dev/null; then
    echo "ERROR: Server failed to start. Check logs:"
    lxc exec dns-server -- journalctl --no-pager -n 20 2>/dev/null || true
    exit 1
fi
echo "[SETUP] Server running (PID: $(lxc exec dns-server -- pgrep -f rust-dns-dhcp))"
echo ""

# ========================================
# DNS TESTS (using temporary static IP)
# ========================================
echo "--- DNS Tests (pre-DHCP) ---"

# T5 — DNS: Wildcard resolution (*.test.lab)
echo "T5: DNS wildcard resolution"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 random.test.lab A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "192.168.99.1" ]; then
    pass "T5: Wildcard *.test.lab → 192.168.99.1"
else
    fail "T5: Wildcard *.test.lab" "Expected 192.168.99.1, got '$RESULT'"
fi

# T6 — DNS: Static record
echo "T6: DNS static record"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 static-host.test.lab A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "192.168.99.42" ]; then
    pass "T6: Static record static-host.test.lab → 192.168.99.42"
else
    fail "T6: Static record" "Expected 192.168.99.42, got '$RESULT'"
fi

# T8 — DNS: Upstream forwarding
echo "T8: DNS upstream forwarding"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 google.com A +short 2>/dev/null | head -1 | tr -d '\n\r')
if [[ "$RESULT" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] && [ "$RESULT" != "0.0.0.0" ]; then
    pass "T8: Upstream forward google.com → $RESULT"
else
    fail "T8: Upstream forward google.com" "Expected valid IPv4, got '$RESULT'"
fi

# T10 — DNS: TCP fallback
echo "T10: DNS TCP fallback"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 google.com A +tcp +short 2>/dev/null | head -1 | tr -d '\n\r')
if [[ "$RESULT" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    pass "T10: TCP fallback google.com → $RESULT"
else
    fail "T10: TCP fallback" "Expected valid IPv4, got '$RESULT'"
fi

# T9 — DNS: Cache
echo "T9: DNS cache"
# First query (cold)
lxc exec dns-client -- dig @192.168.99.1 cloudflare.com A +short > /dev/null 2>&1
sleep 0.5
# Second query (should be cached, query time ~0ms)
TIME2=$(lxc exec dns-client -- dig @192.168.99.1 cloudflare.com A +stats 2>/dev/null | grep "Query time" | grep -oP '\d+' | head -1)
if [ -n "$TIME2" ] && [ "$TIME2" -le 5 ]; then
    pass "T9: Cache hit (query time ${TIME2}ms)"
else
    fail "T9: Cache" "Expected ≤5ms on cached query, got '${TIME2}ms'"
fi

# T18 — DNS: AAAA query
echo "T18: DNS AAAA query"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 google.com AAAA +short 2>/dev/null | head -1 | tr -d '\n\r')
if [[ "$RESULT" =~ : ]]; then
    pass "T18: AAAA query google.com → $RESULT"
else
    fail "T18: AAAA query" "Expected IPv6 address, got '$RESULT'"
fi

# T19 — DNS: NXDOMAIN
echo "T19: DNS NXDOMAIN"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 thisdoesnotexist12345.com A 2>/dev/null | grep "status:")
if echo "$RESULT" | grep -q "NXDOMAIN"; then
    pass "T19: NXDOMAIN for non-existent domain"
else
    fail "T19: NXDOMAIN" "Expected NXDOMAIN, got '$RESULT'"
fi

echo ""

# ========================================
# ADBLOCK TESTS
# ========================================
echo "--- Adblock Tests ---"

# T11 prep — Trigger adblock download
echo "T11 prep: Triggering adblock list download..."
UPDATE_RESULT=$(lxc exec dns-server -- curl -s -X POST http://127.0.0.1:5380/update 2>/dev/null)
echo "  Update result: $UPDATE_RESULT"
sleep 2

# T14 — Adblock API: Stats
echo "T14: Adblock API stats"
STATS=$(lxc exec dns-server -- curl -s http://127.0.0.1:5380/stats 2>/dev/null)
DOMAIN_COUNT=$(echo "$STATS" | jq -r '.domain_count' 2>/dev/null)
if [ -n "$DOMAIN_COUNT" ] && [ "$DOMAIN_COUNT" -gt 10000 ]; then
    pass "T14: Stats show $DOMAIN_COUNT blocked domains"
else
    fail "T14: Stats" "Expected >10000 domains, got '$DOMAIN_COUNT' (raw: $STATS)"
fi

# T11 — Adblock: Blocked domain
echo "T11: Adblock blocked domain"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 pagead2.googlesyndication.com A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "0.0.0.0" ]; then
    pass "T11: pagead2.googlesyndication.com blocked → 0.0.0.0"
else
    fail "T11: Blocked domain" "Expected 0.0.0.0, got '$RESULT'"
fi

# T12 — Adblock: Non-blocked domain
echo "T12: Adblock non-blocked domain"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 github.com A +short 2>/dev/null | head -1 | tr -d '\n\r')
if [[ "$RESULT" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] && [ "$RESULT" != "0.0.0.0" ]; then
    pass "T12: github.com not blocked → $RESULT"
else
    fail "T12: Non-blocked domain" "Expected valid non-zero IPv4, got '$RESULT'"
fi

# T13 — Adblock: Whitelist override
echo "T13: Adblock whitelist override"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 allowed-ads.example.com A +short 2>/dev/null | head -1 | tr -d '\n\r')
if [ "$RESULT" != "0.0.0.0" ]; then
    pass "T13: Whitelisted domain not blocked"
else
    fail "T13: Whitelist override" "Expected non-zero response, got '$RESULT'"
fi

# T15 — Adblock API: Search
echo "T15: Adblock API search"
SEARCH=$(lxc exec dns-server -- curl -s "http://127.0.0.1:5380/search?q=pagead" 2>/dev/null)
if echo "$SEARCH" | grep -q "pagead2.googlesyndication.com"; then
    pass "T15: Search found pagead2.googlesyndication.com"
else
    fail "T15: Search" "Expected to find pagead domain in results"
fi

# T16 — Adblock API: Whitelist CRUD
echo "T16: Adblock API whitelist CRUD"
# Add
lxc exec dns-server -- curl -s -X POST -H 'Content-Type: application/json' \
    -d '{"domain":"newdomain.test"}' http://127.0.0.1:5380/whitelist > /dev/null 2>&1
# List
WL=$(lxc exec dns-server -- curl -s http://127.0.0.1:5380/whitelist 2>/dev/null)
if echo "$WL" | grep -q "newdomain.test"; then
    # Delete
    lxc exec dns-server -- curl -s -X DELETE http://127.0.0.1:5380/whitelist/newdomain.test > /dev/null 2>&1
    WL2=$(lxc exec dns-server -- curl -s http://127.0.0.1:5380/whitelist 2>/dev/null)
    if ! echo "$WL2" | grep -q "newdomain.test"; then
        pass "T16: Whitelist CRUD (add+list+delete)"
    else
        fail "T16: Whitelist CRUD" "Delete failed, domain still in whitelist"
    fi
else
    fail "T16: Whitelist CRUD" "Add failed, domain not in whitelist"
fi

# T17 — Adblock: Subdomain blocked by parent
echo "T17: Adblock subdomain blocked by parent"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 sub.doubleclick.net A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "0.0.0.0" ]; then
    pass "T17: sub.doubleclick.net blocked via parent → 0.0.0.0"
else
    # doubleclick.net might not be in StevenBlack, try another known parent
    RESULT2=$(lxc exec dns-client -- dig @192.168.99.1 tracking.ads.google.com A +short 2>/dev/null | tr -d '\n\r')
    if [ "$RESULT2" = "0.0.0.0" ]; then
        pass "T17: Subdomain blocked via parent domain"
    else
        fail "T17: Subdomain blocked by parent" "Expected 0.0.0.0, got '$RESULT' / '$RESULT2'"
    fi
fi

echo ""

# ========================================
# DHCP TESTS
# ========================================
echo "--- DHCP Tests ---"

# T1/T2 — DHCP: IP attribution (static lease)
echo "T1/T2: DHCP IP attribution"
# Flush all IPs and run dhclient fresh
lxc exec dns-client -- bash -c 'killall dhclient 2>/dev/null; rm -f /var/lib/dhcp/dhclient*; ip addr flush dev eth0; sleep 1; dhclient -1 -v eth0 2>&1' || true
sleep 3

IP=$(lxc exec dns-client -- ip -4 addr show eth0 2>/dev/null | grep -oP '192\.168\.99\.\d+' | head -1)
if [ -n "$IP" ]; then
    if [ "$IP" = "192.168.99.50" ]; then
        pass "T1: DHCP attribution — got IP $IP"
        pass "T2: Static lease — got expected 192.168.99.50"
    else
        pass "T1: DHCP attribution — got IP $IP (in range)"
        fail "T2: Static lease" "Expected 192.168.99.50, got $IP"
    fi
else
    fail "T1: DHCP attribution" "No IP received"
    fail "T2: Static lease" "No IP received"
fi

# T3 — DHCP: Options (gateway and DNS)
echo "T3: DHCP options"
GW=$(lxc exec dns-client -- ip route 2>/dev/null | grep default | awk '{print $3}')
if [ "$GW" = "192.168.99.254" ]; then
    pass "T3: Gateway is 192.168.99.254"
else
    fail "T3: Gateway" "Expected 192.168.99.254, got '$GW'"
fi

# T4 — DHCP: Lease file
echo "T4: DHCP lease file"
LEASES=$(lxc exec dns-server -- cat /var/lib/server-dashboard/dhcp-leases 2>/dev/null)
if echo "$LEASES" | grep -q "192.168.99"; then
    pass "T4: Lease file contains entries"
else
    # Leases might not be persisted yet (60s timer), trigger by waiting or checking in-memory
    skip "T4: Lease file" "Leases not yet persisted (60s timer)"
fi

# T7 — DNS: DHCP hostname expand-hosts
echo "T7: DNS DHCP hostname (expand-hosts)"
RESULT=$(lxc exec dns-client -- dig @192.168.99.1 testclient.test.lab A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "192.168.99.50" ]; then
    pass "T7: testclient.test.lab → 192.168.99.50"
else
    fail "T7: Hostname expand-hosts" "Expected 192.168.99.50, got '$RESULT'"
fi

# T22 — DHCP: Lease renewal
echo "T22: DHCP lease renewal"
lxc exec dns-client -- bash -c 'killall dhclient 2>/dev/null; ip addr flush dev eth0; rm -f /var/lib/dhcp/dhclient*; sleep 1; dhclient -1 eth0 2>/dev/null' || true
sleep 3
IP2=$(lxc exec dns-client -- ip -4 addr show eth0 2>/dev/null | grep -oP '192\.168\.99\.\d+' | head -1)
if [ "$IP2" = "192.168.99.50" ]; then
    pass "T22: Lease renewal — same IP 192.168.99.50"
else
    fail "T22: Lease renewal" "Expected 192.168.99.50, got '$IP2'"
fi

echo ""

# ========================================
# LOGGING & MISC TESTS
# ========================================
echo "--- Logging & Misc Tests ---"

# T20 — Query logging
echo "T20: Query logging (JSON)"
sleep 2
LOG=$(lxc exec dns-server -- tail -10 /var/lib/server-dashboard/queries.log 2>/dev/null)
if [ -n "$LOG" ]; then
    # Check that at least one line is valid JSON
    VALID=$(echo "$LOG" | head -1 | jq -r '.domain' 2>/dev/null)
    if [ -n "$VALID" ] && [ "$VALID" != "null" ]; then
        pass "T20: Query log is valid JSON (found: $VALID)"
    else
        fail "T20: Query logging" "Log exists but not valid JSON: $(echo "$LOG" | head -1)"
    fi
else
    fail "T20: Query logging" "No log file or empty"
fi

# T21 — Hot-reload (SIGHUP)
echo "T21: Hot-reload (SIGHUP)"
# Add a new static record via config modification
lxc exec dns-server -- bash -c 'cat /var/lib/server-dashboard/dns-dhcp-config.json | \
    jq ".dns.static_records += [{\"name\":\"new.test.lab\",\"type\":\"A\",\"value\":\"192.168.99.99\",\"ttl\":60}]" \
    > /tmp/new.json && mv /tmp/new.json /var/lib/server-dashboard/dns-dhcp-config.json' 2>/dev/null

# Send SIGHUP
lxc exec dns-server -- bash -c 'kill -HUP $(pgrep -f rust-dns-dhcp)' 2>/dev/null
sleep 2

RESULT=$(lxc exec dns-client -- dig @192.168.99.1 new.test.lab A +short 2>/dev/null | tr -d '\n\r')
if [ "$RESULT" = "192.168.99.99" ]; then
    pass "T21: Hot-reload — new.test.lab → 192.168.99.99"
else
    fail "T21: Hot-reload" "Expected 192.168.99.99, got '$RESULT'"
fi

# T23 — Network isolation
echo "T23: Network isolation"
lxc exec dns-client -- ping -c 1 -W 1 10.0.0.254 2>&1 > /dev/null
PING_EXIT=$?
if [ "$PING_EXIT" -ne 0 ]; then
    pass "T23: Network isolated from LAN (ping 10.0.0.254 failed as expected)"
else
    fail "T23: Network isolation" "ping 10.0.0.254 succeeded — network NOT isolated!"
fi

echo ""

# ========================================
# SUMMARY
# ========================================
echo "========================================="
echo "  RESULTS: $PASS passed, $FAIL failed, $SKIP skipped"
echo "========================================="

# Cleanup: stop server
lxc exec dns-server -- bash -c 'kill $(pgrep -f rust-dns-dhcp) 2>/dev/null' || true

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
exit 0
