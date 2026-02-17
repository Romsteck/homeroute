# Deploiement vers Production

Ce workspace est un **environnement de build** lie a un conteneur de production.
Vous developpez et buildez ici, puis deployez sur la production via les outils MCP `deploy`.

## Environnement
- **App** : {{slug}}
- **IDE** : https://code.{{slug}}.{{domain}}
- **DEV** : https://dev.{{slug}}.{{domain}}
- **PROD** : https://{{slug}}.{{domain}}

## Architecture
- **Ici (DEV)**: code source, build tools, code-server IDE
- **Production (PROD)**: binaire deploye + assets statiques + base de donnees
- Pas d'endpoint public pour ce conteneur de dev — il sert uniquement a builder
- L'IDE code-server est accessible via `code.{{slug}}.{{domain}}`

## Regles
- **JAMAIS deployer en production sauf si l'utilisateur l'a explicitement demande**
- TOUJOURS utiliser l'outil `deploy_app` pour un deploiement complet en une commande
- Le binaire est deploye a `/opt/app/app` sur le conteneur de production
- Le service systemd `app.service` est cree/redemarre automatiquement

## Outils disponibles
- `deploy_app` — **Deploiement complet** en une commande (build + migration + push + deploy + health check)
- `deploy` — Deployer un binaire compile vers la production (necessite `binary_path`)
- `prod_status` — Verifier le statut du service en production
- `prod_logs` — Consulter les logs du service en production
- `prod_exec` — Executer une commande shell sur la production
- `prod_push` — Copier un fichier ou dossier vers la production
- `prod_schema` — Afficher le schema de la base de donnees PROD (lecture seule)
- `schema_diff` — Comparer le schema DEV vs PROD
- `migrate_schema` — Appliquer les modifications de schema sur PROD (sans toucher aux donnees)
- `dev_health_check` — Verifier l'etat de tous les services DEV
- `dev_test_endpoint` — Tester un endpoint HTTP local

## Procedure de deploiement
1. Utiliser `deploy_app` — il orchestre tout automatiquement :
   - Build release du binaire
   - Build frontend (si frontend/ existe)
   - Migration de schema (si .dataverse/ existe)
   - Push des assets frontend
   - Deploy du binaire
   - Health check
2. Verifier avec `prod_status` et `prod_logs`

### Chemin frontend sur PROD
- Le backend lit les assets statiques depuis **`/opt/app/frontend/dist`**
- `deploy_app` gere automatiquement le push vers le bon chemin

## Applications mobiles (Flutter/Expo)
Pour les apps avec frontend mobile:
- Le backend tourne localement dans ce conteneur pendant le dev
- Le mobile se connecte via l'IP locale du conteneur
- Pour la prod, deployer le backend sur PROD comme ci-dessus

> **Note** : Pour le workflow de developpement (services systemd, hot-reload, outils de verification), voir `homeroute-dev.md`.
