#!/bin/bash
###############################################################################
# provision-env-dev.sh — Provision the DEV environment container on Medion
#
# Wrapper around provision-env.sh with dev-specific settings:
#   - slug: dev
#   - IP: 10.0.0.200
#   - Installs dev tools (Rust, Node.js, pnpm, code-server)
#   - code-server on port 8443
#
# Run this ON the router (10.0.0.254), or from dev via:
#   ssh root@10.0.0.254 'bash -s' < scripts/provision-env-dev.sh
#
# Prerequisites:
#   - DHCP reservation for 10.0.0.200 configured in HomeRoute
#   - Bridge br0 on Medion
#   - debootstrap installed
#   - env-agent binary at /opt/homeroute/data/agent-binaries/env-agent
#
###############################################################################
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Forward all extra arguments (e.g. --dry-run)
exec "${SCRIPT_DIR}/provision-env.sh" \
    dev \
    10.0.0.200 \
    --host medion \
    --dev-tools \
    --code-server-port 8443 \
    "$@"
