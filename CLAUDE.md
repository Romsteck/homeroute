# Notes pour Claude Code

> ⚠️ **Le frontend Leptos (`crates/hr-web/`) est temporairement obsolète.**
> Ne pas modifier, maintenir ni supprimer ce code. Le frontend actif est l'application React/Vite dans `web/`.

## Architecture

HomeRoute est un **binaire Rust unifié** qui gère tous les services réseau.

- **Frontend**: Application React/Vite dans `web/` — servie comme fichiers statiques par le backend Rust
- **Frontend (obsolète)**: `crates/hr-web/` contient une interface Leptos SSR temporairement obsolète — ne pas maintenir
- **Backend**: Binaire Rust unique (Cargo workspace) dans `/opt/homeroute/crates/`
- **Service systemd**: `homeroute.service`

### Cargo Workspace

```
crates/
├── homeroute/       # Binaire principal (supervisor + main)
├── hr-common/       # Types partagés, config, EventBus
├── hr-auth/         # Auth (SQLite sessions, YAML users, Argon2id)
├── hr-proxy/        # Reverse proxy HTTPS (TLS/SNI, WebSocket, forward-auth)
├── hr-dns/          # Serveur DNS (UDP/TCP port 53, cache, upstream)
├── hr-dhcp/         # Serveur DHCP (DHCPv4, leases, DORA)
├── hr-ipv6/         # IPv6 RA + DHCPv6 stateless
├── hr-adblock/      # Moteur adblock (FxHashSet, sources, whitelist)
├── hr-acme/         # Let's Encrypt ACME (wildcards DNS-01 via Cloudflare)
├── hr-analytics/    # Capture trafic + agrégation (SQLite)
├── hr-servers/      # Gestion serveurs (monitoring, WoL, scheduler)
├── hr-system/       # Système (énergie, updates, réseau, DDNS Cloudflare)
├── hr-api/          # Routeur API HTTP (axum, routes /api/*, WebSocket)
├── hr-web/          # ⚠️ OBSOLÈTE — Interface Leptos SSR (ne pas maintenir)
```

## Gestion du serveur

- Le service systemd `homeroute.service` gère le binaire Rust
- `systemctl restart homeroute` pour redémarrer après modifications
- `systemctl reload homeroute` (SIGHUP) pour hot-reload de la config proxy
- Le build frontend (`cd /opt/homeroute/web && npm run build`) peut être lancé

## Stockage

| Données | Format | Chemin |
|---------|--------|--------|
| Sessions | SQLite | `/opt/homeroute/data/auth.db` |
| Users | YAML | `/opt/homeroute/data/users.yml` |
| Analytics | SQLite | `/opt/homeroute/data/analytics.db` |
| Serveurs | JSON | `/data/servers.json` |
| WoL schedules | JSON | `/data/wol-schedules.json` |
| Config proxy | JSON | `/var/lib/server-dashboard/rust-proxy-config.json` |
| Config DNS/DHCP | JSON | `/var/lib/server-dashboard/dns-dhcp-config.json` |
| Config reverseproxy | JSON | `/var/lib/server-dashboard/reverseproxy-config.json` |
| Certificats ACME | PEM | `/var/lib/server-dashboard/acme/` |
| DHCP leases | JSON | `/var/lib/server-dashboard/dhcp-leases` |
| Env config | dotenv | `/opt/homeroute/.env` |

## Ports

| Port | Service |
|------|---------|
| 443 | HTTPS reverse proxy (hr-proxy) |
| 80 | HTTP→HTTPS redirect |
| 53 | DNS (hr-dns, UDP+TCP) |
| 67 | DHCP (hr-dhcp) |
| 4000 | API management (hr-api, interne) |

## Cloudflare

⚠️ **JAMAIS désactiver le mode proxied Cloudflare** - il convertit IPv6 → IPv4 pour les clients externes.

Les enregistrements DNS sont synchronisés automatiquement:
- **Cloudflare**: AAAA → IPv6 agent (proxied)
- **DNS local**: A → IPv4 agent + AAAA → IPv6 agent (direct aux containers LXC)

## Commandes utiles

```bash
# Build tout (serveur + frontend Vite)
cd /opt/homeroute && make all

# Déployer (build + restart service)
cd /opt/homeroute && make deploy

# Build serveur uniquement
cd /opt/homeroute && make server

# Build frontend Vite uniquement
cd /opt/homeroute && make web

# Tests
cd /opt/homeroute && make test

# Redémarrer le service
systemctl restart homeroute

# Hot-reload config proxy
systemctl reload homeroute

# Logs
journalctl -u homeroute -f

# Vérifier le service
curl -s http://localhost:4000/api/health | jq
```

## Règles Frontend (OBLIGATOIRE)

- **JAMAIS** lancer le serveur manuellement (`cargo run`, etc.)
- **TOUJOURS** utiliser `systemctl` pour gérer le service
- **TOUJOURS** utiliser `make deploy` pour build + restart
- **NE PAS** modifier le code Leptos dans `crates/hr-web/` — il est temporairement obsolète
- Pour tester après modification : `make deploy && curl -s http://localhost:4000/api/health`

## Workflow de mise à jour des agents (OBLIGATOIRE)

Lors de la modification du binaire `hr-agent`, suivre **obligatoirement** ce workflow:

### 1. Build du nouvel agent

```bash
cd /opt/homeroute && cargo build --release -p hr-agent
```

### 2. Copie vers le répertoire de distribution

```bash
cp target/release/hr-agent /opt/homeroute/data/agent-binaries/hr-agent
```

### 3. Déclenchement de la mise à jour

```bash
curl -X POST http://localhost:4000/api/applications/agents/update
```

### 4. Vérification de l'état

```bash
curl http://localhost:4000/api/applications/agents/update/status | jq
```

Vérifier que tous les agents ont:
- `status: "connected"`
- `current_version` = version attendue
- `metrics_flowing: true`

### 5. Correction des agents défaillants

Si un agent ne se reconnecte pas après la mise à jour:

```bash
# Via API (recommandé):
curl -X POST http://localhost:4000/api/applications/{id}/update/fix

# Ou manuellement via LXC:
lxc exec hr-{slug} -- bash -c "curl -fsSL http://10.0.0.254:4000/api/applications/agents/binary -o /usr/local/bin/hr-agent && chmod +x /usr/local/bin/hr-agent && systemctl restart hr-agent"
```

### Checklist de vérification

Après déclenchement d'une mise à jour, vérifier:
- [ ] Tous les agents montrent `status: connected`
- [ ] Tous les agents reportent la `current_version` attendue
- [ ] `metrics_flowing: true` pour tous les agents
- [ ] Aucun agent en état `failed_reconnect`
