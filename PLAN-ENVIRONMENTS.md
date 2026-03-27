# HomeRoute Environments — Plan de transformation

> Inspiré de Microsoft Power Platform, adapté et amélioré pour HomeRoute.
> Date: 2026-03-27

## Table des matières

1. [Vision](#1-vision)
2. [Mapping Power Platform → HomeRoute](#2-mapping-power-platform--homeroute)
3. [Architecture cible](#3-architecture-cible)
4. [Convention de nommage et routing](#4-convention-de-nommage-et-routing)
5. [Environnements](#5-environnements)
6. [Maker Portal (make.mynetwk.biz)](#6-maker-portal-makemynetwkbiz)
7. [Studio par environnement](#7-studio-par-environnement)
8. [Contexte Claude Code par projet](#8-contexte-claude-code-par-projet)
9. [Model-Driven Apps](#9-model-driven-apps)
10. [Pipelines et déploiement](#10-pipelines-et-deploiement)
11. [Migration DB (hr-db → env-agent)](#11-migration-db-hr-db--env-agent)
12. [Délégation des features HomeRoute](#12-delegation-des-features-homeroute)
13. [Plan d'implémentation](#13-plan-dimplementation)
14. [Contraintes et points d'attention](#14-contraintes-et-points-dattention)

---

## 1. Vision

HomeRoute devient un **Admin Center** qui orchestre des **environnements isolés**.
Chaque environnement est un container nspawn autonome piloté par un **env-agent**.
Les applications sont des **processus** à l'intérieur des environnements (pas des containers séparés).

**Avant** : 1 container nspawn par app × par env = N×M containers.
**Après** : 1 container nspawn par env, N apps comme processus dedans.

Tout tourne sur **Medion** pour commencer (single-host, multi-env).
Le multi-host viendra plus tard, l'interface env-agent est abstraite pour le permettre.

---

## 2. Mapping Power Platform → HomeRoute

| Power Platform | HomeRoute | Notes |
|---|---|---|
| Admin Center | `hub.mynetwk.biz` | Infra, hosts, DNS, proxy, monitoring |
| Environment (dev/prod/test) | Container nspawn + env-agent | 1 container = 1 env |
| Dataverse | SQLite par app, géré par env-agent | 1 DB engine par env, 1 schema/app |
| Power Apps (dans un env) | Processus dans le container | Pas des containers séparés |
| Solutions (packaging) | Git repo + manifest par app | Le code EST la solution, le tag EST la version |
| Pipelines (deploy) | hr-pipeline (build → test → migrate → promote) | Git-native, pas d'export/import |
| Maker Portal | `make.mynetwk.biz` | Env switcher, vue apps, pipelines |
| Maker Studio | `studio.<env>.mynetwk.biz` | 1 studio par env (code-server + board + docs + DB) |
| Model-Driven Apps | Dataverse UI par app + par env | Schema, formulaires, vues auto-générées |

### Améliorations par rapport à Power Platform

| Limitation PP | Notre approche |
|---|---|
| Pas de vue cross-env d'une app | Timeline par app : voir l'app dans tous les envs |
| Env switcher perd le contexte | Deep links : `make.mynetwk.biz/trader?env=dev` |
| Solutions = packaging manuel | Git-native : le tag est la version |
| Pas de diff entre envs | Env diff : comparer config/versions/schemas entre envs |
| Pipelines basiques | Pipelines composables avec DB migration intégrée |
| Pas de rollback simple | Rollback 1-click avec backup DB automatique |
| Low-code only | Code-first : accès direct au code via studio |
| Pas de CI/CD natif | Pipelines intégrées avec tests, migration, health check |

---

## 3. Architecture cible

```
HomeRoute Admin Center (10.0.0.254)
  ├── hr-edge (443, 80)
  │     ├── TLS/ACME (wildcard par env)
  │     ├── Routing env-aware : {app}.{env}.mynetwk.biz → env_ip:app_port
  │     └── SSO central (hr-auth)
  │
  ├── hr-orchestrator (4001)
  │     ├── hr-environment     # lifecycle envs (create/start/stop/freeze/destroy)
  │     ├── hr-pipeline        # build → test → migrate DB → deploy → health
  │     ├── hr-container       # nspawn (inchangé, 1 container = 1 env)
  │     ├── hr-registry        # registre apps + env-agents WebSocket
  │     └── hr-git             # bare repos centraux
  │
  ├── hr-netcore (53, 67)
  │     └── DNS : *.{env}.mynetwk.biz → IP du container env
  │
  ├── homeroute (4000)
  │     ├── hr-api (REST + WebSocket)
  │     └── SPA (hub.mynetwk.biz + make.mynetwk.biz)
  │
  │
  │   ┌─── Medion (multi-env host) ────────────────────────────┐
  │   │                                                         │
  │   │  ┌─ env-dev (container, 10.0.0.200) ─────────────────┐ │
  │   │  │  env-agent (MCP, full access)                      │ │
  │   │  │  ├── DbManager (SQLite par app)                    │ │
  │   │  │  ├── Studio (code-server, Claude Code)             │ │
  │   │  │  ├── App supervisor (start/stop/restart par app)   │ │
  │   │  │  ├── Log collector                                 │ │
  │   │  │  ├── Secrets vault                                 │ │
  │   │  │  └── Apps :                                        │ │
  │   │  │       ├── trader (process, :3001)                  │ │
  │   │  │       ├── wallet (process, :3002)                  │ │
  │   │  │       ├── home (process, :3003)                    │ │
  │   │  │       ├── files (process, :3004)                   │ │
  │   │  │       └── ...                                      │ │
  │   │  └────────────────────────────────────────────────────┘ │
  │   │                                                         │
  │   │  ┌─ env-prod (container, 10.0.0.202) 🔒 ─────────────┐ │
  │   │  │  env-agent (MCP, read-only sauf pipeline)          │ │
  │   │  │  ├── DbManager (SQLite par app, write via pipeline)│ │
  │   │  │  ├── App supervisor                                │ │
  │   │  │  ├── Log collector                                 │ │
  │   │  │  ├── Secrets vault                                 │ │
  │   │  │  └── Apps :                                        │ │
  │   │  │       ├── trader (process, :3001)                  │ │
  │   │  │       ├── wallet (process, :3002)                  │ │
  │   │  │       └── ...                                      │ │
  │   │  └────────────────────────────────────────────────────┘ │
  │   │                                                         │
  │   │  ┌─ env-acc (container, 10.0.0.201) ── optionnel ────┐ │
  │   │  │  env-agent (MCP, snapshot de prod + builds récents)│ │
  │   │  │  └── ...                                           │ │
  │   │  └────────────────────────────────────────────────────┘ │
  │   └─────────────────────────────────────────────────────────┘
  │
  └── Futur : multi-host (env-agents sur d'autres machines)
```

### Ports dans un environnement

Chaque container a sa propre IP, donc pas de conflit de ports entre envs.
À l'intérieur d'un env, l'env-agent assigne les ports :

| Port | Usage |
|------|-------|
| 4010 | env-agent MCP HTTP |
| 4011 | env-agent WebSocket (vers orchestrator) |
| 3000-3099 | Apps (assignés dynamiquement ou par config) |
| 8443 | code-server (studio) |

---

## 3b. Stack technique

| Composant | Backend | Frontend | Notes |
|---|---|---|---|
| **env-agent** | Rust (binaire) | — | Tourne dans chaque container env |
| **hr-environment** | Rust (crate orchestrator) | — | Lifecycle envs |
| **hr-pipeline** | Rust (crate orchestrator) | — | Build/test/migrate/deploy |
| **Maker Portal** | Axum (API dédiée ou routes hr-api) | Vite + React | App séparée du hub, dans `/ssd_pool/apps/make/` |
| **Hub** | Existant (hr-api) | Existant (web/) | Inchangé — infra/admin uniquement |
| **Studio** | env-agent (MCP + code-server mgmt) | code-server (existant) | 1 instance par env |

### Séparation Hub vs Maker Portal

| | Hub (`hub.mynetwk.biz`) | Maker Portal (`make.mynetwk.biz`) |
|---|---|---|
| **Scope** | Infra, hosts, DNS, proxy, monitoring | Apps, envs, pipelines, model-driven |
| **Audience** | Admin réseau/infra | Dev/maker d'apps |
| **Codebase** | `web/` (existant) | `/ssd_pool/apps/make/` (nouveau) |
| **Backend** | hr-api (existant) | API dédiée (nouvelles routes Axum) |
| **Déploiement** | Servi par homeroute | App dans env-prod (self-hosted) |

---

## 4. Convention de nommage et routing

### URLs

```
hub.mynetwk.biz                 →  Admin center (infra, hosts, DNS, proxy)
make.mynetwk.biz                →  Maker portal (apps, envs, pipelines)
studio.dev.mynetwk.biz          →  Studio de l'env DEV
studio.prod.mynetwk.biz         →  Studio de l'env PROD (read-only)
studio.acc.mynetwk.biz          →  Studio de l'env ACC
trader.dev.mynetwk.biz          →  App Trader en dev
trader.prod.mynetwk.biz         →  App Trader en prod
trader.mynetwk.biz              →  Alias vanity → prod
```

### Certificats TLS

Un wildcard par niveau, tous via DNS-01 Cloudflare (hr-acme) :

| Certificat | Couvre |
|---|---|
| `*.mynetwk.biz` | Prod apps (vanity), hub, make |
| `*.dev.mynetwk.biz` | Env dev : apps + studio |
| `*.prod.mynetwk.biz` | Env prod : apps + studio |
| `*.acc.mynetwk.biz` | Env acc : apps + studio |
| `*.<slug>.mynetwk.biz` | Auto-provisionné pour tout nouvel env |

### Routing hr-edge

Le routing devient env-aware. Parsing du Host header :

```
{app}.{env}.mynetwk.biz  →  lookup env → container IP → lookup app → port
{app}.mynetwk.biz        →  alias prod → env-prod IP → app port
studio.{env}.mynetwk.biz →  env container IP → code-server port (8443)
make.mynetwk.biz         →  localhost:4000 (SPA)
hub.mynetwk.biz          →  localhost:4000 (SPA)
```

L'env-agent expose un endpoint de discovery (`GET /discovery`) pour que hr-edge
sache quelles apps tournent et sur quels ports.

### DNS

hr-netcore crée automatiquement les records :

```
*.dev.mynetwk.biz   →  10.0.0.200 (IP container env-dev)
*.prod.mynetwk.biz  →  10.0.0.202 (IP container env-prod)
*.acc.mynetwk.biz   →  10.0.0.201 (IP container env-acc)
```

---

## 5. Environnements

### Définition

Un environnement est un **container nspawn** contenant :

- Un **env-agent** (évolution de hr-agent) qui pilote tout
- Un **DbManager** avec les SQLite de chaque app
- Un **app supervisor** qui gère les processus des apps
- Un **code-server** (sauf en prod)
- Les **binaires et assets** des apps
- Les **env vars et secrets** de l'env

### Types d'environnements

| Type | Slug | Éditable | Studio | DB writes | Pipeline promote |
|------|------|----------|--------|-----------|-----------------|
| **Development** | `dev` | ✅ Oui | ✅ code-server + Claude Code | ✅ Libre | → acc, prod |
| **Acceptance** | `acc` | ❌ Non | ✅ Lecture seule | ✅ Via tests | → prod |
| **Production** | `prod` | ❌ Non | ✅ Lecture seule (logs, DB read) | ❌ Via pipeline uniquement | — |

### Lifecycle

```
create-env(slug, type, host)
  → provisionne container nspawn
  → installe env-agent + dépendances
  → configure réseau (IP, DNS)
  → provisionne certificat wildcard
  → démarre env-agent

start-env(slug) → machinectl start
stop-env(slug)  → machinectl stop
freeze-env(slug) → snapshot état + stop
destroy-env(slug) → cleanup complet (comme aujourd'hui, cf. HOMEROUTE-APPS.md §5)
```

### Structure fichiers dans un env

```
/ (rootfs du container nspawn)
├── opt/
│   └── env-agent/
│       ├── env-agent              # binaire
│       ├── env-agent.toml         # config (type, slug, apps, etc.)
│       └── data/
│           ├── db/                # SQLite par app
│           │   ├── trader.db
│           │   ├── wallet.db
│           │   └── home.db
│           ├── secrets/           # vault chiffré par app
│           └── logs/              # logs agrégés
├── apps/
│   ├── trader/
│   │   ├── bin/trader             # binaire (Axum)
│   │   ├── web/dist/              # assets frontend
│   │   ├── CLAUDE.md              # contexte projet (généré par env-agent)
│   │   └── .claude/settings.json  # MCP servers pré-configurés
│   ├── wallet/
│   ├── home/
│   └── files/
├── studio/
│   └── code-server/               # instance partagée entre projets
└── etc/
    ├── systemd/system/
    │   ├── env-agent.service
    │   ├── trader.service          # 1 service systemd par app
    │   ├── wallet.service
    │   └── code-server.service
    └── env-agent.toml
```

---

## 6. Maker Portal (make.mynetwk.biz)

### Concept

Le Maker Portal est l'équivalent de `make.powerapps.com`. C'est le point d'entrée
pour gérer les applications et les environnements.

Techniquement : une **app Vite + React séparée** (pas dans le SPA hub).
Séparation claire : hub = infra/admin, make = apps/envs/pipelines.
Hébergée comme une app dans un env (self-hosting) ou servie par homeroute sur un path dédié.

### Fonctionnalités

#### Vue principale : Apps dans l'env sélectionné

```
┌─────────────────────────────────────────────────────────┐
│  make.mynetwk.biz                                       │
│  ┌─────────────────────────────────────┐                │
│  │ 🔽 Environnement: Production       │  [+ New Env]   │
│  └─────────────────────────────────────┘                │
│                                                         │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ Trader   │ │ Wallet   │ │ Home     │ │ Files    │   │
│  │ v2.3.1   │ │ v1.8.0   │ │ v3.1.0   │ │ v1.2.0   │   │
│  │ ● Online │ │ ● Online │ │ ● Online │ │ ⚠ Warn  │   │
│  │ [Open]   │ │ [Open]   │ │ [Open]   │ │ [Open]   │   │
│  │ [Studio] │ │ [Studio] │ │ [Studio] │ │ [Studio] │   │
│  │ [Logs]   │ │ [Logs]   │ │ [Logs]   │ │ [Logs]   │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
│                                                         │
│  ┌─ Pipelines récentes ────────────────────────────┐    │
│  │ ✅ Trader v2.3.1  dev → prod  il y a 2h        │    │
│  │ 🔄 Wallet v1.8.1  dev → acc   en cours...      │    │
│  │ ❌ Files  v1.2.1  dev → prod  échoué (tests)   │    │
│  └─────────────────────────────────────────────────┘    │
│                                                         │
│  ┌─ Environnements ───────────────────────────────┐     │
│  │ 🟢 prod   6 apps │ 10.0.0.202 │ 🔒 verrouillé │    │
│  │ 🟡 acc    4 apps │ 10.0.0.201 │    ouvert      │    │
│  │ 🟢 dev    6 apps │ 10.0.0.200 │    ouvert      │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

#### Env switcher

Le dropdown change tout le contexte :
- Apps affichées = celles déployées dans cet env
- Versions = celles de l'env (dev peut avoir v2.4.0-beta, prod a v2.3.1)
- Actions = adaptées (prod : pas de "Edit", juste "Logs", "Rollback", "DB read")

#### Vues additionnelles

- **Vue app cross-env** : timeline d'une app dans tous les envs (versions, statuts, derniers deploys)
- **Env diff** : comparer versions/config/schemas DB entre deux envs
- **Pipeline builder** : configurer les étapes d'une pipeline par app
- **Env settings** : config de l'env (variables, secrets, permissions)

---

## 7. Studio par environnement

### Concept

**1 studio par env**, pas par app. Le studio est une instance code-server unique
dans le container de l'env, qui switch entre les projets (dossiers d'apps).

```
studio.dev.mynetwk.biz   →  Studio de l'env DEV (éditable)
studio.prod.mynetwk.biz  →  Studio de l'env PROD (read-only)
```

Avantages :
- 1 seule config code-server par env (pas N configs pour N apps)
- 1 seul processus code-server (économie de ressources)
- Le switch de projet = changer de dossier workspace

### Onglets du studio (par projet sélectionné)

| Onglet | Fonction | En prod |
|--------|----------|---------|
| **Code** | code-server (VS Code in browser) | Read-only |
| **Board** | Kanban/todos du projet (via Hub MCP) | Read-only |
| **Docs** | Documentation app (éditeur WYSIWYG) | Read-only |
| **Pipes** | Historique deploys, trigger manual | View + rollback |
| **DB** | Explorer schema Dataverse, query builder | SELECT only |
| **Logs** | Live tail, filtrable par app/service | Full access |

### Restrictions par type d'env

| Action | DEV | ACC | PROD |
|--------|-----|-----|------|
| Modifier le code | ✅ | ❌ | ❌ |
| Build / Run local | ✅ | ❌ | ❌ |
| Modifier schema DB | ✅ | ❌ | ❌ |
| Insert/Update/Delete data | ✅ | ✅ (tests) | ❌ (pipeline only) |
| SELECT data | ✅ | ✅ | ✅ |
| Lire les logs | ✅ | ✅ | ✅ |
| Trigger pipeline promote | ✅ | ✅ | ❌ |
| Rollback | ❌ | ✅ | ✅ (via pipeline) |
| Modifier env vars | ✅ | ✅ | ❌ (pipeline only) |

---

## 8. Contexte Claude Code par projet

### Objectif

Quand Claude Code s'ouvre dans un studio sur un projet d'app, il doit avoir
**tout le contexte nécessaire** pour travailler efficacement, sans rien demander.

### Fichiers générés par l'env-agent

À chaque ouverture du studio ou changement de projet, l'env-agent
génère/met à jour les fichiers de contexte dans le dossier de l'app :

#### `CLAUDE.md` (généré dynamiquement)

```markdown
# {App Name} — Environnement {ENV_TYPE}

## Identité
- App: {name} (slug: {slug})
- Env: {env_slug} ({studio_url})
- Stack: {stack}
- Version: {version}
- URL app: {app}.{env}.mynetwk.biz

## Base de données
- Engine: SQLite via env-agent MCP (tools db.*)
- Tables: {liste des tables avec row counts}
- ⚠️ Utilise les tools MCP db.* — JAMAIS sqlite3 directement

## Commandes
- Build backend: {commande selon stack}
- Build frontend: {commande selon stack}
- Déployer dans cet env: mcp env.deploy_app({slug})
- Promouvoir vers {next_env}: mcp pipeline.promote({slug}, {next_env})
- Tests: {commande tests}

## Règles
- {règles selon le type d'env : éditable ou read-only}
- TOUJOURS passer par les pipelines pour promouvoir
- Les tests doivent passer avant tout promote

## Architecture
{contenu de docs.get(app_id, "structure")}

## Features
{contenu de docs.get(app_id, "features")}
```

#### `.claude/settings.json` (MCP servers pré-configurés)

```json
{
  "mcpServers": {
    "env": {
      "url": "http://localhost:4010/mcp",
      "note": "env-agent local — db.*, app.*, pipeline.*, studio.*"
    },
    "homeroute": {
      "url": "http://10.0.0.254:4001/mcp",
      "note": "orchestrator — proxy, DNS, monitoring (lecture)"
    },
    "hub": {
      "url": "http://10.0.0.20:3500/mcp",
      "note": "todos, jobs — context auto-scoped au projet"
    }
  }
}
```

#### `.claude/rules/env-context.md` (permissions de l'env)

Contenu adapté selon le type d'env (dev/acc/prod) — cf. section 7.

#### `.claude/rules/app-context.md` (doc app injectée)

Contenu pullé depuis `mcp__homeroute__docs_get(app_id)`.

### 3 niveaux MCP accessibles

| MCP Server | Scope | Tools principaux |
|---|---|---|
| **env-agent** (localhost:4010) | Environnement courant | `db.*`, `app.*`, `pipeline.*`, `studio.*` |
| **homeroute** (10.0.0.254:4001) | Infra globale | `proxy.*`, `dns.*`, `monitoring.*` (lecture) |
| **hub** (10.0.0.20:3500) | Gestion projet | `todos.*`, `jobs.*` (scoped au context app) |

### Flow complet

```
Développeur ouvre studio.dev.mynetwk.biz
  │
  └→ Sélectionne projet "Trader" dans le workspace
       │
       └→ env-agent détecte le switch
            │
            ├→ Génère/met à jour CLAUDE.md (identité, DB schema, commandes)
            ├→ Génère/met à jour .claude/settings.json (MCP servers)
            ├→ Injecte .claude/rules/env-context.md (permissions env)
            ├→ Injecte .claude/rules/app-context.md (doc app)
            └→ code-server ouvre /apps/trader/
                 │
                 └→ Claude Code se lance avec tout le contexte
                      │
                      └→ L'agent sait :
                           ├── quelle app, quel env, quelles permissions
                           ├── le schema DB, les tables, les relations
                           ├── la doc features/structure/backend
                           ├── les todos en cours (Hub MCP, context="trader")
                           └── comment build, test, déployer, promouvoir
```

---

## 9. Model-Driven Apps

### Concept

Chaque app a un **model-driven-app** : une UI auto-générée à partir de son schema
Dataverse (comme Power Apps model-driven). Deux niveaux :

#### 1. Model-driven par projet (dans le studio)

Accessible depuis l'onglet "DB" du studio. Permet de :
- Visualiser/éditer le schema (tables, colonnes, relations)
- Définir des formulaires (CRUD UI auto-générée)
- Définir des vues (listes filtrées, triées)
- Construire un dashboard basique

C'est l'outil de design du modèle de données.

#### 2. Model-driven par environnement

L'app model-driven déployée dans chaque env montre les **données runtime**.
En dev : données de test, en prod : données réelles (read-only sauf via l'app).

### Interface avec Claude Code

Claude Code peut interagir avec le model-driven via MCP :
- `db.create_table`, `db.add_column`, etc. pour modifier le schema
- Les modifications se reflètent automatiquement dans les formulaires
- "Ajoute une colonne status à la table trades" → `db.add_column` + UI mise à jour

---

## 10. Pipelines et déploiement

### Concept

Les pipelines remplacent le concept de "Solutions" de Power Platform.
Pas de packaging, pas d'export/import. Git-native.

### Pipeline standard : promote app vers un env cible

```
Pipeline "promote {app} v{version} → {target_env}":

  1. 📋 Pré-checks
     - Tests passent dans l'env source
     - Version taggée dans git
     - Pas de pipeline en cours pour cette app

  2. 📸 Backup
     - Snapshot DB de l'env cible (avant migration)
     - Sauvegarde binaire actuel (pour rollback)

  3. 🔄 DB Migration
     - Diff schema source vs cible (hr-db.export_migrations_since())
     - Génère les migrations DDL
     - Dry-run sur une copie de la DB cible
     - Si OK : applique sur la vraie DB

  4. 🚀 Déploiement
     - Copie binaire + assets → env cible
     - env-agent cible met à jour les fichiers
     - env-agent restart le processus de l'app

  5. 🏥 Validation
     - Health check (HTTP GET /api/health)
     - Smoke tests (optionnel, configurables)
     - Si FAIL : rollback automatique (binaire + DB)

  6. 📢 Notification
     - Status pipeline → Maker portal
     - Notification Hub (job complete)
```

### Pipelines composables

Chaque app peut configurer sa pipeline avec des étapes custom :

```toml
# /apps/trader/pipeline.toml
[promote]
steps = ["test", "backup-db", "migrate-db", "deploy", "health-check"]

[promote.test]
command = "cargo test"
timeout = 300

[promote.health-check]
url = "/api/health"
timeout = 30
retries = 3
```

### Rollback

Chaque promote garde :
- Le binaire précédent (`bin/trader.prev`)
- Le snapshot DB (`db/trader.db.bak.{timestamp}`)
- Rollback = restaurer les deux + restart

---

## 11. Migration DB (hr-db → env-agent)

### État actuel (post changements agent précédent)

- `hr-db` : crate propre dans `crates/orchestrator/hr-db/` avec `DataverseEngine`
- `DbManager` : dans `hr-orchestrator/src/db_manager.rs`, gère N DBs centralisées
- DBs stockées sur le routeur : `/opt/homeroute/data/db/{slug}.db`
- `hr-agent` : vidé de tout code Dataverse, MCP server est un no-op
- 15 outils MCP `db.*` dans l'orchestrator

### Migration cible

```
AUJOURD'HUI                              CIBLE
────────────────────────                 ────────────────────────
hr-orchestrator                          hr-orchestrator
  └── DbManager                            └── proxy MCP db.*
      └── /opt/homeroute/data/db/               → route vers env-agent
          ├── trader.db
          └── wallet.db                  env-dev (container)
                                           └── env-agent
hr-agent (vide)                                └── DbManager
                                                   └── /opt/env-agent/data/db/
                                                       ├── trader.db
                                                       └── wallet.db

                                         env-prod (container)
                                           └── env-agent
                                               └── DbManager (read via pipeline)
                                                   └── /opt/env-agent/data/db/
                                                       ├── trader.db
                                                       └── wallet.db
```

### Étapes de migration

1. `hr-db` reste un crate partagé (librairie)
2. `DbManager` descend dans l'env-agent
3. L'orchestrator garde un proxy MCP qui route `db.{tool}(app_id)` vers
   le bon env-agent via WebSocket (comme avant, mais env-aware)
4. Copie des fichiers `.db` du routeur vers les containers d'env
5. Les données de dev restent en dev, les données de prod restent en prod

### Crates impactés

| Crate | Action |
|-------|--------|
| `hr-db` | Inchangé (librairie) |
| `hr-orchestrator` | DbManager supprimé, remplacé par proxy vers env-agents |
| `hr-agent` → `env-agent` | Intègre DbManager + hr-db |
| `hr-environment` (nouveau) | Lifecycle des envs dans l'orchestrator |

---

## 12. Délégation des features HomeRoute

### Ce qui reste au niveau HomeRoute (global)

| Feature | Service | Détails |
|---------|---------|---------|
| TLS/Certs | hr-acme | Wildcards par env, provisionnement auto |
| Reverse proxy | hr-edge | Point d'entrée unique (443), route vers envs |
| DNS | hr-netcore | `*.{env}.mynetwk.biz` → IP container |
| Auth/SSO | hr-auth | Tokens émis centralement |
| Git repos | hr-git | Bare repos centraux, webhooks |
| Env lifecycle | hr-environment | Create/start/stop/destroy envs |
| Pipelines | hr-pipeline | Orchestration build → deploy cross-env |
| Monitoring | hr-api | Dashboards cross-env, alertes |

### Ce qui descend dans l'env-agent

| Feature | Détails |
|---------|---------|
| DB engine | SQLite par app, DbManager local |
| App supervisor | Start/stop/restart processus d'apps |
| Local proxy | Routage interne port → app |
| Log collector | Agrégation logs des apps |
| Secrets vault | Env vars et secrets chiffrés |
| Studio | code-server + contexte Claude Code |
| Health checks | Ping local des apps |
| Token validation | Valide les tokens SSO (pas les émet) |
| Discovery | Expose quelles apps tournent et sur quels ports |

### Communication

```
hr-edge ──────── HTTPS ──────── client
    │
    └── lookup env (Host header) → IP container
         │
    env-agent ← WebSocket → hr-orchestrator
         │                        │
         ├── MCP HTTP (4010)      ├── proxy MCP (route db.* vers env)
         ├── discovery (GET)      ├── pipeline (orchestration)
         └── apps (processes)     └── env lifecycle
```

---

## 13. Plan d'implémentation

### Phase 0 — Préparation (pas de casse)

> Objectif : préparer sans rien casser de l'existant.

- [x] **0.1** Créer le crate `hr-environment` (vide, scaffold)
- [x] **0.2** Définir le protocole env-agent (interface MCP complète)
- [x] **0.3** Définir la config `env-agent.toml` (format, champs)
- [x] **0.4** Préparer la structure Cargo workspace (ajouter les membres)

### Phase 1 — env-agent (évolution de hr-agent) ✅

> Objectif : transformer hr-agent en env-agent capable de gérer N apps + DB.

- [x] **1.1** Fork hr-agent → env-agent (nouveau binaire `env-agent`)
- [x] **1.2** Intégrer `DbManager` + `hr-db` dans env-agent
- [x] **1.3** Implémenter app supervisor (systemd services par app)
- [x] **1.4** Implémenter MCP server env-agent (23 tools : 15 db.*, 6 app.*, 2 env.*)
- [x] **1.5** Implémenter discovery endpoint (GET /discovery)
- [x] **1.6** Intégrer code-server management dans env-agent
- [x] **1.7** Implémenter la génération dynamique de CLAUDE.md et .claude/

### Phase 2 — hr-environment (orchestrator) ✅ (complet)

> Objectif : gérer le lifecycle des envs depuis l'orchestrator.

- [x] **2.1** Implémenter EnvironmentManager (CRUD, persistence JSON, token auth Argon2)
- [x] **2.2** Ajouter IPC protocol (6 variants OrchestratorRequest pour envs)
- [x] **2.3** Connecter env-agent via WebSocket (`/envs/ws` dans orchestrator)
- [x] **2.4** Proxy MCP : router db.*/app.* vers le bon env-agent
- [x] **2.5** Adapter hr-edge pour le routing env-aware ({app}.{env}.domain) — AppDiscovery → SetAppRoute
- [x] **2.6** Adapter hr-acme pour les wildcards par env — WildcardType::Environment, AcmeRequestEnvWildcard IPC
- [x] **2.7** Adapter hr-netcore pour les DNS par env — DNS wildcard connect + cleanup disconnect

### Phase 3 — Premier env-dev sur Medion ✅ (scripts prêts)

> Scripts de provisioning créés. Exécution requiert validation humaine.

- [x] **3.1-3.8** Scripts `provision-env.sh`, `provision-env-dev.sh`, `migrate-apps-to-env.sh`

### Phase 4 — Premier env-prod sur Medion ✅ (scripts prêts)

> Scripts de provisioning créés. Exécution requiert validation humaine.

- [x] **4.1-4.8** Script `provision-env-prod.sh`

### Phase 5 — Pipelines ✅ (complet)

> Objectif : déploiement automatisé dev → prod avec DB migration.

- [x] **5.1** PipelineStore (persistence JSON, CRUD runs + definitions)
- [x] **5.2** PipelineRunner (exécution séquentielle, progress tracking, timeout)
- [x] **5.3** Migration differ (diff schemas hr-db, génération DDL)
- [x] **5.4** Rollback automatique (restore binaire + DB snapshot)
- [x] **5.5** Intégrer dans le Maker Portal (UI) — API connectée, mock data supprimé

### Phase 6 — Maker Portal ✅ (complet)

> Scaffold complet dans `web-make/`. Prêt pour `pnpm install && pnpm dev`.

- [x] **6.1** Scaffold Maker Portal (`web-make/`) — Vite + React 19 + Tailwind v4
- [x] **6.2** Pages : Dashboard, AppDetail, Pipelines, Environments, EnvironmentDetail
- [x] **6.3** Composants : Layout, EnvSwitcher, AppCard, StatusBadge, PipelineRow
- [x] **6.4** Types TypeScript + API stub avec mock data
- [x] **6.5** Backend API (routes hr-api: apps, monitoring, control, logs, DB)
- [x] **6.6** Model-driven-app (DB explorer page, schema viewer, data preview)
- [x] **6.7** Makefile targets: web-make, deploy-web-make

### Phase 7 — Polish et multi-host ✅ (complet)

> Objectif : finitions et préparation du futur.

- [x] **7.1** Mise à jour CLAUDE.md (nouvelle architecture, commandes env-agent)
- [x] **7.2** Mise à jour du plan avec états complétés
- [x] **7.3** Makefile targets (env-agent, provision-env-dev, provision-env-prod)
- [x] **7.4** Secrets vault par env — SecretsManager + 4 MCP tools (secrets.*)
- [x] **7.5** Monitoring cross-env — monitoring.envs MCP tool + API route
- [x] **7.6** Abstraction multi-host — list_by_host, host_capacity, HostMetrics protocol

---

## 14. Contraintes et points d'attention

### Préservation du code

> ⚠️ **CRITIQUE** : ne pas perdre le code dev dans `/ssd_pool/apps/`.

- Phase 3 : **copie** des sources, jamais de déplacement destructif
- Les anciens containers prod restent en standby jusqu'à validation complète
- Backup avant chaque migration de DB

### Medion first

Tout sur Medion pour simplifier. L'interface env-agent est abstraite
(WebSocket + MCP HTTP) pour permettre le multi-host plus tard sans refacto.

### Compatibilité descendante

Pendant la migration :
- Les anciens containers continuent de tourner
- hr-edge supporte les deux modes (ancien routing + env-aware)
- La bascule est progressive, app par app si nécessaire

### Ce qui ne change PAS

- hr-edge reste le point d'entrée unique (443)
- hr-netcore reste le DNS/DHCP global
- hr-auth reste le SSO central
- hr-git reste le gestionnaire de repos
- Le Hub (10.0.0.20:3500) reste le système de todos/jobs

### Dépendances du workspace Cargo

```
hr-db (librairie)
  └── utilisé par env-agent (dans le container)

hr-environment (nouveau crate orchestrator)
  └── utilisé par hr-orchestrator

hr-pipeline (nouveau crate orchestrator)
  └── utilisé par hr-orchestrator

env-agent (nouveau binaire, remplace hr-agent)
  └── dépend de hr-db, hr-common, hr-ipc
```
