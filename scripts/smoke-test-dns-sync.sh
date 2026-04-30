#!/usr/bin/env bash
# Smoke test du DnsRouteSync : vérifie que toutes les routes connues du proxy
# résolvent vers EDGE_SERVER_IP, et qu'un domaine inconnu retourne NXDOMAIN.
#
# Usage : ssh romain@10.0.0.20 'bash -s' < scripts/smoke-test-dns-sync.sh
#   ou en local sur Medion : bash scripts/smoke-test-dns-sync.sh

set -uo pipefail

DNS_HOST="${DNS_HOST:-127.0.0.1}"
EXPECTED_IP="${EXPECTED_IP:-10.0.0.254}"
BASE_DOMAIN="${BASE_DOMAIN:-mynetwk.biz}"

pass=0
fail=0

check_resolves() {
    local fqdn="$1"
    local got
    got="$(dig +short +time=2 +tries=1 "$fqdn" @"$DNS_HOST" | head -1)"
    if [[ "$got" == "$EXPECTED_IP" ]]; then
        printf "  ✓ %-45s %s\n" "$fqdn" "$got"
        pass=$((pass + 1))
    else
        printf "  ✗ %-45s expected=%s got=%q\n" "$fqdn" "$EXPECTED_IP" "$got"
        fail=$((fail + 1))
    fi
}

check_nxdomain() {
    local fqdn="$1"
    local status
    status="$(dig +time=2 +tries=1 "$fqdn" @"$DNS_HOST" +noall +comments 2>/dev/null | grep -oE 'status: [A-Z]+' | awk '{print $2}')"
    if [[ "$status" == "NXDOMAIN" ]]; then
        printf "  ✓ %-45s NXDOMAIN\n" "$fqdn"
        pass=$((pass + 1))
    else
        printf "  ✗ %-45s expected=NXDOMAIN got=%q\n" "$fqdn" "$status"
        fail=$((fail + 1))
    fi
}

echo "=== Smoke test DnsRouteSync (DNS @${DNS_HOST}) ==="

echo "Builtins :"
check_resolves "proxy.${BASE_DOMAIN}"
check_resolves "auth.${BASE_DOMAIN}"

echo "Routes manuelles enabled :"
if [[ -r /var/lib/server-dashboard/rust-proxy-config.json ]]; then
    domains=$(jq -r '.routes[] | select(.enabled) | .domain' \
        /var/lib/server-dashboard/rust-proxy-config.json)
    for d in $domains; do check_resolves "$d"; done
else
    echo "  (rust-proxy-config.json non lisible — skip)"
fi

echo "Apps :"
if [[ -r /opt/homeroute/data/app-routes.json ]]; then
    apps=$(jq -r 'keys[]' /opt/homeroute/data/app-routes.json)
    for d in $apps; do check_resolves "$d"; done
else
    echo "  (app-routes.json non lisible — skip)"
fi

echo "Domaines absents (doivent NXDOMAIN) :"
check_nxdomain "_smoke-$$-$(date +%s).${BASE_DOMAIN}"
check_nxdomain "${BASE_DOMAIN}"

echo "Garde anti-leak (search domain noise) :"
check_nxdomain "api.stripe.com.${BASE_DOMAIN}"

echo
echo "=== Résultat : ${pass} OK / ${fail} KO ==="
exit "$fail"
