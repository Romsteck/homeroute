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
│   ├── hr-db/                 #   Dataverse engine (SQLite, shared lib)
│   ├── hr-environment/        #   types, protocole, config des environnements
│   └── hr-pipeline/           #   pipelines déploiement (store, runner, migration)
├── api/                       # Service homeroute (4000)
│   ├── homeroute/             #   binaire (thin shell)
│   └── hr-api/                #   routes axum, WebSocket events
└── agents/                    # Binaires autonomes
    ├── hr-agent/              #   agent dans containers nspawn (legacy)
    ├── hr-host-agent/         #   agent hôte distant
    └── env-agent/             #   agent environnement (multi-app, DB, studio)
```

### Environnements (nouveau)

HomeRoute gère des **environnements** inspirés de Microsoft Power Platform :

- 1 container nspawn = 1 environnement (dev/acc/prod)
- N apps comme processus dans chaque env
- `env-agent` pilote chaque env (DB, apps, MCP, studio)
- Pipelines pour promouvoir entre envs (build → test → migrate DB → deploy → health)

```
make.mynetwk.biz                →  Maker portal (apps, envs, pipelines)
studio.dev.mynetwk.biz          →  Studio env DEV (code-server + Claude Code)
{app}.{env}.mynetwk.biz         →  App dans un env
```

Voir `PLAN-ENVIRONMENTS.md` pour le plan complet.

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
| Agent registry (legacy) | `/var/lib/server-dashboard/agent-registry.json` |
| Environments | `/var/lib/server-dashboard/environments.json` |
| Pipelines | `/var/lib/server-dashboard/pipelines.json` |
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
make studio-env      # build web-studio-env (studio environnements)
make agent           # build hr-agent (auto-incrémente version)
make env-agent       # build env-agent
make test            # cargo test

# Déploiement vers la production (depuis dev)
make deploy-prod         # build all + rsync + restart homeroute + health check
make deploy-edge         # build + rsync + restart hr-edge seul
make deploy-orchestrator # build + rsync + restart hr-orchestrator seul
make deploy-netcore      # build + rsync + restart hr-netcore (rare)
make deploy-env-agent    # build + deploy env-agent vers TOUS les containers d'env + studio-env frontend
make deploy-studio-env   # build + deploy studio-env frontend seul

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
- **TOUJOURS** `make agent` après modification du crate `hr-agent` (legacy, calendar uniquement)
- **TOUJOURS** `make deploy-env-agent` après modification du crate `env-agent` (build + deploy vers tous les containers d'env)
- **TOUJOURS** `make deploy-orchestrator` si modification de hr-environment, hr-pipeline, hr-db
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

Les applications sont gérées via des **environnements** (env-dev, env-prod). Chaque environnement est un container nspawn piloté par un env-agent. Les anciens containers per-app (hr-v2-*) ont été supprimés.

### Environnements actifs

| Env | Container | IP | Apps | Studio |
|-----|-----------|-----|------|--------|
| dev | env-dev | 10.0.0.105 | 10 apps | studio.dev.mynetwk.biz |
| prod | env-prod | 10.0.0.112 | 0 apps (à déployer) | studio.prod.mynetwk.biz |

### Développement (sources sur CloudMaster)

- Sources : `/ssd_pool/apps/<name>/`
- Les sources sont copiées dans env-dev : `/apps/<name>/`
- DBs dans env-dev : `/opt/env-agent/data/db/<name>.db`

### Déploiement apps dans les envs

- Build sur CloudMaster, deploy via pipeline ou manuellement dans l'env
- `make deploy-env-agent` pour mettre à jour l'env-agent dans tous les containers

### Portails

| URL | Rôle |
|-----|------|
| `hub.mynetwk.biz` | Admin center (infra, DNS, proxy, envs) |
| `make.mynetwk.biz` | Maker portal (apps, pipelines, env switcher) |
| `studio.<env>.mynetwk.biz` | Studio (code-server, board, docs, DB, logs) |
| `<app>.<env>.mynetwk.biz` | App dans un env |

### Store Flutter (app mobile HomeRoute)

- Path : `/opt/homeroute/store_flutter/`
- Build : `export PATH=/ssd_pool/flutter/bin:$PATH && flutter build apk --release`
- Deploy : `curl --data-binary @build/app/outputs/flutter-apk/app-release.apk http://10.0.0.254:4000/api/store/apps/<id>/upload`

### Règles générales apps

- **JAMAIS** build sur les conteneurs — toujours sur CloudMaster
- **JAMAIS** build sur le routeur (10.0.0.254)
- PATH Flutter : `export PATH=/ssd_pool/flutter/bin:$PATH`

## MCP Self-Improvement

Le serveur MCP HomeRoute est implémenté dans ce même repo. Claude Code peut donc faire évoluer ses propres outils MCP :

### Code source MCP

| Serveur MCP | Fichier source |
|-------------|---------------|
| **Orchestrator MCP** (HTTP, tools infra + envs) | `crates/orchestrator/hr-orchestrator/src/mcp.rs` |
| **Agent MCP** (stdio, Store + Studio + Docs) | `crates/agents/hr-agent/src/mcp.rs` |
| **Env-Agent MCP** (stdio + HTTP, db.* + app.* + env.*) | `crates/agents/env-agent/src/mcp.rs` |

Fichiers connexes :
- `crates/agents/hr-agent/src/mcp_instructions.txt` — instructions incluses dans le MCP agent

### Ce que Claude Code peut faire

- **Ajouter de nouveaux tools** : créer le handler dans le code MCP, l'enregistrer dans la liste des tools, et ajouter le nom dans `generate_mcp_json()` pour l'auto-approve
- **Fixer des bugs** : les tools MCP existants peuvent avoir des bugs — Claude Code peut les diagnostiquer et corriger
- **Adapter le protocole** : si le protocole MCP évolue, Claude Code peut mettre à jour l'implémentation
- **Mettre à jour les instructions** : modifier `mcp_instructions.txt` pour améliorer le contexte fourni aux tools

C'est du self-improvement : Claude Code améliore les outils qu'il utilise lui-même via MCP.

### Workflow après modification MCP

- Si modification de `crates/orchestrator/hr-orchestrator/src/mcp.rs` → `make deploy-orchestrator`
- Si modification de `crates/agents/hr-agent/src/mcp.rs` → `make agent && make agent-prod`
- Si modification de `crates/agents/env-agent/src/mcp.rs` → `make env-agent` (puis déployer dans les envs)

## Documentation des Apps

Les applications HomeRoute disposent d'un système de documentation centralisé stocké dans `/opt/homeroute/data/docs/`. Chaque app a 5 sections : `meta` (JSON), `structure`, `features`, `backend`, `notes` (Markdown).

### Outils MCP docs

| Outil | Usage |
|-------|-------|
| `docs.list` | Lister les apps documentées avec statut de complétude |
| `docs.get` | Lire la doc d'une app (params: `app_id`, `section` optionnel) |
| `docs.create` | Créer le scaffold pour une nouvelle app |
| `docs.update` | Mettre à jour une section (params: `app_id`, `section`, `content`) |
| `docs.search` | Recherche full-text dans toutes les docs |
| `docs.completeness` | Vérifier sections remplies vs vides |

### Règle obligatoire

- **TOUJOURS** lire la doc (`docs.get`) avant de modifier significativement une app
- **TOUJOURS** mettre à jour la doc après ajout/modification de features, structure, ou backend
- Descriptions orientées utilisateur, pas techniques
