#!/bin/bash
###############################################################################
# provision-env-prod.sh — Provision the PROD environment container on Medion
#
# Wrapper around provision-env.sh with prod-specific settings:
#   - slug: prod
#   - IP: 10.0.0.202
#   - No dev tools (no Rust, Node.js, pnpm, code-server)
#   - Minimal footprint for production workloads
#
# Run this ON the router (10.0.0.254), or from dev via:
#   ssh root@10.0.0.254 'bash -s' < scripts/provision-env-prod.sh
#
# Prerequisites:
#   - DHCP reservation for 10.0.0.202 configured in HomeRoute
#   - Bridge br0 on Medion
#   - debootstrap installed
#   - env-agent binary at /opt/homeroute/data/agent-binaries/env-agent
#
###############################################################################
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Forward all extra arguments (e.g. --dry-run)
exec "${SCRIPT_DIR}/provision-env.sh" \
    prod \
    10.0.0.202 \
    --host medion \
    "$@"
