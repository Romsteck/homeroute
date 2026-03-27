#!/bin/bash
###############################################################################
# migrate-apps-to-env.sh — Migrate existing apps into an environment container
#
# Copies app source code and databases into an env container, creates systemd
# services for each app, and updates the env-agent config.
#
# IMPORTANT: This is a COPY operation — originals are never deleted.
#
# Usage:
#   ./migrate-apps-to-env.sh <env_slug> <app1> [app2] [app3] ...
#
# Arguments:
#   env_slug    Environment slug: dev, prod, acc
#   app1..N     App slugs to migrate (e.g. trader, wallet, home, files)
#
# Options:
#   --source-dir DIR    Source apps directory (default: /ssd_pool/apps)
#   --db-dir DIR        Source DB directory (default: /opt/homeroute/data/db)
#   --dry-run           Print what would be done without executing
#   --help              Show this help
#
# Examples:
#   ./migrate-apps-to-env.sh dev trader wallet home files
#   ./migrate-apps-to-env.sh prod trader wallet --dry-run
#   ./migrate-apps-to-env.sh dev myfrigo --source-dir /ssd_pool/apps
#
# Prerequisites:
#   - Container env-{slug} must exist and be running
#   - App source directories must exist in --source-dir
#   - rsync must be installed
#
###############################################################################
set -euo pipefail

# =============================================================================
# Defaults
# =============================================================================
STORAGE_PATH="/var/lib/machines"
SOURCE_DIR="/ssd_pool/apps"
DB_SOURCE_DIR="/opt/homeroute/data/db"
DRY_RUN=false
APP_PORT_BASE=3000

# =============================================================================
# Argument parsing
# =============================================================================
usage() {
    sed -n '2,/^###/p' "$0" | head -n -1 | sed 's/^# \?//'
    exit 0
}

if [[ $# -lt 2 ]]; then
    echo "Error: missing required arguments."
    echo "Usage: $0 <env_slug> <app1> [app2] ..."
    echo "Run $0 --help for details."
    exit 1
fi

SLUG="$1"; shift
APPS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-dir)   SOURCE_DIR="$2"; shift 2 ;;
        --db-dir)       DB_SOURCE_DIR="$2"; shift 2 ;;
        --dry-run)      DRY_RUN=true; shift ;;
        --help|-h)      usage ;;
        --*)            echo "Unknown option: $1"; exit 1 ;;
        *)              APPS+=("$1"); shift ;;
    esac
done

if [[ ${#APPS[@]} -eq 0 ]]; then
    echo "Error: no apps specified."
    exit 1
fi

CONTAINER_NAME="env-${SLUG}"
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

# =============================================================================
# Prerequisite checks
# =============================================================================
check_prerequisites() {
    log "Checking prerequisites..."

    if [[ ! -d "$ROOTFS" ]]; then
        die "Container rootfs not found at $ROOTFS. Run provision-env.sh first."
    fi

    if ! machinectl show "$CONTAINER_NAME" &>/dev/null; then
        warn "Container $CONTAINER_NAME may not be running. Proceeding with direct rootfs copy."
    fi

    if ! command -v rsync &>/dev/null; then
        die "rsync is not installed. Run: apt install rsync"
    fi

    # Check each app source exists
    for app in "${APPS[@]}"; do
        if [[ ! -d "${SOURCE_DIR}/${app}" ]]; then
            die "App source not found: ${SOURCE_DIR}/${app}"
        fi
    done

    log "Prerequisites OK. Migrating ${#APPS[@]} app(s) to ${CONTAINER_NAME}."
}

# =============================================================================
# Migrate a single app
# =============================================================================
migrate_app() {
    local app="$1"
    local port="$2"
    local app_src="${SOURCE_DIR}/${app}"
    local app_dest="${ROOTFS}/apps/${app}"
    local db_src="${DB_SOURCE_DIR}/${app}.db"
    local db_dest="${ROOTFS}/opt/env-agent/data/db/${app}.db"

    log ""
    log "--- Migrating app: ${app} (port ${port}) ---"

    # Step 1: Copy source code
    log "  Copying source code: ${app_src}/ -> ${app_dest}/"
    if $DRY_RUN; then
        echo "[DRY-RUN] rsync -a --info=progress2 ${app_src}/ ${app_dest}/"
    else
        mkdir -p "$app_dest"
        rsync -a --info=progress2 \
            --exclude='target/' \
            --exclude='node_modules/' \
            --exclude='.next/' \
            --exclude='dist/' \
            --exclude='.git/' \
            "${app_src}/" "${app_dest}/"
        log "  Source code copied (excludes: target/, node_modules/, .next/, dist/, .git/)"
    fi

    # Step 2: Copy database (if exists)
    if [[ -f "$db_src" ]]; then
        log "  Copying database: ${db_src} -> ${db_dest}"
        if $DRY_RUN; then
            echo "[DRY-RUN] cp ${db_src} ${db_dest}"
        else
            mkdir -p "$(dirname "$db_dest")"
            cp "$db_src" "$db_dest"
            # Also copy WAL and SHM if they exist (SQLite)
            [[ -f "${db_src}-wal" ]] && cp "${db_src}-wal" "${db_dest}-wal"
            [[ -f "${db_src}-shm" ]] && cp "${db_src}-shm" "${db_dest}-shm"
            log "  Database copied"
        fi
    else
        log "  No database found at ${db_src} (skipping)"
    fi

    # Step 3: Create systemd service for the app
    log "  Creating systemd service: ${app}.service"
    local service_content
    service_content=$(cat <<SVCEOF
[Unit]
Description=HomeRoute App: ${app}
After=network-online.target env-agent.service
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/apps/${app}
ExecStart=/apps/${app}/${app}
Restart=always
RestartSec=5
Environment=PORT=${port}
Environment=RUST_LOG=info
Environment=DATABASE_URL=/opt/env-agent/data/db/${app}.db

[Install]
WantedBy=multi-user.target
SVCEOF
    )

    if $DRY_RUN; then
        echo "[DRY-RUN] Would write ${ROOTFS}/etc/systemd/system/${app}.service"
    else
        echo "$service_content" > "${ROOTFS}/etc/systemd/system/${app}.service"
        log "  Service file created"
    fi
}

# =============================================================================
# Update env-agent.toml with app list
# =============================================================================
update_env_agent_config() {
    log ""
    log "--- Updating env-agent.toml with app list ---"

    local config_file="${ROOTFS}/etc/env-agent.toml"

    if [[ ! -f "$config_file" ]] && ! $DRY_RUN; then
        warn "env-agent.toml not found at ${config_file}. Skipping config update."
        return
    fi

    # Build the apps TOML array
    local apps_toml=""
    local port=$APP_PORT_BASE
    for app in "${APPS[@]}"; do
        apps_toml+="
[[apps]]
slug = \"${app}\"
port = ${port}
service = \"${app}.service\"
db = \"/opt/env-agent/data/db/${app}.db\"
"
        port=$((port + 1))
    done

    if $DRY_RUN; then
        echo "[DRY-RUN] Would append to ${config_file}:"
        echo "$apps_toml"
    else
        # Remove any existing [[apps]] sections before appending
        # (simple approach: check if [[apps]] exists and warn)
        if grep -q '^\[\[apps\]\]' "$config_file" 2>/dev/null; then
            warn "Existing [[apps]] sections found in env-agent.toml."
            warn "New apps will be appended. You may want to review for duplicates."
        fi

        echo "$apps_toml" >> "$config_file"
        log "env-agent.toml updated with ${#APPS[@]} app(s)"
    fi
}

# =============================================================================
# Summary
# =============================================================================
print_summary() {
    log ""
    log "============================================="
    log " App migration complete!"
    log "============================================="
    log ""
    log "  Environment:  ${CONTAINER_NAME}"
    log "  Apps migrated: ${APPS[*]}"
    log ""

    local port=$APP_PORT_BASE
    for app in "${APPS[@]}"; do
        log "    ${app}  ->  /apps/${app}/  (port ${port})"
        port=$((port + 1))
    done

    log ""
    log "Next steps:"
    log "  1. Build each app inside the container (if needed)"
    log "  2. Restart the env-agent to pick up the new config"
    log "  3. Start app services: machinectl shell ${CONTAINER_NAME} /bin/systemctl start <app>"
    log "  4. Verify health: curl http://<container_ip>:<port>/api/health"
    log ""
    log "NOTE: Original files were NOT deleted (copy-only operation)."
    log ""
}

# =============================================================================
# Main
# =============================================================================
main() {
    log "Migrating apps to environment: ${CONTAINER_NAME}"
    log "Apps: ${APPS[*]}"
    log "Source: ${SOURCE_DIR} | DB source: ${DB_SOURCE_DIR}"
    log ""

    check_prerequisites

    local port=$APP_PORT_BASE
    for app in "${APPS[@]}"; do
        migrate_app "$app" "$port"
        port=$((port + 1))
    done

    update_env_agent_config
    print_summary
}

main
