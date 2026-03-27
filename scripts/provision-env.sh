#!/bin/bash
###############################################################################
# provision-env.sh — Generic environment provisioning for HomeRoute
#
# Creates a systemd-nspawn container for an environment (dev/prod/acc).
# Must be run ON the router (10.0.0.254) or via SSH to it.
#
# Usage:
#   ./provision-env.sh <slug> <ip_address> [options]
#
# Arguments:
#   slug          Environment slug: dev, prod, acc
#   ip_address    Static IP for the container (e.g. 10.0.0.200)
#
# Options:
#   --host NAME         Target host (default: medion) — for future multi-host
#   --dev-tools         Install Rust toolchain, Node.js, pnpm, code-server
#   --code-server-port  Port for code-server (default: 8443)
#   --dry-run           Print what would be done without executing
#   --help              Show this help
#
# Examples:
#   ./provision-env.sh dev 10.0.0.200 --dev-tools
#   ./provision-env.sh prod 10.0.0.202
#   ./provision-env.sh acc 10.0.0.201 --dry-run
#
# Prerequisites:
#   - debootstrap installed on the router
#   - systemd-nspawn / machinectl available
#   - Bridge br0 configured
#   - /opt/homeroute/data/agent-binaries/env-agent exists (or will be created)
#   - DHCP reservation for the chosen IP should be configured in HomeRoute
#
###############################################################################
set -euo pipefail

# =============================================================================
# Defaults
# =============================================================================
CONTAINER_PREFIX="env"
STORAGE_PATH="/var/lib/machines"
NSPAWN_DIR="/etc/systemd/nspawn"
BRIDGE="br0"
DNS_SERVER="10.0.0.254"
HOMEROUTE_ADDRESS="10.0.0.254"
HOMEROUTE_PORT=4001
AGENT_BINARY_SRC="/opt/homeroute/data/agent-binaries/env-agent"
CODE_SERVER_PORT=8443
DEV_TOOLS=false
DRY_RUN=false
HOST="medion"

# =============================================================================
# Argument parsing
# =============================================================================
usage() {
    sed -n '2,/^###/p' "$0" | head -n -1 | sed 's/^# \?//'
    exit 0
}

if [[ $# -lt 2 ]]; then
    echo "Error: missing required arguments."
    echo "Usage: $0 <slug> <ip_address> [options]"
    echo "Run $0 --help for details."
    exit 1
fi

SLUG="$1"; shift
IP_ADDRESS="$1"; shift

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)          HOST="$2"; shift 2 ;;
        --dev-tools)     DEV_TOOLS=true; shift ;;
        --code-server-port) CODE_SERVER_PORT="$2"; shift 2 ;;
        --dry-run)       DRY_RUN=true; shift ;;
        --help|-h)       usage ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

CONTAINER_NAME="${CONTAINER_PREFIX}-${SLUG}"
ROOTFS="${STORAGE_PATH}/${CONTAINER_NAME}"

# =============================================================================
# Helpers
# =============================================================================
log() { echo "[$(date '+%H:%M:%S')] $*"; }
warn() { echo "[$(date '+%H:%M:%S')] WARNING: $*" >&2; }
die() { echo "[$(date '+%H:%M:%S')] ERROR: $*" >&2; exit 1; }

run() {
    if $DRY_RUN; then
        echo "[DRY-RUN] $*"
    else
        log "Running: $*"
        "$@"
    fi
}

# Generate a 64-character hex token for the env-agent
generate_token() {
    openssl rand -hex 32
}

# =============================================================================
# Prerequisite checks
# =============================================================================
check_prerequisites() {
    log "Checking prerequisites..."

    if ! command -v debootstrap &>/dev/null; then
        die "debootstrap is not installed. Run: apt install debootstrap"
    fi

    if ! command -v machinectl &>/dev/null; then
        die "machinectl is not installed (systemd-container package)."
    fi

    if ! ip link show "$BRIDGE" &>/dev/null 2>&1; then
        die "Bridge $BRIDGE does not exist. Configure networking first."
    fi

    if [[ ! -f "$AGENT_BINARY_SRC" ]] && ! $DRY_RUN; then
        warn "env-agent binary not found at $AGENT_BINARY_SRC"
        warn "You will need to build and place it there before starting the agent."
        warn "  make env-agent && cp crates/target/release/env-agent $AGENT_BINARY_SRC"
    fi

    # Check if container already exists
    if [[ -d "$ROOTFS" ]]; then
        die "Container rootfs already exists at $ROOTFS. Remove it first or choose a different slug."
    fi

    log "Prerequisites OK."
}

# =============================================================================
# Step 1: Bootstrap rootfs
# =============================================================================
bootstrap_rootfs() {
    log "=== Step 1: Bootstrap Ubuntu 24.04 (Noble) rootfs ==="

    run debootstrap --variant=minbase noble "$ROOTFS" http://archive.ubuntu.com/ubuntu

    if $DRY_RUN; then return; fi

    # Empty machine-id (regenerated on first boot)
    echo "" > "${ROOTFS}/etc/machine-id"

    # Set hostname
    echo "$CONTAINER_NAME" > "${ROOTFS}/etc/hostname"

    log "Rootfs bootstrapped at $ROOTFS"
}

# =============================================================================
# Step 2: Configure networking
# =============================================================================
configure_networking() {
    log "=== Step 2: Configure networking ==="

    if $DRY_RUN; then
        echo "[DRY-RUN] Would configure DHCP on host0, DNS=$DNS_SERVER, static IP=$IP_ADDRESS"
        return
    fi

    # Enable systemd-networkd
    mkdir -p "${ROOTFS}/etc/systemd/system/multi-user.target.wants"
    ln -sf /lib/systemd/system/systemd-networkd.service \
        "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"

    # Mask systemd-resolved (we use static resolv.conf)
    ln -sf /dev/null "${ROOTFS}/etc/systemd/system/systemd-resolved.service"

    # Write networkd config for bridge interface (DHCP)
    mkdir -p "${ROOTFS}/etc/systemd/network"
    cat > "${ROOTFS}/etc/systemd/network/80-container.network" <<NETEOF
[Match]
Name=host0

[Network]
DHCP=yes

[DHCPv4]
UseHostname=false
UseDNS=no
UseDomains=no
NETEOF

    # Static resolv.conf pointing to HomeRoute DNS
    rm -f "${ROOTFS}/etc/resolv.conf"
    cat > "${ROOTFS}/etc/resolv.conf" <<DNSEOF
nameserver ${DNS_SERVER}
nameserver 8.8.8.8
options edns0 timeout:2 attempts:3
DNSEOF

    # Disable IPv6 (containers lack IPv6 routing)
    mkdir -p "${ROOTFS}/etc/sysctl.d"
    cat > "${ROOTFS}/etc/sysctl.d/99-disable-ipv6.conf" <<SYSCTLEOF
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
SYSCTLEOF

    # Force curl IPv4
    echo "--ipv4" > "${ROOTFS}/root/.curlrc"

    # Prefer IPv4 in getaddrinfo
    echo "precedence ::ffff:0:0/96  100" > "${ROOTFS}/etc/gai.conf"

    log "Networking configured (DHCP on host0, DNS=$DNS_SERVER)"
}

# =============================================================================
# Step 3: Write .nspawn unit
# =============================================================================
write_nspawn_unit() {
    log "=== Step 3: Write .nspawn unit ==="

    if $DRY_RUN; then
        echo "[DRY-RUN] Would write ${NSPAWN_DIR}/${CONTAINER_NAME}.nspawn"
        return
    fi

    mkdir -p "$NSPAWN_DIR"
    cat > "${NSPAWN_DIR}/${CONTAINER_NAME}.nspawn" <<NSPAWNEOF
[Exec]
Boot=yes
PrivateUsers=no

[Network]
Bridge=${BRIDGE}
NSPAWNEOF

    log "Nspawn unit written: ${NSPAWN_DIR}/${CONTAINER_NAME}.nspawn"
}

# =============================================================================
# Step 4: Install base packages
# =============================================================================
install_base_packages() {
    log "=== Step 4: Install base packages ==="

    local packages="dbus systemd-sysv iproute2 curl ca-certificates sqlite3 tmux git e2fsprogs"

    if $DRY_RUN; then
        echo "[DRY-RUN] Would chroot-install: $packages"
        return
    fi

    chroot "$ROOTFS" /bin/bash -c "
        apt-get update -qq 2>/dev/null
        apt-get install -y -qq $packages 2>/dev/null
        systemctl enable systemd-networkd 2>/dev/null || true
        systemctl mask systemd-resolved 2>/dev/null || true
        chattr +i /etc/resolv.conf 2>/dev/null || true
    "

    log "Base packages installed"
}

# =============================================================================
# Step 5: Install dev tools (Rust, Node.js, pnpm, code-server) — dev only
# =============================================================================
install_dev_tools() {
    if ! $DEV_TOOLS; then
        log "=== Step 5: Skipping dev tools (not a dev environment) ==="
        return
    fi

    log "=== Step 5: Install dev tools (Rust, Node.js, pnpm, code-server) ==="

    if $DRY_RUN; then
        echo "[DRY-RUN] Would install Rust toolchain, Node.js 22, pnpm, code-server"
        return
    fi

    chroot "$ROOTFS" /bin/bash -c "
        set -euo pipefail

        # --- Rust toolchain ---
        echo '>>> Installing Rust toolchain...'
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable 2>/dev/null
        echo 'source /root/.cargo/env' >> /root/.bashrc

        # --- Node.js 22 LTS ---
        echo '>>> Installing Node.js 22...'
        curl -fsSL https://deb.nodesource.com/setup_22.x | bash - 2>/dev/null
        apt-get install -y -qq nodejs 2>/dev/null

        # --- pnpm ---
        echo '>>> Installing pnpm...'
        npm install -g pnpm 2>/dev/null

        # --- code-server ---
        echo '>>> Installing code-server...'
        curl -fsSL https://code-server.dev/install.sh | sh 2>/dev/null

        # Configure code-server to listen on all interfaces
        mkdir -p /root/.config/code-server
        cat > /root/.config/code-server/config.yaml <<'CSEOF'
bind-addr: 0.0.0.0:${CODE_SERVER_PORT}
auth: none
cert: false
CSEOF

        # Create code-server systemd service
        cat > /etc/systemd/system/code-server.service <<'CSVCEOF'
[Unit]
Description=code-server (Studio)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/bin/code-server --config /root/.config/code-server/config.yaml /apps
Restart=always
RestartSec=5
Environment=HOME=/root

[Install]
WantedBy=multi-user.target
CSVCEOF

        systemctl enable code-server 2>/dev/null || true
    "

    # Fix code-server config (the heredoc inside chroot doesn't expand shell vars)
    cat > "${ROOTFS}/root/.config/code-server/config.yaml" <<CSEOF
bind-addr: 0.0.0.0:${CODE_SERVER_PORT}
auth: none
cert: false
CSEOF

    log "Dev tools installed (Rust, Node.js 22, pnpm, code-server on :${CODE_SERVER_PORT})"
}

# =============================================================================
# Step 6: Install env-agent
# =============================================================================
install_env_agent() {
    log "=== Step 6: Install env-agent ==="

    local TOKEN
    TOKEN=$(generate_token)

    if $DRY_RUN; then
        echo "[DRY-RUN] Would copy env-agent binary to container"
        echo "[DRY-RUN] Would write /etc/env-agent.toml with token=${TOKEN:0:8}..."
        echo "[DRY-RUN] Would create env-agent.service"
        return
    fi

    # Copy binary (if available)
    if [[ -f "$AGENT_BINARY_SRC" ]]; then
        cp "$AGENT_BINARY_SRC" "${ROOTFS}/usr/local/bin/env-agent"
        chmod +x "${ROOTFS}/usr/local/bin/env-agent"
        log "env-agent binary copied"
    else
        warn "env-agent binary not found — skipping binary copy."
        warn "Place it manually: cp env-agent ${ROOTFS}/usr/local/bin/env-agent"
    fi

    # Write env-agent.toml
    cat > "${ROOTFS}/etc/env-agent.toml" <<TOMLEOF
# env-agent configuration for environment: ${SLUG}
# Generated on $(date -Iseconds)

# HomeRoute orchestrator connection
homeroute_address = "${HOMEROUTE_ADDRESS}"
homeroute_port = ${HOMEROUTE_PORT}

# Authentication token (64-char hex)
token = "${TOKEN}"

# Environment slug
env_slug = "${SLUG}"

# Network interface for IP detection
interface = "host0"

# Ports
mcp_port = 4010
code_server_port = ${CODE_SERVER_PORT}

# Apps directory
apps_path = "/apps"

# Database directory (one SQLite DB per app)
db_path = "/opt/env-agent/data/db"
TOMLEOF

    log "env-agent.toml written (token: ${TOKEN:0:8}...)"
    log "IMPORTANT: Save this token — you will need it to register the env-agent in HomeRoute:"
    log "  Token: ${TOKEN}"

    # Write env-agent systemd service
    cat > "${ROOTFS}/etc/systemd/system/env-agent.service" <<SVCEOF
[Unit]
Description=HomeRoute Environment Agent (${SLUG})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/env-agent
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
SVCEOF

    # Enable the service
    chroot "$ROOTFS" systemctl enable env-agent 2>/dev/null || true

    log "env-agent.service created and enabled"
}

# =============================================================================
# Step 7: Create directory structure
# =============================================================================
create_directories() {
    log "=== Step 7: Create directory structure ==="

    if $DRY_RUN; then
        echo "[DRY-RUN] Would create /apps/, /opt/env-agent/data/db/"
        return
    fi

    mkdir -p "${ROOTFS}/apps"
    mkdir -p "${ROOTFS}/opt/env-agent/data/db"
    mkdir -p "${ROOTFS}/opt/env-agent/logs"

    log "Directories created: /apps, /opt/env-agent/data/db, /opt/env-agent/logs"
}

# =============================================================================
# Step 8: Start and verify
# =============================================================================
start_and_verify() {
    log "=== Step 8: Start container and verify ==="

    if $DRY_RUN; then
        echo "[DRY-RUN] Would run: systemctl enable systemd-nspawn@${CONTAINER_NAME}"
        echo "[DRY-RUN] Would run: machinectl start ${CONTAINER_NAME}"
        echo "[DRY-RUN] Would wait for container to come up"
        return
    fi

    # Enable the container to start on boot
    run systemctl enable "systemd-nspawn@${CONTAINER_NAME}.service"

    # Start the container
    run machinectl start "$CONTAINER_NAME"

    # Wait for it to come up (check host0 interface)
    log "Waiting for container to come up..."
    local ready=false
    for i in $(seq 1 30); do
        if machinectl shell "$CONTAINER_NAME" /bin/bash -c "ip link show host0" 2>/dev/null | grep -q "UP"; then
            ready=true
            break
        fi
        sleep 1
    done

    if $ready; then
        log "Container is running!"

        # Get the actual IP
        local actual_ip
        actual_ip=$(machinectl shell "$CONTAINER_NAME" /bin/bash -c \
            "ip -4 addr show host0 | grep -oP '(?<=inet\s)\d+(\.\d+){3}'" 2>/dev/null | tail -1 || echo "unknown")

        log "Container IP: $actual_ip (expected: $IP_ADDRESS)"
        if [[ "$actual_ip" != "$IP_ADDRESS" ]] && [[ "$actual_ip" != "unknown" ]]; then
            warn "IP mismatch! Expected $IP_ADDRESS but got $actual_ip."
            warn "Check DHCP reservation in HomeRoute for MAC of ${CONTAINER_NAME}."
        fi
    else
        warn "Container network not ready after 30s. It may still be booting."
        warn "Check with: machinectl status $CONTAINER_NAME"
    fi
}

# =============================================================================
# Summary
# =============================================================================
print_summary() {
    log ""
    log "============================================="
    log " Environment provisioning complete!"
    log "============================================="
    log ""
    log "  Container:    $CONTAINER_NAME"
    log "  Rootfs:       $ROOTFS"
    log "  Expected IP:  $IP_ADDRESS"
    log "  Host:         $HOST"
    log "  Dev tools:    $DEV_TOOLS"
    log ""
    log "  env-agent config:  /etc/env-agent.toml (inside container)"
    log "  Apps directory:    /apps/ (inside container)"
    log "  DB directory:      /opt/env-agent/data/db/ (inside container)"
    log ""
    if $DEV_TOOLS; then
        log "  code-server:       http://${IP_ADDRESS}:${CODE_SERVER_PORT}"
        log "  Rust:              /root/.cargo/bin/rustc (inside container)"
        log "  Node.js:           /usr/bin/node (inside container)"
        log "  pnpm:              /usr/bin/pnpm (inside container)"
        log ""
    fi
    log "Next steps:"
    log "  1. Verify DHCP reservation for IP $IP_ADDRESS in HomeRoute"
    log "  2. Register the env-agent token in HomeRoute orchestrator"
    log "  3. Configure DNS wildcard: *.${SLUG}.mynetwk.biz -> $IP_ADDRESS"
    log "  4. Configure TLS wildcard cert for *.${SLUG}.mynetwk.biz"
    log "  5. Migrate apps: ./migrate-apps-to-env.sh $SLUG trader wallet home files"
    log ""
}

# =============================================================================
# Main
# =============================================================================
main() {
    log "Provisioning environment: $CONTAINER_NAME (IP: $IP_ADDRESS)"
    log "Host: $HOST | Dev tools: $DEV_TOOLS | Dry run: $DRY_RUN"
    log ""

    check_prerequisites
    bootstrap_rootfs
    configure_networking
    write_nspawn_unit
    install_base_packages
    install_dev_tools
    install_env_agent
    create_directories
    start_and_verify
    print_summary
}

main
