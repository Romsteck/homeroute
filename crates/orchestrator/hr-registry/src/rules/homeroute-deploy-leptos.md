# Deploiement vers Production — Stack Leptos (Rust SSR + WASM)

Ce workspace est un **environnement de build** lie a un conteneur de production.
Vous developpez et buildez ici, puis deployez sur la production via les outils MCP `deploy`.

## Environnement
- **App** : {{slug}}
- **IDE** : https://code.{{slug}}.{{domain}}
- **DEV** : https://dev.{{slug}}.{{domain}}
- **PROD** : https://{{slug}}.{{domain}}

## Architecture
- **Ici (DEV)**: code source, build tools, code-server IDE, `cargo-leptos watch` (port 3000)
- **Production (PROD)**: binaire compile + assets WASM/JS/CSS dans `site/`
- Le binaire sert le SSR et les assets statiques depuis `/opt/app/site/`
- En DEV : `cargo-leptos watch` (hot-reload SSR + WASM)
- En PROD : `/opt/app/app` (binaire autonome)

## Regles
- **JAMAIS deployer en production sauf si l'utilisateur l'a explicitement demande**
- TOUJOURS utiliser l'outil `deploy_app` pour un deploiement complet en une commande
- Le binaire est deploye a `/opt/app/app` sur le conteneur de production
- Les assets (WASM, JS, CSS) sont dans `/opt/app/site/`
- Le service systemd `app.service` est cree/redemarre automatiquement
- Pas de Node.js en production — tout est Rust + WASM

## Outils disponibles
- `deploy_app` — **Deploiement complet** en une commande (cargo-leptos build + migration + push site/ + deploy binary + service + health check)
- `prod_status` — Verifier le statut du service en production
- `prod_logs` — Consulter les logs du service en production
- `prod_exec` — Executer une commande shell sur la production
- `prod_push` — Copier un fichier ou dossier vers la production
- `prod_schema` — Afficher le schema de la base de donnees PROD (lecture seule)
- `schema_diff` — Comparer le schema DEV vs PROD
- `migrate_schema` — Appliquer les modifications de schema sur PROD (sans toucher aux donnees)
- `dev_health_check` — Verifier l'etat de tous les services DEV (code-server, cargo-leptos-dev)
- `dev_test_endpoint` — Tester un endpoint HTTP local

## Procedure de deploiement
1. Utiliser `deploy_app` — il orchestre tout automatiquement :
   - `cargo-leptos build --release` dans /root/workspace
   - Migration de schema (si .dataverse/ existe)
   - Push du dossier `target/site/` vers `/opt/app/site/` sur PROD
   - Deploy du binaire depuis `target/server/release/`
   - Creation/mise a jour de `app.service` avec `LEPTOS_SITE_ADDR=0.0.0.0:3000`
   - Restart du service + health check
2. Verifier avec `prod_status` et `prod_logs`

### Artifacts deployes vers PROD
- **Binaire** : `/opt/app/app` (depuis `target/server/release/`)
- **Assets** : `/opt/app/site/` (WASM, JS, CSS depuis `target/site/`)
- **PAS** de `node_modules/`, pas de Node.js

## Variables d'environnement production
- `LEPTOS_SITE_ADDR=0.0.0.0:3000` — adresse d'ecoute du serveur
- `LEPTOS_SITE_ROOT=site` — dossier des assets relatif au WorkingDirectory
- `RUST_LOG=info` — niveau de log

## Filesystem PROD
```
/opt/app/
  app                   # Binaire Leptos (SSR)
  site/                 # Assets statiques (WASM, JS, CSS)
  .dataverse/app.db     # Base de donnees SQLite (si applicable)
  .env                  # Variables d'environnement serveur
```

> **Note** : Pour le workflow de developpement (services systemd, hot-reload, outils de verification), voir `homeroute-dev-leptos.md`.
