# Notes pour Claude Code

## Architecture

HomeRoute utilise **deux binaires** pour isoler les services réseau critiques :

- **`hr-netcore`** : DNS, DHCP, Adblock, IPv6 — ne redémarre quasi jamais
- **`homeroute`** : API, Proxy HTTPS, Auth, ACME, Containers, Cloud relay — redémarre à chaque deploy
- **Communication** : IPC Unix socket (`/run/hr-netcore.sock`, JSON-line)
- **Frontend** : React/Vite dans `web/` — fichiers statiques servis par Rust
- **Services** : `hr-netcore.service` + `homeroute.service` (systemd)

### Cargo Workspace

```
crates/
├── homeroute/       # Binaire API/Proxy (supervisor)
├── hr-netcore/      # Binaire réseau (DNS/DHCP/Adblock/IPv6)
├── hr-ipc/          # Protocole IPC Unix socket (partagé)
├── hr-common/       # Types partagés, config, EventBus
├── hr-auth/         # Auth (SQLite sessions, YAML users, Argon2id)
├── hr-proxy/        # Reverse proxy HTTPS (TLS/SNI, WebSocket)
├── hr-dns/          # DNS (UDP/TCP port 53, cache, upstream)
├── hr-dhcp/         # DHCP (DHCPv4, leases, DORA)
├── hr-ipv6/         # IPv6 RA + DHCPv6 stateless
├── hr-adblock/      # Adblock (FxHashSet, sources, whitelist)
├── hr-acme/         # ACME Let's Encrypt (DNS-01 Cloudflare)
├── hr-firewall/     # Firewall IPv6 (nftables)
├── hr-container/    # Containers systemd-nspawn
├── hr-registry/     # Registry applications/agents
├── hr-agent/        # Agent binaire dans les containers nspawn
├── hr-host-agent/   # Agent hôte
└── hr-api/          # API HTTP (axum, /api/*, WebSocket)
```

## Stockage

| Données | Chemin |
|---------|--------|
| Sessions SQLite | `/opt/homeroute/data/auth.db` |
| Users YAML | `/opt/homeroute/data/users.yml` |
| Hosts JSON | `/opt/homeroute/data/hosts.json` |
| Config proxy/DNS/DHCP | `/var/lib/server-dashboard/*.json` |
| Certificats ACME | `/var/lib/server-dashboard/acme/` |
| Env config | `/opt/homeroute/.env` |

## Ports

| Port | Service | Binaire |
|------|---------|---------|
| 443 | HTTPS reverse proxy (hr-proxy) | homeroute |
| 80 | HTTP→HTTPS redirect | homeroute |
| 4000 | API management (hr-api) | homeroute |
| 53 | DNS (hr-dns) | hr-netcore |
| 67 | DHCP (hr-dhcp) | hr-netcore |

## Cloudflare

⚠️ **JAMAIS désactiver le mode proxied** — convertit IPv6 → IPv4 pour clients externes. Sauf en mode Cloud Gateway.

## Infrastructure

| Rôle | Host | IP | Usage |
|------|------|----|-------|
| **DEV** | cloudmaster | 10.0.0.10 | Build, tests, développement |
| **PROD** | — | 10.0.0.254 | Exécution de HomeRoute |

⚠️ **JAMAIS** démarrer homeroute sur le serveur de dev. Aucun service ne doit y tourner.

## Commandes

```bash
# Build (sûr sur dev)
make server          # cargo build --release -p homeroute
make netcore         # cargo build --release -p hr-netcore
make web             # npm run build (web/) seulement
make agent           # build hr-agent (auto-incrémente version)
make test            # cargo test

# Déploiement vers la production (depuis dev)
make deploy-prod     # build all + rsync + restart homeroute (PAS hr-netcore) + health check
make deploy-netcore  # build + rsync + restart hr-netcore (rare, seulement si DNS/DHCP change)
make agent-prod      # push hr-agent vers les containers de prod

# Déploiement local (UNIQUEMENT sur le serveur de prod lui-même)
make deploy          # build all + systemctl restart (bloqué sur dev)

# Monitoring prod (via SSH)
ssh root@10.0.0.254 'journalctl -u homeroute -f'
ssh root@10.0.0.254 'journalctl -u hr-netcore -f'
curl -s http://10.0.0.254:4000/api/health | jq
ssh root@10.0.0.254 'systemctl reload homeroute'   # hot-reload proxy config (SIGHUP)
ssh root@10.0.0.254 'systemctl reload hr-netcore'  # hot-reload DNS/DHCP/Adblock config (SIGHUP)
```

## Règles obligatoires

- **JAMAIS** `cargo run` directement — utiliser `make deploy-prod` depuis dev
- **JAMAIS** `make deploy` sur le serveur de dev (bloqué par sécurité)
- **JAMAIS** `systemctl start/restart homeroute` sur le serveur de dev
- **TOUJOURS** `make deploy-prod` après modification du backend Rust (depuis dev) — ne restart que homeroute, DNS reste up
- **TOUJOURS** `make deploy-netcore` si modification de hr-dns, hr-dhcp, hr-ipv6, hr-adblock, hr-netcore ou hr-ipc
- **TOUJOURS** `make agent && make agent-prod` après modification du crate `hr-agent`
- Exécuter les commandes dans les containers via **`POST /api/applications/{id}/exec`** (pas machinectl)
- Passer les commandes comme un seul string bash : `command: ["cmd"]`

## Équipes d'agents (OBLIGATOIRE)

**TOUJOURS** créer une équipe (TeamCreate + Task) sauf pour les modifications triviales (typo, 1 ligne).

### Subagents spécialisés disponibles

| Tâche | subagent_type |
|-------|---------------|
| Backend Rust, crates/, API | `backend-rust` |
| Frontend React/Vite (web/ ou web-studio/) | `frontend-react` |
| Mise à jour binaire hr-agent dans les containers | `agent-updater` |
| Autre (investigations, scripts) | `general-purpose` |

### Répartitions types

- **Fullstack** : `backend-rust` + `frontend-react` en parallèle
- **Refactoring** : un agent `backend-rust` par crate concernée
- **Bug** : un agent investigation + un agent correctif

### Reporting — limitation connue et workarounds

Les agents peuvent parfois ne pas marquer leurs tâches comme complètes. Pour contourner :

1. **Inclure dans chaque prompt de spawn** : _"Quand tu as terminé : appelle TaskUpdate pour marquer la tâche completed, puis envoie-moi un SendMessage résumant ce que tu as fait."_
2. **Si un agent semble bloqué** : lui envoyer un SendMessage — _"Où en es-tu ? Si terminé, marque la tâche et résume."_
3. Les subagents `backend-rust`, `frontend-react` et `agent-updater` incluent déjà ces instructions dans leur system prompt.
