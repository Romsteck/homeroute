#!/bin/bash
###############################################################################
# setup-studio.sh — Install code-server as the global HomeRoute Studio
#
# After the env teardown, there is a single code-server instance on the host,
# exposed via studio.mynetwk.biz. This script:
#   1. Ensures /opt/homeroute/apps/ exists.
#   2. Installs code-server (via the official install.sh) if missing.
#   3. Drops /etc/systemd/system/hr-studio.service (heredoc — no separate file).
#   4. Enables + starts hr-studio.service.
#   5. Smoke-tests it on 127.0.0.1:8443.
#   6. Asks the homeroute API to register the studio.mynetwk.biz proxy route.
#   7. Drops a stub /opt/homeroute/apps/CLAUDE.md so the workspace is non-empty
#      on the very first boot.
#
# Idempotent — safe to rerun.
#
# Usage:
#   sudo ./setup-studio.sh           # apply
#   ./setup-studio.sh --dry-run      # print actions only
#   ./setup-studio.sh --help
#
# Run on the prod router (10.0.0.254).
#
# Design choices:
#   - Runs as **root**. A dedicated `homeroute-studio` user would be safer, but
#     all the other homeroute-managed services run as root for parity, and the
#     studio frequently needs to read/write under /opt/homeroute/apps which is
#     also root-owned. Documented here so the next maintainer doesn't wonder.
#   - code-server is launched with `--auth none`. We do NOT want code-server's
#     own password screen — auth is enforced one layer up by hr-edge using the
#     hr-auth session cookie on `studio.mynetwk.biz`. Binding only to 127.0.0.1
#     guarantees the only ingress is through the proxy.
###############################################################################
set -euo pipefail

# =============================================================================
# Defaults
# =============================================================================
APPS_DIR="${APPS_DIR:-/opt/homeroute/apps}"
DATA_DIR="${DATA_DIR:-/opt/homeroute/data/code-server}"
SERVICE_FILE="${SERVICE_FILE:-/etc/systemd/system/hr-studio.service}"
STUDIO_BIND="${STUDIO_BIND:-127.0.0.1:8443}"
STUDIO_PORT="${STUDIO_PORT:-8443}"
STUDIO_DOMAIN="${STUDIO_DOMAIN:-studio.mynetwk.biz}"
HOMEROUTE_API="${HOMEROUTE_API:-http://127.0.0.1:4000}"
DRY_RUN=false

# =============================================================================
# Logging helpers
# =============================================================================
if [[ -t 1 ]]; then
    C_RESET=$'\033[0m'
    C_INFO=$'\033[36m'
    C_OK=$'\033[32m'
    C_WARN=$'\033[33m'
    C_ERR=$'\033[31m'
else
    C_RESET=""; C_INFO=""; C_OK=""; C_WARN=""; C_ERR=""
fi

_ts() { date '+%H:%M:%S'; }
log_info()  { printf '%s[%s] [INFO]  %s%s\n'  "$C_INFO" "$(_ts)" "$*" "$C_RESET"; }
log_ok()    { printf '%s[%s] [ OK ]  %s%s\n'  "$C_OK"   "$(_ts)" "$*" "$C_RESET"; }
log_warn()  { printf '%s[%s] [WARN]  %s%s\n'  "$C_WARN" "$(_ts)" "$*" "$C_RESET" >&2; }
log_error() { printf '%s[%s] [ERROR] %s%s\n'  "$C_ERR"  "$(_ts)" "$*" "$C_RESET" >&2; }
die()       { log_error "$*"; exit 1; }

# =============================================================================
# Argument parsing
# =============================================================================
usage() {
    sed -n '2,/^###/p' "$0" | head -n -1 | sed 's/^# \?//'
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=true; shift ;;
        --help|-h) usage ;;
        *) die "Unknown option: $1 (use --help)" ;;
    esac
done

run() {
    if $DRY_RUN; then
        log_info "DRY-RUN: $*"
    else
        eval "$@"
    fi
}

# =============================================================================
# Preflight
# =============================================================================
preflight() {
    log_info "Preflight checks…"

    if ! $DRY_RUN && [[ "$(id -u)" -ne 0 ]]; then
        die "must run as root (or use --dry-run)"
    fi

    for bin in curl systemctl; do
        command -v "$bin" >/dev/null 2>&1 || die "missing required binary: $bin"
    done

    if [[ ! -d "$APPS_DIR" ]]; then
        log_info "creating $APPS_DIR (root:root 0755)"
        run "mkdir -p '$APPS_DIR'"
        run "chown root:root '$APPS_DIR'"
        run "chmod 0755 '$APPS_DIR'"
    else
        log_ok "$APPS_DIR already exists"
    fi

    if [[ ! -d "$DATA_DIR" ]]; then
        log_info "creating $DATA_DIR (code-server user-data-dir)"
        run "mkdir -p '$DATA_DIR'"
    fi

    log_ok "preflight passed"
}

# =============================================================================
# Install code-server
# =============================================================================
install_code_server() {
    if command -v code-server >/dev/null 2>&1; then
        local version
        version="$(code-server --version 2>/dev/null | head -n1 || echo unknown)"
        log_ok "code-server already installed: $version"
        return 0
    fi

    log_info "code-server not found — installing via official script"
    if $DRY_RUN; then
        log_info "DRY-RUN: curl -fsSL https://code-server.dev/install.sh | sh"
        return 0
    fi

    curl -fsSL https://code-server.dev/install.sh | sh \
        || die "code-server install failed"

    command -v code-server >/dev/null 2>&1 \
        || die "code-server still not on PATH after install"

    log_ok "code-server installed: $(code-server --version 2>/dev/null | head -n1)"
}

# =============================================================================
# Write the systemd unit (heredoc — no separate config/ file)
# =============================================================================
write_unit() {
    local tmp content
    tmp="$(mktemp)"
    cat >"$tmp" <<UNIT
[Unit]
Description=HomeRoute Studio (code-server)
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=${APPS_DIR}
Environment=PASSWORD=
Environment=HASHED_PASSWORD=
ExecStart=/usr/bin/code-server \\
  --bind-addr ${STUDIO_BIND} \\
  --auth none \\
  --disable-telemetry \\
  --disable-update-check \\
  --user-data-dir ${DATA_DIR} \\
  ${APPS_DIR}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
UNIT

    if [[ -f "$SERVICE_FILE" ]] && cmp -s "$tmp" "$SERVICE_FILE"; then
        log_ok "$SERVICE_FILE already up to date"
        rm -f "$tmp"
        return 0
    fi

    if $DRY_RUN; then
        log_info "DRY-RUN: would install $SERVICE_FILE with content:"
        sed 's/^/    /' "$tmp"
        rm -f "$tmp"
        return 0
    fi

    install -m 0644 "$tmp" "$SERVICE_FILE"
    rm -f "$tmp"
    log_ok "wrote $SERVICE_FILE"
}

# =============================================================================
# Enable + start + smoke test
# =============================================================================
enable_and_start() {
    run "systemctl daemon-reload"
    run "systemctl enable --now hr-studio.service"

    if $DRY_RUN; then
        log_info "DRY-RUN: skipping smoke test"
        return 0
    fi

    sleep 1
    if ! systemctl is-active --quiet hr-studio.service; then
        systemctl status --no-pager hr-studio.service || true
        die "hr-studio.service failed to start"
    fi
    log_ok "hr-studio.service is active"

    local http
    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            "http://${STUDIO_BIND}/healthz" || echo "000")"
    case "$http" in
        200|204|302|401)
            log_ok "code-server responded on http://${STUDIO_BIND}/healthz (HTTP $http)"
            ;;
        000)
            log_warn "could not reach http://${STUDIO_BIND}/healthz — service may still be warming up"
            ;;
        *)
            log_warn "code-server /healthz returned HTTP $http"
            ;;
    esac
}

# =============================================================================
# Register edge route for studio.mynetwk.biz
# TODO(wave3): confirm /api/edge/routes shape with hr-api maintainer.
# =============================================================================
register_route() {
    local payload http
    payload="$(printf '{"domain":"%s","target":"127.0.0.1:%s","auth_required":true}' \
                "$STUDIO_DOMAIN" "$STUDIO_PORT")"

    if $DRY_RUN; then
        log_info "DRY-RUN: POST $HOMEROUTE_API/api/edge/routes $payload"
        return 0
    fi

    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            -X POST "$HOMEROUTE_API/api/edge/routes" \
            -H 'Content-Type: application/json' \
            -d "$payload" || echo "000")"

    case "$http" in
        200|201|204)
            log_ok "edge route registered: $STUDIO_DOMAIN → 127.0.0.1:$STUDIO_PORT"
            ;;
        404)
            log_warn "POST /api/edge/routes returned 404 — endpoint not yet wired"
            log_warn "  → run scripts/migrate-routes.sh once the API is in place"
            ;;
        *)
            log_warn "POST /api/edge/routes failed: HTTP $http"
            log_warn "  → run scripts/migrate-routes.sh after the API is fixed"
            ;;
    esac
}

# =============================================================================
# Stub CLAUDE.md so the workspace is non-empty on first open
# =============================================================================
write_stub_claude_md() {
    local stub="${APPS_DIR}/CLAUDE.md"
    if [[ -f "$stub" ]]; then
        log_info "$stub already exists, leaving it alone"
        return 0
    fi

    if $DRY_RUN; then
        log_info "DRY-RUN: would write $stub stub"
        return 0
    fi

    cat >"$stub" <<'STUB'
# HomeRoute Studio workspace

10 apps live under `/opt/homeroute/apps/{slug}/`. Use the `app.*` MCP tools to
manage them (start/stop/logs/exec). See each app's own `CLAUDE.md` for
project-specific notes.

This stub is auto-generated by `scripts/setup-studio.sh` so the studio
workspace is not empty on first open. The full version is generated by
`hr-apps::context` once the apps registry is populated.
STUB
    log_ok "wrote stub $stub"
}

# =============================================================================
# Main
# =============================================================================
main() {
    log_info "setup-studio.sh starting (dry-run=$DRY_RUN)"
    preflight
    install_code_server
    write_unit
    enable_and_start
    register_route
    write_stub_claude_md

    local cs_version="(dry-run)"
    if ! $DRY_RUN && command -v code-server >/dev/null 2>&1; then
        cs_version="$(code-server --version 2>/dev/null | head -n1 || echo unknown)"
    fi

    log_ok "setup-studio.sh complete"
    printf '\n=== Summary ===\n'
    printf '  code-server version : %s\n' "$cs_version"
    printf '  unit file           : %s\n' "$SERVICE_FILE"
    printf '  bind address        : %s\n' "$STUDIO_BIND"
    printf '  public URL          : https://%s/\n' "$STUDIO_DOMAIN"
    if $DRY_RUN; then
        printf '  (dry-run — no changes were applied)\n'
    fi
}

main "$@"
