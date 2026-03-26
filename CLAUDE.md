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
- **`hr-edge`** : Reverse proxy HTTPS, TLS/ACME, Auth — restart rare
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
│   └── hr-tunnel/             #   client QUIC tunnel
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

## Applications HomeRoute

Les applications tournent dans des conteneurs nspawn gérés par HomeRoute. Le développement se fait sur CloudMaster (10.0.0.10), dans `/ssd_pool/apps/`.

### Apps Axum + Vite/React

| App | Path dev | Conteneur prod | IP prod |
|-----|----------|----------------|---------|
| trader | `/ssd_pool/apps/trader/` | hr-v2-trader-prod | 10.0.0.122 |
| wallet | `/ssd_pool/apps/wallet/` | hr-v2-wallet-prod | 10.0.0.103 |
| home | `/ssd_pool/apps/home/` | hr-v2-home-prod | 10.0.0.127 |
| files | `/ssd_pool/apps/files/` | hr-v2-files-prod | 10.0.0.109 |

**Procédure dev :**
- Backend : `cd /ssd_pool/apps/<name>/ && cargo build --release`
- Frontend : `cd /ssd_pool/apps/<name>/web && pnpm build`
- Deploy : copier le binaire et les assets dans le conteneur prod, puis restart le service
- Test : `curl http://<IP_PROD>:3000/api/health`
- Logs : `sudo ssh root@10.0.0.254 machinectl shell hr-v2-<name>-prod /bin/journalctl -f`

### Apps Next.js

| App | Path dev | Conteneur prod | IP prod |
|-----|----------|----------------|---------|
| padel | `/ssd_pool/apps/padel/` | hr-v2-padel-prod | 10.0.0.117 |
| www | `/ssd_pool/apps/www/` | hr-v2-www-prod | 10.0.0.125 |
| forge | `/ssd_pool/apps/forge/forge/` | hr-v2-forge-prod | 10.0.0.128 |
| aptymus | `/ssd_pool/apps/aptymus/` | hr-v2-aptymus-prod | 10.0.0.119 |

**Procédure dev :**
- Build : `cd /ssd_pool/apps/<name>/ && pnpm build`
- Deploy : copier le build dans le conteneur prod
- Test : `curl http://<IP_PROD>:3000/`

### App Axum + Flutter (MyFrigo)

- Path dev : `/ssd_pool/apps/myfrigo/`
- Conteneur prod : hr-v2-myfrigo-prod (10.0.0.126)
- Backend : `cargo build --release`
- Mobile : `export PATH=/ssd_pool/flutter/bin:$PATH && flutter build apk`
- Auth : désactivée (attente SSO HomeRoute)

### Store Flutter (app mobile HomeRoute)

- Path : `/opt/homeroute/store_flutter/` (PAS dans `/ssd_pool/apps/`)
- Build : `export PATH=/ssd_pool/flutter/bin:$PATH && flutter build apk --release`
- Deploy : `curl --data-binary @build/app/outputs/flutter-apk/app-release.apk http://10.0.0.254:4000/api/store/apps/<id>/upload`
- Le store est file-based (`catalog.json`), pas SQLite
- C'est l'app mobile principale — toujours prioriser l'app Flutter pour les changements store

### Calendar App

- Conteneur : hr-v2-calendar-prod (10.0.0.110)
- Stack : Next.js
- En pause actuellement

### Règles générales apps

- **JAMAIS** build sur les conteneurs prod — toujours sur CloudMaster
- **JAMAIS** build sur le routeur (10.0.0.254)
- Le routeur prod est accessible via : `sudo ssh root@10.0.0.254`
- Les conteneurs sont dans `/var/lib/machines/` sur le routeur
- Pour exécuter une commande dans un conteneur : `sudo ssh root@10.0.0.254 machinectl shell hr-v2-<name>-prod /bin/bash -c "commande"`
- PATH Flutter : `export PATH=/ssd_pool/flutter/bin:$PATH`

## MCP Self-Improvement

Le serveur MCP HomeRoute est implémenté dans ce même repo. Claude Code peut donc faire évoluer ses propres outils MCP :

### Code source MCP

| Serveur MCP | Fichier source |
|-------------|---------------|
| **Orchestrator MCP** (HTTP, tools infra) | `crates/orchestrator/hr-orchestrator/src/mcp.rs` |
| **Agent MCP** (stdio, Dataverse + Deploy + Store + Studio + Docs) | `crates/agents/hr-agent/src/mcp.rs` |

Fichiers connexes :
- `crates/agents/hr-agent/src/mcp_instructions.txt` — instructions incluses dans le MCP agent
- `crates/agents/hr-agent/src/dataverse.rs` — opérations Dataverse utilisées par le MCP agent

### Ce que Claude Code peut faire

- **Ajouter de nouveaux tools** : créer le handler dans le code MCP, l'enregistrer dans la liste des tools, et ajouter le nom dans `generate_mcp_json()` pour l'auto-approve
- **Fixer des bugs** : les tools MCP existants peuvent avoir des bugs — Claude Code peut les diagnostiquer et corriger
- **Adapter le protocole** : si le protocole MCP évolue, Claude Code peut mettre à jour l'implémentation
- **Mettre à jour les instructions** : modifier `mcp_instructions.txt` pour améliorer le contexte fourni aux tools

C'est du self-improvement : Claude Code améliore les outils qu'il utilise lui-même via MCP.

### Workflow après modification MCP

- Si modification de `crates/orchestrator/hr-orchestrator/src/mcp.rs` → `make deploy-orchestrator`
- Si modification de `crates/agents/hr-agent/src/mcp.rs` → `make agent && make agent-prod`
