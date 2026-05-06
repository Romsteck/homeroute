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
#   - Runs as a dedicated **`hr-studio`** system user (not root). This isolates
#     the Studio's `~/.claude/` (memory, settings, MCP, auto-approve) from any
#     other service or admin session on the host. `/opt/homeroute/apps/` is
#     group-owned by `hr-studio` with setgid so both the Studio and `romain`
#     (added to the group) can edit each other's files seamlessly.
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
STUDIO_USER="${STUDIO_USER:-hr-studio}"
STUDIO_GROUP="${STUDIO_GROUP:-hr-studio}"
STUDIO_HOME="${STUDIO_HOME:-/var/lib/hr-studio}"
EXTRA_GROUP_MEMBERS="${EXTRA_GROUP_MEMBERS:-romain}"
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

    ensure_user_and_group

    if [[ ! -d "$APPS_DIR" ]]; then
        log_info "creating $APPS_DIR ($STUDIO_USER:$STUDIO_GROUP 2775)"
        run "mkdir -p '$APPS_DIR'"
        run "chown '$STUDIO_USER:$STUDIO_GROUP' '$APPS_DIR'"
        run "chmod 2775 '$APPS_DIR'"
    else
        log_ok "$APPS_DIR already exists"
    fi

    apply_apps_permissions

    if [[ ! -d "$DATA_DIR" ]]; then
        log_info "creating $DATA_DIR (code-server user-data-dir, owned by $STUDIO_USER)"
        run "mkdir -p '$DATA_DIR'"
        run "chown -R '$STUDIO_USER:$STUDIO_GROUP' '$DATA_DIR'"
    else
        local current_owner
        current_owner="$(stat -c '%U:%G' "$DATA_DIR" 2>/dev/null || echo unknown)"
        if [[ "$current_owner" != "${STUDIO_USER}:${STUDIO_GROUP}" ]]; then
            log_info "rechowning $DATA_DIR ($current_owner -> $STUDIO_USER:$STUDIO_GROUP)"
            run "chown -R '$STUDIO_USER:$STUDIO_GROUP' '$DATA_DIR'"
        else
            log_ok "$DATA_DIR already owned by $STUDIO_USER:$STUDIO_GROUP"
        fi
    fi

    log_ok "preflight passed"
}

# =============================================================================
# Ensure the dedicated system user/group exists
# =============================================================================
ensure_user_and_group() {
    if getent group "$STUDIO_GROUP" >/dev/null 2>&1; then
        log_ok "group $STUDIO_GROUP already exists"
    else
        log_info "creating system group $STUDIO_GROUP"
        run "groupadd --system '$STUDIO_GROUP'"
    fi

    if id "$STUDIO_USER" >/dev/null 2>&1; then
        log_ok "user $STUDIO_USER already exists"
    else
        log_info "creating system user $STUDIO_USER (home=$STUDIO_HOME, shell=/bin/bash)"
        run "useradd --system --gid '$STUDIO_GROUP' --create-home \
              --home-dir '$STUDIO_HOME' --shell /bin/bash \
              --comment 'HomeRoute Studio service' '$STUDIO_USER'"
    fi

    if [[ -n "$EXTRA_GROUP_MEMBERS" ]]; then
        local member
        for member in $EXTRA_GROUP_MEMBERS; do
            if ! id "$member" >/dev/null 2>&1; then
                log_warn "extra group member '$member' does not exist — skipping"
                continue
            fi
            if id -nG "$member" | tr ' ' '\n' | grep -qx "$STUDIO_GROUP"; then
                log_ok "$member already in group $STUDIO_GROUP"
            else
                log_info "adding $member to group $STUDIO_GROUP"
                run "usermod -aG '$STUDIO_GROUP' '$member'"
            fi
        done
    fi
}

# =============================================================================
# Permissions on /opt/homeroute/apps/ — group-owned by hr-studio with setgid +
# default POSIX ACLs so files created by the `romain` user (umask 0022) still
# end up group-writable for the Studio agent.
# =============================================================================
apply_apps_permissions() {
    log_info "ensuring $APPS_DIR is group=$STUDIO_GROUP, g+rwX, setgid on dirs"
    run "chgrp -R '$STUDIO_GROUP' '$APPS_DIR'"
    run "chmod -R g+rwX '$APPS_DIR'"
    run "find '$APPS_DIR' -type d -exec chmod g+s {} +"

    # Default ACLs: kernel-applied, immune to the creating process's umask.
    # The hr-studio.service has UMask=0002 (so its files are g+w by default),
    # but plain `romain` shells default to 0022 — without this default ACL,
    # files romain creates land as 644 + romain:romain group, and the
    # hr-studio user can't unlink/edit them. The default ACL on each dir
    # forces every newly-created child to inherit g:hr-studio:rwX.
    if command -v setfacl >/dev/null 2>&1; then
        log_info "applying POSIX default ACLs (g:$STUDIO_GROUP:rwX) on $APPS_DIR"
        run "setfacl -R -m g:$STUDIO_GROUP:rwX '$APPS_DIR'"
        run "setfacl -R -d -m g:$STUDIO_GROUP:rwX '$APPS_DIR'"
    else
        log_info "setfacl not installed — skipping default ACLs (acl package recommended)"
    fi

    log_ok "$APPS_DIR permissions applied (group=$STUDIO_GROUP, setgid on dirs, default ACL on subtree)"
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
User=${STUDIO_USER}
Group=${STUDIO_GROUP}
UMask=0002
WorkingDirectory=${APPS_DIR}
Environment=HOME=${STUDIO_HOME}
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
