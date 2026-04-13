---
name: deploy
description: Smart deploy — analyse les fichiers modifiés et lance le(s) bon(s) make deploy-* automatiquement
disable-model-invocation: true
argument-hint: "[all|edge|orchestrator|netcore|web|prod]"
allowed-tools: Bash(make *) Bash(git diff *) Bash(git status *) Bash(curl *) Bash(ssh *)
---

# Smart Deploy HomeRoute

Tu es le skill de déploiement HomeRoute. Tu analyses les changements et déploies vers la prod.

## Étape 1 — Analyser les changements

Exécute ces commandes pour comprendre ce qui a changé :

```!
echo "=== Staged ==="
git -C /nvme/homeroute diff --cached --name-only 2>/dev/null
echo "=== Unstaged ==="
git -C /nvme/homeroute diff --name-only 2>/dev/null
echo "=== Since last deploy commit ==="
git -C /nvme/homeroute diff HEAD --name-only 2>/dev/null
```

## Étape 2 — Déterminer les targets

Si l'utilisateur a passé un argument explicite (`$ARGUMENTS`), utilise-le directement :
- `all` ou `prod` → `make deploy-prod`
- `edge` → `make deploy-edge`
- `orchestrator` → `make deploy-orchestrator`
- `netcore` → `make deploy-netcore`
- `web` → build web seulement, rsync web/dist/ vers prod

Sinon, déduis les targets depuis les fichiers modifiés :

| Fichiers modifiés | Target Make |
|---|---|
| `crates/edge/**`, `crates/shared/**` utilisé par edge | `make deploy-edge` |
| `crates/orchestrator/**`, `crates/shared/**` utilisé par orchestrator | `make deploy-orchestrator` |
| `crates/netcore/**` | `make deploy-netcore` |
| `crates/api/**`, `crates/shared/**` | `make deploy-prod` (inclut homeroute) |
| `web/**` | `make deploy-prod` (inclut le frontend) |
| Changements dans `crates/shared/` (hr-common, hr-ipc) | Déployer TOUS les binaires qui en dépendent |
| Mélange de plusieurs zones | `make deploy-prod` (full) |

**Règle critique** : si `crates/shared/` est modifié, il faut rebuild tous les binaires qui en dépendent. En cas de doute, `make deploy-prod`.

## Étape 3 — Exécuter

1. Lancer le(s) `make deploy-*` approprié(s) depuis `/nvme/homeroute`
2. Attendre la fin du build + deploy
3. Si le make échoue, afficher l'erreur et s'arrêter

## Étape 4 — Vérifier

1. Health check : `curl -sf http://10.0.0.20:4000/api/health | python3 -m json.tool`
2. Derniers logs : `curl -s 'http://10.0.0.20:4000/api/logs?limit=5' | python3 -m json.tool`
3. Résumer : quels targets déployés, health OK/KO, erreurs éventuelles dans les logs

## Règles

- Travailler depuis `/nvme/homeroute` (le working directory du projet)
- Ne JAMAIS lancer `cargo run` — seulement les targets Make
- Ne JAMAIS lancer `make deploy` (local) — seulement `make deploy-prod` ou les targets individuels
- Si aucun fichier n'a changé et pas d'argument, le signaler et ne rien déployer
