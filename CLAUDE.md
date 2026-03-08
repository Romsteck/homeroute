# Notes pour Claude Code

## Architecture

HomeRoute utilise **4 binaires** communiquant via IPC Unix sockets :

```
hr-edge (443, 80)                hr-orchestrator (4001)
  ├─ hr-proxy (reverse proxy)      ├─ hr-registry (agents)
  ├─ hr-acme (Let's Encrypt)       ├─ hr-container (nspawn)
  ├─ hr-auth (sessions)            ├─ hr-git (bare repos)
  └─ hr-tunnel (QUIC client)       └─ WebSocket agents/hosts
         │                                │
         └──────── IPC ───────────────────┘
                     │
                 homeroute (4000)         hr-netcore (53, 67)
                   ├─ hr-api (REST)        ├─ hr-dns
                   ├─ WebSocket events     ├─ hr-dhcp
                   └─ SPA web/dist         ├─ hr-adblock
                                           └─ hr-ipv6
```

- **`hr-netcore`** : DNS, DHCP, Adblock, IPv6 — ne redémarre quasi jamais
- **`hr-edge`** : Reverse proxy HTTPS, TLS/ACME, Auth, Cloud relay tunnel — restart rare
- **`hr-orchestrator`** : Containers nspawn, Agent registry, Git, WebSocket agents/hosts — restart modéré
- **`homeroute`** : API REST thin shell, WebSocket events frontend, SPA — restart fréquent (zéro impact proxy)
- **Communication** : IPC Unix sockets (`/run/hr-{service}.sock`, JSON-line)
- **Frontend** : React/Vite dans `web/` — fichiers statiques servis par Rust
- **Services** : `hr-netcore.service` + `hr-edge.service` + `hr-orchestrator.service` + `homeroute.service`

### Cargo Workspace

```
crates/
├── shared/                    # Fondations partagées
│   ├── hr-common/             #   types, config, EventBus, supervisor
│   └── hr-ipc/                #   transport Unix socket, protocoles IPC
├── edge/                      # Service hr-edge (443, 80)
│   ├── hr-edge/               #   binaire
│   ├── hr-proxy/              #   reverse proxy HTTPS, SNI, WebSocket
│   ├── hr-acme/               #   ACME Let's Encrypt (DNS-01 Cloudflare)
│   ├── hr-auth/               #   sessions SQLite, Argon2id
│   └── hr-tunnel/             #   client QUIC vers cloud relay
├── netcore/                   # Service hr-netcore (53, 67)
│   ├── hr-netcore/            #   binaire
│   ├── hr-dns/                #   DNS UDP/TCP, cache, upstream
│   ├── hr-dhcp/               #   DHCPv4, leases, DORA
│   ├── hr-adblock/            #   FxHashSet, sources, whitelist
│   └── hr-ipv6/               #   RA + DHCPv6 stateless
├── orchestrator/              # Service hr-orchestrator (4001)
│   ├── hr-orchestrator/       #   binaire
│   ├── hr-registry/           #   applications, agents WebSocket
│   ├── hr-container/          #   systemd-nspawn lifecycle
│   ├── hr-git/                #   bare repos, Smart HTTP
│   └── hr-dataverse/          #   schémas + requêtes SQLite agents
├── api/                       # Service homeroute (4000)
│   ├── homeroute/             #   binaire (thin shell)
│   └── hr-api/                #   routes axum, WebSocket events
└── agents/                    # Binaires autonomes
    ├── hr-agent/              #   agent dans containers nspawn
    ├── hr-host-agent/         #   agent hôte distant
    └── hr-cloud-relay/        #   serveur QUIC relay (VPS)
```

### IPC Sockets

| Socket | Protocole | Propriétaire |
|--------|-----------|-------------|
| `/run/hr-netcore.sock` | `IpcRequest` / `IpcResponse` | hr-netcore |
| `/run/hr-edge.sock` | `EdgeRequest` / `IpcResponse` | hr-edge |
| `/run/hr-orchestrator.sock` | `OrchestratorRequest` / `IpcResponse` | hr-orchestrator |

## Stockage

| Données | Chemin |
|---------|--------|
| Sessions SQLite | `/opt/homeroute/data/auth.db` |
| Users YAML | `/opt/homeroute/data/users.yml` |
| Hosts JSON | `/data/hosts.json` |
| Config proxy/DNS/DHCP | `/var/lib/server-dashboard/*.json` |
| Certificats ACME | `/var/lib/server-dashboard/acme/` |
| Agent registry | `/var/lib/server-dashboard/agent-registry.json` |
| Containers V2 | `/var/lib/server-dashboard/containers-v2.json` |
| Env config | `/opt/homeroute/.env` |

## Ports

| Port | Service | Binaire |
|------|---------|---------|
| 443 | HTTPS reverse proxy | hr-edge |
| 80 | HTTP→HTTPS redirect | hr-edge |
| 4000 | API management REST | homeroute |
| 4001 | Orchestrator (WebSocket agents/hosts) | hr-orchestrator |
| 53 | DNS | hr-netcore |
| 67 | DHCP | hr-netcore |

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
make edge            # cargo build --release -p hr-edge
make orchestrator    # cargo build --release -p hr-orchestrator
make netcore         # cargo build --release -p hr-netcore
make web             # npm run build (web/) seulement
make agent           # build hr-agent (auto-incrémente version)
make test            # cargo test

# Déploiement vers la production (depuis dev)
make deploy-prod         # build all + rsync + restart homeroute + health check
make deploy-edge         # build + rsync + restart hr-edge seul
make deploy-orchestrator # build + rsync + restart hr-orchestrator seul
make deploy-netcore      # build + rsync + restart hr-netcore (rare)
make agent-prod          # push hr-agent vers les containers de prod

# Déploiement local (UNIQUEMENT sur le serveur de prod lui-même)
make deploy          # build all + systemctl restart (bloqué sur dev)

# Monitoring prod (via SSH)
ssh root@10.0.0.254 'journalctl -u homeroute -u hr-edge -u hr-orchestrator -f'
ssh root@10.0.0.254 'journalctl -u hr-netcore -f'
curl -s http://10.0.0.254:4000/api/health | jq
ssh root@10.0.0.254 'systemctl reload hr-edge'      # hot-reload proxy config (SIGHUP)
ssh root@10.0.0.254 'systemctl reload hr-netcore'    # hot-reload DNS/DHCP/Adblock (SIGHUP)
```

## Règles obligatoires

- **JAMAIS** `cargo run` directement — utiliser `make deploy-prod` depuis dev
- **JAMAIS** `make deploy` sur le serveur de dev (bloqué par sécurité)
- **JAMAIS** `systemctl start/restart homeroute` sur le serveur de dev
- **TOUJOURS** `make deploy-prod` après modification du backend Rust (depuis dev)
- **TOUJOURS** `make deploy-edge` si modification de hr-proxy, hr-acme, hr-auth, hr-tunnel, hr-edge
- **TOUJOURS** `make deploy-orchestrator` si modification de hr-registry, hr-container, hr-git, hr-orchestrator
- **TOUJOURS** `make deploy-netcore` si modification de hr-dns, hr-dhcp, hr-ipv6, hr-adblock, hr-netcore
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
