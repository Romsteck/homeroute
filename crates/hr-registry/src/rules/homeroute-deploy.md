# Déploiement vers Production

Ce workspace est un **environnement de build** lié à un conteneur de production.
Vous développez et buildez ici, puis déployez sur la production via les outils MCP `deploy`.

## Architecture
- **Ici (DEV)**: code source, build tools, code-server IDE
- **Production (PROD)**: binaire déployé + assets statiques + base de données
- Pas d'endpoint public pour ce conteneur de dev — il sert uniquement à builder
- L'IDE code-server est accessible via `code.{slug}.{domaine}`

## Règles
- **JAMAIS déployer en production sauf si l'utilisateur l'a explicitement demandé**
- TOUJOURS builder le binaire AVANT de déployer (`cargo build --release`)
- TOUJOURS utiliser l'outil `deploy` du serveur MCP — JAMAIS copier manuellement
- Le binaire est déployé à `/opt/app/app` sur le conteneur de production
- Le service systemd `app.service` est créé/redémarré automatiquement

## Outils disponibles
- `deploy` — Déployer un binaire compilé vers la production (nécessite `binary_path`)
- `prod_status` — Vérifier le statut du service en production
- `prod_logs` — Consulter les logs du service en production
- `prod_exec` — Exécuter une commande shell sur la production
- `prod_push` — Copier un fichier ou dossier vers la production

## Procédure de déploiement
1. Builder le binaire: `cargo build --release`
2. Builder le frontend si applicable (dans le workspace)
3. Pousser les assets statiques **AVANT** le binaire: `prod_push` de `frontend/dist` vers `/opt/app/frontend/dist`
4. Pousser la base de données si le schéma a changé: `prod_push` avec le dossier `.dataverse/`
5. Déployer le binaire avec l'outil MCP `deploy` (restart automatique du service)
6. Vérifier avec `prod_status` et `prod_logs`

### Chemin frontend sur PROD
- Le backend lit les assets statiques depuis **`/opt/app/frontend/dist`** (via `WorkingDirectory=/opt/app`)
- TOUJOURS pousser vers `/opt/app/frontend/dist` — JAMAIS vers `/opt/app/dist`
- Le `index.html` est chargé au démarrage du service — un `prod_push` seul ne suffit pas, il faut redémarrer le service après (d'où l'intérêt de pusher les assets AVANT le `deploy`)

## Applications mobiles (Flutter/Expo)
Pour les apps avec frontend mobile:
- Le backend tourne localement dans ce conteneur pendant le dev
- Le mobile se connecte via l'IP locale du conteneur (ex: `http://10.0.x.y:3000`)
- Pour la prod, déployer le backend sur PROD comme ci-dessus
