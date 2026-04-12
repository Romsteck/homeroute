#!/bin/bash
###############################################################################
# migrate-routes.sh — Reconfigure edge proxy + DNS for the dismantled-envs world
#
# After the env teardown, every app lives directly on the host (10.0.0.254) and
# is reachable as `{slug}.mynetwk.biz` (instead of `{slug}.{env}.mynetwk.biz`).
# This script:
#   1. Reads /opt/homeroute/data/apps.json (registry built by hr-apps).
#   2. Upserts an edge route for each app pointing to 127.0.0.1:{port}.
#   3. Adds a route for studio.mynetwk.biz → 127.0.0.1:8443 (auth required).
#   4. Removes the legacy `{slug}.{env}.mynetwk.biz`, `make.mynetwk.biz` and
#      `studio.{env}.mynetwk.biz` routes.
#   5. Drops legacy DNS records pointing to the env containers
#      (10.0.0.200/201/202).
#   6. Adds a `{slug}.mynetwk.biz` A record per app (or skips if a wildcard
#      `*.mynetwk.biz` already exists).
#   7. Reloads hr-edge and hr-netcore.
#
# The script is **idempotent** — rerunning it is safe.
#
# Usage:
#   sudo ./migrate-routes.sh           # apply
#   ./migrate-routes.sh --dry-run      # print actions only
#   ./migrate-routes.sh --help
#
# Must run on the prod router (10.0.0.254) — hr-edge / hr-netcore must be up.
###############################################################################
set -euo pipefail

# =============================================================================
# Defaults
# =============================================================================
APPS_JSON="${APPS_JSON:-/opt/homeroute/data/apps.json}"
HOSTS_JSON="${HOSTS_JSON:-/data/hosts.json}"
HOMEROUTE_API="${HOMEROUTE_API:-http://127.0.0.1:4000}"
STUDIO_DOMAIN="${STUDIO_DOMAIN:-studio.mynetwk.biz}"
STUDIO_PORT="${STUDIO_PORT:-8443}"
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
# Counters (for the final summary)
# =============================================================================
ROUTES_ADDED=0
ROUTES_REMOVED=0
DNS_ADDED=0
DNS_REMOVED=0

# =============================================================================
# Preflight
# =============================================================================
preflight() {
    log_info "Preflight checks…"

    if ! $DRY_RUN && [[ "$(id -u)" -ne 0 ]]; then
        die "must run as root (or use --dry-run)"
    fi

    for bin in curl jq; do
        command -v "$bin" >/dev/null 2>&1 || die "missing required binary: $bin"
    done

    if ! systemctl is-active --quiet hr-edge; then
        die "hr-edge.service is not active — refusing to mutate routes"
    fi
    if ! systemctl is-active --quiet hr-netcore; then
        die "hr-netcore.service is not active — refusing to mutate DNS"
    fi

    [[ -f "$APPS_JSON" ]] || die "apps registry not found: $APPS_JSON"

    if ! jq -e 'type == "array" or has("apps")' "$APPS_JSON" >/dev/null 2>&1; then
        die "$APPS_JSON is not valid JSON (expected array or {apps:[…]})"
    fi

    log_ok "preflight passed"
}

# =============================================================================
# Detect homeroute IP for DNS A records
# =============================================================================
detect_homeroute_ip() {
    local ip=""
    if [[ -f "$HOSTS_JSON" ]]; then
        ip="$(jq -r '.. | objects | select(.role? == "router") | .ip? // empty' "$HOSTS_JSON" 2>/dev/null | head -n1 || true)"
    fi
    if [[ -z "$ip" ]]; then
        ip="$(ip -4 -o addr show scope global 2>/dev/null \
              | awk '{print $4}' | cut -d/ -f1 \
              | grep -E '^10\.0\.0\.' | head -n1 || true)"
    fi
    [[ -z "$ip" ]] && ip="10.0.0.254"
    echo "$ip"
}

# =============================================================================
# Apps loader — emits TSV: slug<TAB>port<TAB>domain<TAB>visibility
# =============================================================================
load_apps() {
    jq -r '
        (if type == "array" then . else .apps end)
        | .[]
        | [
            (.slug // .id // ""),
            ((.port // 0) | tostring),
            (.domain // ((.slug // .id) + ".mynetwk.biz")),
            (.visibility // "public")
          ]
        | @tsv
    ' "$APPS_JSON"
}

# =============================================================================
# Edge route helpers
#
# NOTE: the exact route endpoint shape is being created by another agent in
# wave 3 of the dismantle-envs effort. Best guess as of 2026-04-11 :
#   GET    /api/edge/routes              -> { "routes": [{ "domain": "...", … }] }
#   POST   /api/edge/routes              -> { "domain": "...", "target": "...", "auth_required": bool }
#   DELETE /api/edge/routes/{domain}
# If the endpoint returns 404, we log a warning and continue so the operator
# can rerun this script after the API is in place.
# TODO(wave3): confirm endpoint shape with hr-api maintainer.
# =============================================================================
list_routes() {
    local resp http
    resp="$(curl -sS -o /tmp/migrate-routes.list.$$ -w '%{http_code}' \
            "$HOMEROUTE_API/api/edge/routes" || echo "000")"
    http="$resp"
    if [[ "$http" != "200" ]]; then
        log_warn "GET /api/edge/routes returned HTTP $http — assuming empty list"
        rm -f "/tmp/migrate-routes.list.$$"
        return 0
    fi
    jq -r '(.routes // []) | .[].domain' "/tmp/migrate-routes.list.$$" 2>/dev/null || true
    rm -f "/tmp/migrate-routes.list.$$"
}

upsert_route() {
    local domain="$1" target="$2" auth="$3"
    local payload http
    payload="$(jq -nc \
        --arg d "$domain" --arg t "$target" --argjson a "$auth" \
        '{domain: $d, target: $t, auth_required: $a}')"

    if $DRY_RUN; then
        log_info "DRY-RUN: POST /api/edge/routes $payload"
        ROUTES_ADDED=$((ROUTES_ADDED + 1))
        return 0
    fi

    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            -X POST "$HOMEROUTE_API/api/edge/routes" \
            -H 'Content-Type: application/json' \
            -d "$payload" || echo "000")"

    case "$http" in
        200|201|204)
            log_ok "route upserted: $domain → $target (auth=$auth)"
            ROUTES_ADDED=$((ROUTES_ADDED + 1))
            ;;
        404)
            log_warn "POST /api/edge/routes returned 404 — endpoint not yet wired (wave 3)"
            ;;
        *)
            log_warn "POST /api/edge/routes for $domain failed: HTTP $http"
            ;;
    esac
}

delete_route() {
    local domain="$1" http
    if $DRY_RUN; then
        log_info "DRY-RUN: DELETE /api/edge/routes/$domain"
        ROUTES_REMOVED=$((ROUTES_REMOVED + 1))
        return 0
    fi
    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            -X DELETE "$HOMEROUTE_API/api/edge/routes/$domain" || echo "000")"
    case "$http" in
        200|204)
            log_ok "legacy route removed: $domain"
            ROUTES_REMOVED=$((ROUTES_REMOVED + 1))
            ;;
        404)
            log_info "route already gone: $domain"
            ;;
        *)
            log_warn "DELETE $domain failed: HTTP $http"
            ;;
    esac
}

# =============================================================================
# DNS helpers
# TODO(wave3): confirm exact DNS endpoint shape — assumed to be:
#   GET    /api/dns/static       -> { "records": [{ "name": "...", "value": "...", "type": "A" }] }
#   POST   /api/dns/static       -> { "name": "...", "value": "...", "type": "A" }
#   DELETE /api/dns/static/{name}
# =============================================================================
dns_records_json() {
    local resp http
    resp="$(curl -sS -o /tmp/migrate-routes.dns.$$ -w '%{http_code}' \
            "$HOMEROUTE_API/api/dns/static" || echo "000")"
    http="$resp"
    if [[ "$http" != "200" ]]; then
        log_warn "GET /api/dns/static returned HTTP $http — assuming empty list"
        rm -f "/tmp/migrate-routes.dns.$$"
        echo '{"records":[]}'
        return 0
    fi
    cat "/tmp/migrate-routes.dns.$$"
    rm -f "/tmp/migrate-routes.dns.$$"
}

dns_has_wildcard() {
    local json="$1"
    jq -e '(.records // []) | map(.name) | any(. == "*.mynetwk.biz")' \
        <<<"$json" >/dev/null 2>&1
}

dns_record_exists() {
    local json="$1" name="$2"
    jq -e --arg n "$name" \
        '(.records // []) | any(.name == $n)' <<<"$json" >/dev/null 2>&1
}

dns_add_record() {
    local name="$1" value="$2" payload http
    payload="$(jq -nc --arg n "$name" --arg v "$value" \
        '{name: $n, value: $v, type: "A"}')"
    if $DRY_RUN; then
        log_info "DRY-RUN: POST /api/dns/static $payload"
        DNS_ADDED=$((DNS_ADDED + 1))
        return 0
    fi
    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            -X POST "$HOMEROUTE_API/api/dns/static" \
            -H 'Content-Type: application/json' \
            -d "$payload" || echo "000")"
    case "$http" in
        200|201|204)
            log_ok "DNS record added: $name → $value"
            DNS_ADDED=$((DNS_ADDED + 1))
            ;;
        404)
            log_warn "POST /api/dns/static returned 404 — endpoint not yet wired"
            ;;
        *)
            log_warn "POST /api/dns/static for $name failed: HTTP $http"
            ;;
    esac
}

dns_delete_record() {
    local name="$1" http
    if $DRY_RUN; then
        log_info "DRY-RUN: DELETE /api/dns/static/$name"
        DNS_REMOVED=$((DNS_REMOVED + 1))
        return 0
    fi
    http="$(curl -sS -o /dev/null -w '%{http_code}' \
            -X DELETE "$HOMEROUTE_API/api/dns/static/$name" || echo "000")"
    case "$http" in
        200|204)
            log_ok "legacy DNS record removed: $name"
            DNS_REMOVED=$((DNS_REMOVED + 1))
            ;;
        404)
            log_info "DNS record already gone: $name"
            ;;
        *)
            log_warn "DELETE DNS $name failed: HTTP $http"
            ;;
    esac
}

# =============================================================================
# Main
# =============================================================================
main() {
    log_info "migrate-routes.sh starting (dry-run=$DRY_RUN)"
    preflight

    local homeroute_ip
    homeroute_ip="$(detect_homeroute_ip)"
    log_info "homeroute IP detected: $homeroute_ip"

    # -------- 1. Upsert per-app routes --------
    log_info "Loading apps from $APPS_JSON"
    local apps_tsv
    apps_tsv="$(load_apps)"
    if [[ -z "$apps_tsv" ]]; then
        log_warn "no apps found in $APPS_JSON — nothing to upsert"
    fi

    while IFS=$'\t' read -r slug port domain visibility; do
        [[ -z "$slug" ]] && continue
        if [[ -z "$port" || "$port" == "0" ]]; then
            log_warn "app '$slug' has no port — skipping"
            continue
        fi
        local auth="false"
        [[ "$visibility" == "private" ]] && auth="true"
        upsert_route "$domain" "127.0.0.1:$port" "$auth"
    done <<<"$apps_tsv"

    # -------- 2. Studio route --------
    upsert_route "$STUDIO_DOMAIN" "127.0.0.1:$STUDIO_PORT" "true"

    # -------- 3. Remove legacy routes --------
    log_info "Listing existing routes for cleanup"
    local existing_routes
    existing_routes="$(list_routes)"

    while IFS= read -r domain; do
        [[ -z "$domain" ]] && continue
        if [[ "$domain" =~ \.(dev|acc|prod)\.mynetwk\.biz$ ]] \
            || [[ "$domain" == "make.mynetwk.biz" ]] \
            || [[ "$domain" =~ ^studio\.(dev|acc|prod)\.mynetwk\.biz$ ]]; then
            delete_route "$domain"
        fi
    done <<<"$existing_routes"

    # -------- 4. DNS cleanup + adds --------
    log_info "Inspecting DNS static records"
    local dns_json
    dns_json="$(dns_records_json)"

    # Drop records pointing to the legacy env container IPs.
    while IFS= read -r name; do
        [[ -z "$name" ]] && continue
        dns_delete_record "$name"
    done < <(jq -r '
        (.records // [])
        | .[]
        | select(.value == "10.0.0.200" or .value == "10.0.0.201" or .value == "10.0.0.202")
        | .name
    ' <<<"$dns_json")

    if dns_has_wildcard "$dns_json"; then
        log_info "wildcard DNS *.mynetwk.biz in place, skipping individual records"
    else
        # Refresh after deletes so we don't re-add what we just removed by name.
        dns_json="$(dns_records_json)"
        while IFS=$'\t' read -r slug port domain visibility; do
            [[ -z "$slug" ]] && continue
            local name="${slug}.mynetwk.biz"
            if dns_record_exists "$dns_json" "$name"; then
                log_info "DNS already has $name, skipping"
            else
                dns_add_record "$name" "$homeroute_ip"
            fi
        done <<<"$apps_tsv"

        if ! dns_record_exists "$dns_json" "$STUDIO_DOMAIN"; then
            dns_add_record "$STUDIO_DOMAIN" "$homeroute_ip"
        fi
    fi

    # -------- 5. Reload services --------
    log_info "Reloading hr-edge and hr-netcore"
    run "systemctl reload hr-edge"
    run "systemctl reload hr-netcore"

    # -------- 6. Summary --------
    log_ok "migrate-routes.sh complete"
    printf '\n=== Summary ===\n'
    printf '  routes added/upserted : %d\n' "$ROUTES_ADDED"
    printf '  routes removed        : %d\n' "$ROUTES_REMOVED"
    printf '  DNS records added     : %d\n' "$DNS_ADDED"
    printf '  DNS records removed   : %d\n' "$DNS_REMOVED"
    if $DRY_RUN; then
        printf '  (dry-run — no changes were applied)\n'
    fi
}

main "$@"
