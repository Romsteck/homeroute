# Notes pour Claude Code

## Architecture

HomeRoute utilise **4 binaires** communiquant via IPC Unix sockets :

```
hr-edge (443, 80)                hr-orchestrator (4001)
  ├─ hr-proxy (reverse proxy)      ├─ hr-apps (supervisor Tokio)
  ├─ hr-acme (Let's Encrypt)       ├─ hr-git (bare repos)
  ├─ hr-auth (sessions)            └─ hr-db (SQLite shared lib)
  └─ hr-tunnel (QUIC client)
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
- **`hr-orchestrator`** : Supervisor Tokio des apps locales (`hr-apps`), Git, DB SQLite — restart modéré
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
│   ├── hr-apps/               #   supervisor Tokio des apps locales (lifecycle, ports, logs)
│   ├── hr-git/                #   bare repos, Smart HTTP
│   └── hr-db/                 #   Dataverse engine (SQLite, shared lib)
└── api/                       # Service homeroute (4000)
    ├── homeroute/             #   binaire (thin shell)
    └── hr-api/                #   routes axum, WebSocket events
```

### Apps locales

HomeRoute exécute les apps directement sur l'hôte sous le contrôle d'un **supervisor Tokio** intégré à `hr-orchestrator` (`hr-apps`). Pas de container par app, pas d'environnements.

- 1 app = 1 dossier `/opt/homeroute/apps/{slug}/` + 1 processus supervisé
- URL publique : `{slug}.mynetwk.biz` (route ajoutée à hr-edge)
- Visibility : `public` (anon) ou `private` (auth via hr-auth)
- Code-server global sur `studio.mynetwk.biz` → `127.0.0.1:8443`, workspace `/opt/homeroute/apps/`

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
| Apps registry | `/opt/homeroute/data/apps.json` |
| Port registry | `/opt/homeroute/data/port-registry.json` |
| Sources des apps | `/opt/homeroute/apps/{slug}/` |
| Code-server config | `/opt/homeroute/data/code-server/` |
| Env config homeroute | `/opt/homeroute/.env` |

## Ports

| Port | Service | Binaire |
|------|---------|---------|
| 443 | HTTPS reverse proxy | hr-edge |
| 80 | HTTP→HTTPS redirect | hr-edge |
| 4000 | API management REST | homeroute |
| 4001 | Orchestrator IPC/HTTP | hr-orchestrator |
| 8443 | code-server local (studio.mynetwk.biz) | code-server |
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
make test            # cargo test

# Déploiement vers la production (depuis dev)
make deploy-prod         # build all + rsync + restart homeroute + health check
make deploy-edge         # build + rsync + restart hr-edge seul
make deploy-orchestrator # build + rsync + restart hr-orchestrator seul
make deploy-netcore      # build + rsync + restart hr-netcore (rare)

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
- **TOUJOURS** `make deploy-orchestrator` si modification de hr-apps, hr-git, hr-db, hr-orchestrator
- **TOUJOURS** `make deploy-netcore` si modification de hr-dns, hr-dhcp, hr-ipv6, hr-adblock, hr-netcore
- Exécuter une commande dans le contexte d'une app via l'API `POST /api/apps/{slug}/exec`

## Équipes d'agents (OBLIGATOIRE)

**TOUJOURS** créer une équipe (TeamCreate + Task) sauf pour les modifications triviales (typo, 1 ligne).

### Subagents spécialisés disponibles

| Tâche | subagent_type |
|-------|---------------|
| Backend Rust, crates/, API | `backend-rust` |
| Frontend React/Vite (web/) | `frontend-react` |
| Autre (investigations, scripts) | `general-purpose` |

### Répartitions types

- **Fullstack** : `backend-rust` + `frontend-react` en parallèle
- **Refactoring** : un agent `backend-rust` par crate concernée
- **Bug** : un agent investigation + un agent correctif

### Reporting — limitation connue et workarounds

Les agents peuvent parfois ne pas marquer leurs tâches comme complètes. Pour contourner :

1. **Inclure dans chaque prompt de spawn** : _"Quand tu as terminé : appelle TaskUpdate pour marquer la tâche completed, puis envoie-moi un SendMessage résumant ce que tu as fait."_
2. **Si un agent semble bloqué** : lui envoyer un SendMessage — _"Où en es-tu ? Si terminé, marque la tâche et résume."_
3. Les subagents `backend-rust` et `frontend-react` incluent déjà ces instructions dans leur system prompt.

## Applications HomeRoute

Les applications sont gérées **directement sur l'hôte** par le supervisor `hr-apps` intégré à `hr-orchestrator`. Pas de containers, pas d'environnements, pas de pipelines.

### Modèle

- 1 app = 1 dossier `/opt/homeroute/apps/{slug}/` + 1 processus supervisé Tokio
- Registre : `/opt/homeroute/data/apps.json`
- Allocation de port automatique : `/opt/homeroute/data/port-registry.json`
- URL : `{slug}.mynetwk.biz` — route ajoutée automatiquement à hr-edge
- Visibility : `public` (accès anonyme) ou `private` (auth via hr-auth)
- Logs : capturés par le supervisor (stdout/stderr) → exposés via l'API logs

### Studio (code-server global)

- URL : `studio.mynetwk.biz` → `127.0.0.1:8443`
- Workspace : `/opt/homeroute/apps/`
- Setup : `scripts/setup-studio.sh` (installe code-server local sur le routeur)

### Store Flutter (app mobile HomeRoute)

- Path : `/opt/homeroute/store_flutter/`
- Build : `export PATH=/ssd_pool/flutter/bin:$PATH && flutter build apk --release`
- Deploy : `curl --data-binary @build/app/outputs/flutter-apk/app-release.apk http://10.0.0.254:4000/api/store/apps/<id>/upload`

### Règles générales apps

- **JAMAIS** build sur le routeur (10.0.0.254) — toujours sur CloudMaster
- PATH Flutter : `export PATH=/ssd_pool/flutter/bin:$PATH`

## MCP Self-Improvement

Le serveur MCP HomeRoute est implémenté dans ce même repo. Claude Code peut donc faire évoluer ses propres outils MCP :

### Code source MCP

| Serveur MCP | Fichier source |
|-------------|---------------|
| **Orchestrator MCP** (HTTP, tools infra + `app.*` + `db.*`) | `crates/orchestrator/hr-orchestrator/src/mcp.rs` |

Tools exposés : `app.*` (lifecycle des apps locales via `hr-apps`), `db.*` (Dataverse SQLite via `hr-db`), plus les tools infra (DNS, proxy, ACME, etc.).

### Ce que Claude Code peut faire

- **Ajouter de nouveaux tools** : créer le handler dans le code MCP, l'enregistrer dans la liste des tools, et ajouter le nom dans `generate_mcp_json()` pour l'auto-approve
- **Fixer des bugs** : les tools MCP existants peuvent avoir des bugs — Claude Code peut les diagnostiquer et corriger
- **Adapter le protocole** : si le protocole MCP évolue, Claude Code peut mettre à jour l'implémentation

C'est du self-improvement : Claude Code améliore les outils qu'il utilise lui-même via MCP.

### Workflow après modification MCP

- Si modification de `crates/orchestrator/hr-orchestrator/src/mcp.rs` → `make deploy-orchestrator`

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
