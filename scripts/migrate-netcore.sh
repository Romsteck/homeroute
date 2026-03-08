#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# HomeRoute: Migration monolithe → hr-netcore + homeroute
#
# Ce script gère la transition complète avec rollback automatique.
# Exécuter DEPUIS le serveur de dev (10.0.0.10).
#
# Usage: ./scripts/migrate-netcore.sh
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

PROD="root@10.0.0.254"
PROD_DIR="/opt/homeroute"
API="http://10.0.0.254:4000"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok()   { echo -e "${GREEN}✓ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠ $1${NC}"; }
fail() { echo -e "${RED}⛔ $1${NC}"; }

# ── Pré-checks ──────────────────────────────────────────────────────

echo "═══ Phase 0: Pré-vérifications ═══"

# Vérifier qu'on est sur dev, pas sur prod
if systemctl is-active --quiet homeroute 2>/dev/null; then
    fail "homeroute tourne localement — ce script doit être lancé depuis le serveur de DEV"
    exit 1
fi
ok "Exécution depuis le serveur de dev"

# Vérifier fallback DNS sur dev
if ! grep -q "1.1.1.1" /etc/resolv.conf; then
    warn "Pas de fallback DNS (1.1.1.1) dans /etc/resolv.conf"
    echo "    Ajout automatique..."
    echo "nameserver 1.1.1.1" >> /etc/resolv.conf
    ok "Fallback DNS ajouté"
else
    ok "Fallback DNS présent (1.1.1.1)"
fi

# Vérifier que la prod est joignable
if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "$PROD" 'true' 2>/dev/null; then
    fail "Impossible de joindre la prod ($PROD)"
    exit 1
fi
ok "Prod joignable via SSH"

# Vérifier que homeroute tourne sur prod
if ! ssh "$PROD" 'systemctl is-active --quiet homeroute'; then
    fail "homeroute n'est pas actif sur la prod"
    exit 1
fi
ok "homeroute actif sur la prod"

# Vérifier que les binaires existent
for bin in homeroute hr-netcore; do
    if [ ! -f "crates/target/release/$bin" ]; then
        fail "Binaire manquant: crates/target/release/$bin — lance 'make all' d'abord"
        exit 1
    fi
done
ok "Binaires build présents"

# Vérifier que les fichiers systemd existent
for svc in systemd/hr-netcore.service systemd/homeroute.service; do
    if [ ! -f "$svc" ]; then
        fail "Fichier manquant: $svc"
        exit 1
    fi
done
ok "Fichiers systemd présents"

# ── Phase 1: Backup ─────────────────────────────────────────────────

echo ""
echo "═══ Phase 1: Backup sur la prod ═══"

ssh "$PROD" bash <<'REMOTE_BACKUP'
set -e
BACKUP_DIR="/opt/homeroute/data/backup-pre-netcore"
mkdir -p "$BACKUP_DIR"

# Sauvegarder le binaire monolithe actuel
cp /opt/homeroute/crates/target/release/homeroute "$BACKUP_DIR/homeroute.bak"

# Sauvegarder le service systemd actuel
cp /etc/systemd/system/homeroute.service "$BACKUP_DIR/homeroute.service.bak"

# Sauvegarder les leases DHCP
if [ -f /var/lib/server-dashboard/dhcp-leases ]; then
    cp /var/lib/server-dashboard/dhcp-leases "$BACKUP_DIR/dhcp-leases.bak"
fi

echo "Backup créé dans $BACKUP_DIR"
ls -la "$BACKUP_DIR/"
REMOTE_BACKUP
ok "Backup effectué sur la prod"

# ── Phase 2: Copie des fichiers ─────────────────────────────────────

echo ""
echo "═══ Phase 2: Copie des fichiers vers la prod ═══"

# Binaires
rsync -az --info=progress2 crates/target/release/homeroute "$PROD:$PROD_DIR/crates/target/release/homeroute"
ok "Binaire homeroute copié"

rsync -az --info=progress2 crates/target/release/hr-netcore "$PROD:$PROD_DIR/crates/target/release/hr-netcore"
ok "Binaire hr-netcore copié"

# Services systemd
scp -q systemd/hr-netcore.service "$PROD:/etc/systemd/system/hr-netcore.service"
scp -q systemd/homeroute.service "$PROD:/etc/systemd/system/homeroute.service"
ok "Fichiers systemd copiés"

# Reload systemd + enable hr-netcore
ssh "$PROD" 'systemctl daemon-reload && systemctl enable hr-netcore'
ok "systemd rechargé, hr-netcore enabled"

# ── Phase 3: Bascule ────────────────────────────────────────────────

echo ""
echo "═══ Phase 3: Bascule (downtime DNS ~2s) ═══"
echo "    stop homeroute → start hr-netcore → start homeroute"

ssh "$PROD" 'systemctl stop homeroute && systemctl start hr-netcore && systemctl start homeroute'
ok "Services démarrés"

# Attendre stabilisation
sleep 3

# ── Phase 4: Vérifications ──────────────────────────────────────────

echo ""
echo "═══ Phase 4: Vérifications ═══"

CHECKS_OK=true

# Check 1: hr-netcore actif
if ssh "$PROD" 'systemctl is-active --quiet hr-netcore'; then
    ok "hr-netcore actif"
else
    fail "hr-netcore n'est PAS actif"
    CHECKS_OK=false
fi

# Check 2: homeroute actif
if ssh "$PROD" 'systemctl is-active --quiet homeroute'; then
    ok "homeroute actif"
else
    fail "homeroute n'est PAS actif"
    CHECKS_OK=false
fi

# Check 3: DNS résolution
if dig +short +timeout=3 @10.0.0.254 google.com | grep -q '^[0-9]'; then
    ok "DNS résolution OK (google.com)"
else
    fail "DNS résolution ÉCHOUÉE"
    CHECKS_OK=false
fi

# Check 4: API health
if curl -sf --connect-timeout 5 "$API/api/health" > /dev/null 2>&1; then
    ok "API health OK"
else
    fail "API health ÉCHOUÉE"
    CHECKS_OK=false
fi

# Check 5: Services status
SERVICES_OUTPUT=$(curl -sf --connect-timeout 5 "$API/api/services/status" 2>/dev/null || echo "FAIL")
if echo "$SERVICES_OUTPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['success']" 2>/dev/null; then
    ok "Services status API OK"
    echo "$SERVICES_OUTPUT" | python3 -m json.tool 2>/dev/null | head -30
else
    fail "Services status API ÉCHOUÉE"
    CHECKS_OK=false
fi

# Check 6: IPC socket existe
if ssh "$PROD" 'test -S /run/hr-netcore.sock'; then
    ok "IPC socket /run/hr-netcore.sock existe"
else
    fail "IPC socket manquant"
    CHECKS_OK=false
fi

# ── Phase 5: Rollback automatique si échec ──────────────────────────

if [ "$CHECKS_OK" = false ]; then
    echo ""
    echo "═══ ROLLBACK AUTOMATIQUE ═══"
    fail "Des vérifications ont échoué — rollback vers le monolithe"

    ssh "$PROD" bash <<'REMOTE_ROLLBACK'
set -e
BACKUP_DIR="/opt/homeroute/data/backup-pre-netcore"

# Arrêter les nouveaux services
systemctl stop homeroute 2>/dev/null || true
systemctl stop hr-netcore 2>/dev/null || true
systemctl disable hr-netcore 2>/dev/null || true

# Restaurer l'ancien binaire
cp "$BACKUP_DIR/homeroute.bak" /opt/homeroute/crates/target/release/homeroute

# Restaurer l'ancien service
cp "$BACKUP_DIR/homeroute.service.bak" /etc/systemd/system/homeroute.service
rm -f /etc/systemd/system/hr-netcore.service

systemctl daemon-reload
systemctl start homeroute

echo "Rollback terminé — monolithe restauré"
REMOTE_ROLLBACK

    # Vérifier le rollback
    sleep 2
    if dig +short +timeout=3 @10.0.0.254 google.com | grep -q '^[0-9]'; then
        ok "Rollback réussi — DNS fonctionne à nouveau"
    else
        fail "ROLLBACK ÉCHOUÉ — intervention manuelle requise !"
        echo "    ssh $PROD 'journalctl -u homeroute -n 50'"
    fi

    exit 1
fi

# ── Succès ───────────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════"
echo -e "${GREEN}Migration réussie !${NC}"
echo ""
echo "  hr-netcore : DNS/DHCP/Adblock/IPv6 (ne redémarre quasi jamais)"
echo "  homeroute  : API/Proxy/Auth/ACME/... (redémarre à chaque deploy)"
echo ""
echo "  Les prochains 'make deploy-prod' ne couperont PLUS le DNS."
echo "═══════════════════════════════════════════"
