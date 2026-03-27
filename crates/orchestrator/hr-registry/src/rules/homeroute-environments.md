# Environments

HomeRoute gere des environnements isoles pour deployer et tester les applications.

## Concepts

Chaque environnement est un conteneur nspawn independant avec son propre reseau, ses apps et ses bases de donnees.

Trois types d'environnement :
- **Development** — Acces complet, modifications libres, iteration rapide
- **Acceptance** — Pre-production, validation avant mise en prod
- **Production** — Verrouille, deploiement uniquement via pipeline

Chaque environnement dispose d'un env-agent qui gere les apps, les bases de donnees et le studio.

## Outils MCP — Environnements

- `envs.list` — Lister tous les environnements avec leur statut
- `envs.get` — Details d'un environnement par slug (apps, sante, config)
- `envs.create` — Creer un nouvel environnement (type requis)
- `envs.start` / `envs.stop` — Demarrer ou arreter un environnement
- `envs.destroy` — Supprimer un environnement (confirm=true)

## Outils MCP — Pipelines

- `pipeline.promote` — Promouvoir une app d'un env vers un autre (ex: dev vers prod)
- `pipeline.status` — Verifier le statut d'un pipeline en cours
- `pipeline.history` — Lister les executions recentes de pipelines
- `pipeline.cancel` — Annuler un pipeline en cours d'execution

## Convention d'URLs

| Pattern | Signification |
|---------|---------------|
| `{app}.{env}.mynetwk.biz` | App dans un environnement |
| `studio.{env}.mynetwk.biz` | Studio de l'environnement |
| `{app}.mynetwk.biz` | Alias vers la production |

## Regles

- TOUJOURS promouvoir via `pipeline.promote` (jamais deployer directement en prod)
- TOUJOURS verifier le statut avec `pipeline.status` apres une promotion
- TOUJOURS utiliser `envs.get` pour verifier la sante d'un env avant toute operation
- JAMAIS ecrire directement en base de donnees en production (pipeline uniquement)
- JAMAIS supprimer un environnement de production
