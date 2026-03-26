# Deploiement vers Production — Stack Next.js

Ce workspace est un **environnement de build** lie a un conteneur de production.
Vous developpez et buildez ici, puis deployez sur la production via les outils MCP `deploy`.

## Environnement
- **App** : {{slug}}
- **IDE** : https://code.{{slug}}.{{domain}}
- **DEV** : https://dev.{{slug}}.{{domain}}
- **PROD** : https://{{slug}}.{{domain}}

## Architecture
- **Ici (DEV)**: code source, build tools, code-server IDE, `next dev` (port 3000)
- **Production (PROD)**: build Next.js + custom server (WebSocket + SSR)
- Le custom `server.ts` fait cohabiter Next.js et WebSocket sur le meme port 3000
- En DEV : `next dev` directement (hot-reload)
- En PROD : `node --import tsx server.ts` (Next.js + WebSocket)

## Regles
- **JAMAIS deployer en production sauf si l'utilisateur l'a explicitement demande**
- TOUJOURS utiliser l'outil `deploy_app` pour un deploiement complet en une commande
- Le service systemd `app.service` est cree/redemarre automatiquement
- `tsx` doit etre une dependance normale (pas devDependency) pour fonctionner en PROD

## Outils disponibles
- `deploy_app` — **Deploiement complet** en une commande (build + push artifacts + install deps + service + health check)
- `prod_status` — Verifier le statut du service en production
- `prod_logs` — Consulter les logs du service en production
- `prod_exec` — Executer une commande shell sur la production
- `prod_push` — Copier un fichier ou dossier vers la production
- `prod_schema` — Afficher le schema de la base de donnees PROD (lecture seule)
- `schema_diff` — Comparer le schema DEV vs PROD
- `migrate_schema` — Appliquer les modifications de schema sur PROD (sans toucher aux donnees)
- `dev_health_check` — Verifier l'etat de tous les services DEV (code-server, nextjs-dev)
- `dev_test_endpoint` — Tester un endpoint HTTP local

## Procedure de deploiement
1. Utiliser `deploy_app` — il orchestre tout automatiquement :
   - `npm run build` dans /root/workspace
   - Migration de schema (si .dataverse/ existe)
   - Verification/installation de Node.js sur PROD
   - Push des artifacts (`.next/`, sources, configs) vers /opt/app/
   - `npm ci` sur PROD pour installer les dependances
   - Creation/mise a jour de `app.service` (`node --import tsx server.ts`)
   - Restart du service + health check
2. Verifier avec `prod_status` et `prod_logs`

### Artifacts pousses vers PROD
- `.next/` (build output Next.js)
- `package.json`, `package-lock.json` (dependances)
- `server.ts` (custom server WebSocket)
- `next.config.*`, `tsconfig.json` (config)
- `lib/`, `app/`, `public/` (sources et assets)
- **PAS** de `node_modules/` (installe via `npm ci` sur PROD)

## Variables d'environnement production
- `NODE_ENV=production` et `PORT=3000` sont definis dans app.service
- Les variables `NEXT_PUBLIC_*` doivent etre definies au moment du build (dans le DEV)
- Les variables serveur sont dans `/opt/app/.env` (non versionnees)

## Filesystem PROD
```
/opt/app/
  .next/                # Build output Next.js
  node_modules/         # Dependances production (via npm ci)
  package.json          # Dependances
  server.ts             # Custom server (WebSocket + Next.js)
  next.config.*         # Config Next.js
  lib/                  # Code serveur partage
  app/                  # App Router (pages/routes)
  public/               # Assets statiques
  .dataverse/app.db     # Base de donnees SQLite (si applicable)
  .env                  # Variables d'environnement serveur
```

> **Note** : Pour le workflow de developpement (services systemd, hot-reload, outils de verification), voir `homeroute-dev-nextjs.md`.
