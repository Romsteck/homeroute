# Notes pour Claude Code

## Architecture

HomeRoute est un **binaire Rust unifié** gérant tous les services réseau.

- **Frontend** : React/Vite dans `web/` — fichiers statiques servis par Rust
- **Backend** : Cargo workspace dans `crates/` — un seul binaire `homeroute`
- **Service** : `homeroute.service` (systemd)

### Cargo Workspace

```
crates/
├── homeroute/       # Binaire principal (supervisor)
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

| Port | Service |
|------|---------|
| 443 | HTTPS reverse proxy (hr-proxy) |
| 80 | HTTP→HTTPS redirect |
| 53 | DNS (hr-dns) |
| 67 | DHCP (hr-dhcp) |
| 4000 | API management (hr-api) |

## Cloudflare

⚠️ **JAMAIS désactiver le mode proxied** — convertit IPv6 → IPv4 pour clients externes. Sauf en mode Cloud Gateway.

## Commandes

```bash
make deploy          # build tout + systemctl restart homeroute
make server          # cargo build --release seulement
make web             # npm run build (web/) seulement
make agent           # build hr-agent (auto-incrémente version) + copie dans data/agent-binaries/
make test            # cargo test
journalctl -u homeroute -f
curl -s http://localhost:4000/api/health | jq
systemctl reload homeroute   # hot-reload config proxy (SIGHUP, sans restart)
```

## Règles obligatoires

- **JAMAIS** `cargo run` directement — utiliser `systemctl` et `make deploy`
- **TOUJOURS** `make deploy` après modification du backend Rust
- **TOUJOURS** `make agent` après modification du crate `hr-agent` (auto-incrémente la version, build, copie le binaire)
- Pour pousser le binaire `hr-agent` vers les containers : `curl -X POST http://localhost:4000/api/applications/agents/update` ou utiliser le subagent `agent-updater`
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
