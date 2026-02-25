# Deploiement Production — Stack Next.js

## Environnement

| Variable | Valeur |
|----------|--------|
| Slug application | `{{slug}}` |
| URL Production | `https://{{slug}}.{{domain}}` |

## Architecture DEV → PROD

- **Container DEV** : developpement avec `npm run dev` (hot-reload, port 3000)
- **Container PROD** : application buildee avec `npm run build && npm start`
- Le binaire de l'agent HomeRoute (`/usr/local/bin/hr-agent`) gere la connexion entre les deux

## Regles de deploiement

1. **Ne jamais** deployer sans demande explicite de l'utilisateur
2. **Toujours** utiliser l'outil `deploy_app` pour deployer
3. **Verifier** que les tests passent avant de deployer
4. **Confirmer** le deploiement avec l'utilisateur

## Outils disponibles

| Outil | Description |
|-------|-------------|
| `deploy_app` | Deploie l'application en production (build + restart) |
| `deploy` | Deploiement bas niveau |
| `prod_status` | Statut du service en production |
| `prod_logs` | Logs de production en temps reel |
| `prod_exec` | Executer une commande en production |
| `prod_push` | Pousser des fichiers en production |
| `schema_diff` | Comparer les schemas DEV/PROD |
| `migrate_schema` | Appliquer une migration en production |

## Procedure de deploiement

L'outil `deploy_app` effectue automatiquement :
1. `npm run build` dans le container DEV
2. Copie des artifacts vers le container PROD
3. Restart du service `app.service` en production

Le service production `app.service` execute `npm start` (port 3000).

## Variables d'environnement production

Les variables `NEXT_PUBLIC_*` doivent etre definies au moment du build.
Les variables serveur sont dans `/opt/app/.env` (non versionnees).

## Verification post-deploiement

```bash
# Statut du service
prod_status

# Logs pour verifier le demarrage
prod_logs

# Test de l'URL publique
curl -I https://{{slug}}.{{domain}}
```
