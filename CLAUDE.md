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
- **`hr-edge`** : Reverse proxy HTTPS, TLS/ACME, Auth — restart rare. **Pousse aussi les FQDN du proxy vers le DNS local** (`DnsRouteSync`, owner `hr-edge` dans les `static_records`).
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

### Apps locales — topologie split DEV/PROD

Depuis 2026-04-27, les apps HomeRoute vivent en **topologie split** :

- **Sources canoniques** : sur **CloudMaster** (10.0.0.10) sous `/opt/homeroute/apps/{slug}/src/`. C'est là que tu édites le code dans le studio.
- **Runtime** : sur **Medion** (10.0.0.254). Le supervisor Tokio (`hr-apps`) lance les processus depuis les artefacts buildés rsyncés. `db.sqlite`, `.env` et le port-registry restent sur Medion (jamais déplacés).
- **Build** : exclusivement sur CloudMaster (toolchains cargo/npm/pnpm/flutter). Medion n'a aucune toolchain.
- **Deploy** : `POST /api/apps/{slug}/deploy` (alias de `/build`) → CloudMaster compile, rsync les artefacts vers Medion, hr-apps restart le process. Endpoint exposé dans le frontend via le bouton "Deploy" per-app.

Le flag `sources_on: SourcesLocation` sur chaque `Application` (`medion` | `cloudmaster`) contrôle si le build skippe le rsync UP. Les apps créées après 2026-04-27 sont en `cloudmaster` par défaut. Les apps existantes ont été migrées en Phase 6.

- 1 app = 1 dossier `/opt/homeroute/apps/{slug}/` (sur les DEUX hosts, contenus différents)
- URL publique : `{slug}.mynetwk.biz` (route ajoutée à hr-edge sur Medion)
- Visibility : `public` (anon) ou `private` (auth via hr-auth)
- Code-server global sur `codeserver.mynetwk.biz` → reverse-proxy hr-edge Medion → CloudMaster `10.0.0.10:8443` (hr-studio.service côté CloudMaster), workspace `/opt/homeroute/apps/`
- Si CloudMaster est éteint : l'iframe Studio affiche un bouton "Démarrer CloudMaster" qui appelle `POST /api/hosts/{cloudmaster_id}/wake` (WOL via NIC `b4:2e:99:c9:e7:5f`).
- Backup nightly automatique : systemd timer `cm-backup-apps-src.timer` sur CloudMaster → `medion:/ssd_pool/backups/cm-apps-src/YYYY-MM-DD/`, hardlink-based, rétention 14j.

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

## DNS local (synchro avec le reverse proxy)

Le DNS local (`hr-netcore` sur 10.0.0.254) **n'a plus de wildcard** depuis 2026-04-30. Tout FQDN sous `mynetwk.biz` qui n'a pas de route reverse proxy explicite retourne **NXDOMAIN** sur le LAN. La zone reste authoritative — pas de forward upstream pour les domaines locaux.

Les FQDN connus sont **automatiquement** poussés dans le DNS local par hr-edge (`DnsRouteSync`) à chaque mutation de route :

- builtins (`proxy.`, `auth.`)
- routes manuelles enabled (`reverseproxy-config.json::hosts[]`)
- toutes les apps (locales et distantes)

Tous résolvent vers `EDGE_SERVER_IP` (= 10.0.0.254 actuellement) — c'est hr-edge qui termine TLS pour tout le monde.

Côté `dns-dhcp-config.json::dns.static_records`, ces records sont marqués `managed_by: "hr-edge"`. Les records sans ce champ sont des records **utilisateur** (édités à la main) — ils ne sont jamais écrasés par le sync. Ne jamais ajouter manuellement un record avec `managed_by: "hr-edge"` — il sera supprimé au prochain push.

L'apex `mynetwk.biz` n'est volontairement pas inclus : il retourne NXDOMAIN.

Vérification rapide : `bash scripts/smoke-test-dns-sync.sh` (sur Medion) — toutes les routes doivent résoudre, les domaines inconnus doivent NXDOMAIN.

L'accès externe via Cloudflare n'est pas affecté — Cloudflare a son propre wildcard. **À ce jour, le wildcard Cloudflare est encore actif** : un FQDN inconnu via Internet pointe toujours vers Medion. Une migration analogue côté Cloudflare est à planifier (TODO).

## Cloudflare

⚠️ **JAMAIS désactiver le mode proxied** — convertit IPv6 → IPv4 pour clients externes. Sauf en mode Cloud Gateway.

## Infrastructure

| Rôle | Host | IP | Usage |
|------|------|----|-------|
| **DEV** | cloudmaster | 10.0.0.10 | Build, tests, développement |
| **PROD** | — | 10.0.0.254 | Exécution de HomeRoute |

⚠️ **JAMAIS** démarrer homeroute (le service Rust) sur CloudMaster. Seul tourne là-bas : code-server du Studio (`hr-studio.service`, port 8443, **user système `hr-studio`**) servant les sources des apps, `hr-host-agent` (heartbeat), `cm-backup-apps-src.timer` (backup nightly), et le code-server perso de l'utilisateur (port 9080, `code.mynetwk.biz`, user `romain`).

**Isolation Studio ↔ perso** : `hr-studio.service` tourne sous le user système dédié `hr-studio` (HOME=`/var/lib/hr-studio`) — son `~/.claude/` (mémoire, settings, MCP, auto-approve) est totalement isolé du `~/.claude/` du user `romain`. Le dossier `/opt/homeroute/apps/` est `romain:hr-studio` avec setgid sur les dirs et `g+rwX` ; `romain` est dans le groupe `hr-studio` pour pouvoir éditer librement les fichiers créés par le Studio (et inversement). Le service force `UMask=0002` pour que les fichiers créés héritent du `g+rw`.

⚠️ **TOUTES les commandes `make deploy-*`** sont gardées par `check-on-cloudmaster` qui vérifie `hostname == "cloudmaster"`. Override exceptionnel : `make deploy-* FORCE_BUILD=1` (déconseillé).

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

## Applications HomeRoute

Les applications sont gérées **directement sur l'hôte** par le supervisor `hr-apps` intégré à `hr-orchestrator`. Pas de containers, pas d'environnements, pas de pipelines.

### Modèle

- 1 app = 1 dossier `/opt/homeroute/apps/{slug}/` + 1 processus supervisé Tokio
- Registre : `/opt/homeroute/data/apps.json`
- Allocation de port automatique : `/opt/homeroute/data/port-registry.json`
- URL : `{slug}.mynetwk.biz` — route ajoutée automatiquement à hr-edge
- Visibility : `public` (accès anonyme) ou `private` (auth via hr-auth)
- Logs : capturés par le supervisor (stdout/stderr) → exposés via l'API logs

### Workspace per-app (⚠ règle invariante)

Le workspace code-server d'une app est **`{slug}/src/`**, pas `{slug}/`. Tous les fichiers destinés à l'agent Claude Code (`CLAUDE.md`, `.claude/`, `.mcp.json`) DOIVENT vivre sous `src/` — tout fichier au-dessus de `src/` est invisible pour l'agent et sera supprimé automatiquement.

`src/CLAUDE.md` est **agent-owned** (write-once) ; les infos dynamiques (stack, port, autres apps) sont dans `src/.claude/rules/app-info.md` (régénéré).

Détails complets et check de violation : [.claude/rules/apps-workspace-layout.md](.claude/rules/apps-workspace-layout.md).

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

Tools `db.*` disponibles : `db.tables`, `db.describe`, `db.query` (SQL brut SELECT), `db.find` (query déclarative : filtres, sort, pagination, expand relations — pas de SQL), `db.execute` (SQL brut mutations), `db.overview`, `db.count_rows`, `db.get_schema`, `db.sync_schema`, `db.create_table`, `db.drop_table`, `db.add_column`, `db.remove_column`, `db.create_relation`.

### Ce que Claude Code peut faire

- **Ajouter de nouveaux tools** : créer le handler dans le code MCP, l'enregistrer dans la liste des tools, et ajouter le nom dans `generate_mcp_json()` pour l'auto-approve
- **Fixer des bugs** : les tools MCP existants peuvent avoir des bugs — Claude Code peut les diagnostiquer et corriger
- **Adapter le protocole** : si le protocole MCP évolue, Claude Code peut mettre à jour l'implémentation

C'est du self-improvement : Claude Code améliore les outils qu'il utilise lui-même via MCP.

### Workflow après modification MCP

- Si modification de `crates/orchestrator/hr-orchestrator/src/mcp.rs` → `make deploy-orchestrator`

## Documentation des Apps (DOC-FIRST OBLIGATOIRE)

Chaque app HomeRoute possède une documentation **structurée** stockée dans `/opt/homeroute/data/docs/{app_id}/`. La documentation est organisée en 4 catégories :

| Type | Description | Fichier |
|---|---|---|
| `overview` | Vue d'ensemble (1 par app) : pitch utilisateur, archi, index | `overview.md` |
| `screen` | Une page/écran de l'UI | `screens/{name}.md` |
| `feature` | Capacité utilisateur — globale (cross-écrans) ou per-screen (`scope=screen:<name>`) | `features/{name}.md` |
| `component` | Composant UI réutilisable | `components/{name}.md` |

Chaque entrée a un frontmatter YAML (`title`, `summary`, `scope`, `parent_screen`, `code_refs`, `links`) + un body markdown + (optionnel) un diagramme mermaid attaché à `diagrams/{type}-{name}.mmd`.

L'index full-text vit dans `/opt/homeroute/data/docs/_index.sqlite` (FTS5 BM25). Reconstructible.

### Outils MCP docs (v2)

**Lecture (auto-approuvés)** :

| Outil | Usage |
|-------|-------|
| `docs.overview` | Premier appel obligatoire — overview + index compact + stats |
| `docs.list_entries` | Liste les entrées par type |
| `docs.get` | Lire une entrée complète (frontmatter + body + diagramme) |
| `docs.search` | Recherche FTS5 BM25 (snippets surlignés, ranking) |
| `docs.completeness` | Diagnostic : missing summaries, missing diagrams |
| `docs.diagram_get` | Récupère un mermaid attaché |
| `docs.list_apps` | Liste toutes les apps documentées |

**Mutations (non auto-approuvées)** :

| Outil | Usage |
|-------|-------|
| `docs.update` | Crée/met à jour une entrée |
| `docs.delete` | Supprime une entrée (refuse l'overview) |
| `docs.diagram_set` | Attache/met à jour un diagramme mermaid (max 32 KB) |

### Workflow DOC-FIRST (obligatoire)

1. **`docs.overview(app_id=<slug>)`** AVANT toute exploration de code
2. `docs.search` (mot-clé) ou `docs.list_entries` (catégorie) pour cibler
3. `docs.get` pour lire les entrées pertinentes en détail
4. **Ensuite seulement** : exploration code + modification
5. `docs.update` pour refléter les changements
6. `docs.diagram_set` si un flux change
7. `docs.completeness` pour vérifier qu'il ne manque pas de summary/diagramme

### Règles

- **JAMAIS** coder à l'aveugle dans une app sans avoir lu sa doc
- Descriptions **orientées utilisateur** (« ce qu'il peut faire »), pas implémentation
- Si feature touche ≥ 2 écrans → `scope=global` ; sinon → `scope=screen:<name>`
- Mermaid : `flowchart LR/TD`, **boîtes carrées uniquement**, max 12 nœuds par diagramme

### REST API frontend (lecture seule)

Le frontend tab Documentation du Studio consomme `/api/docs/*` (lecture seule). **Seul l'agent modifie la doc** via MCP.

| Méthode | Route | Usage |
|---|---|---|
| GET | `/api/docs` | Liste apps |
| GET | `/api/docs/:app_id/overview` | Overview + index |
| GET | `/api/docs/:app_id/entries?type=` | Liste filtrable |
| GET | `/api/docs/:app_id/:type/:name` | Entrée complète |
| GET | `/api/docs/:app_id/:type/:name/diagram` | Diagramme mermaid |
| GET | `/api/docs/search?q=` | Recherche FTS5 |
| GET | `/api/docs/:app_id/completeness` | Diagnostic |
