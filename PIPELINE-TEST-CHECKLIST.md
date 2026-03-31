# Pipeline E2E Test Checklist

## Prérequis
- [x] Routes REST config/gates ajoutées dans hr-api
- [x] Déployé (make deploy-prod)
- [x] PipelineConfig sauvegarde OK (PUT /api/pipelines/config → 200)

## Infrastructure
- [x] Env "acc" créé via MCP (IP: 10.0.0.103)
- [x] Env-agent acc connecté au WebSocket orchestrator
- [x] App "www" déclarée dans la config acc (1 app, 1 running)

## Pipeline www : DEV → ACC
- [x] Repo "www" existe dans hr-git (83 commits, branche main)
- [x] Pipeline config sauvegardée (dev → acc → prod, skip test, gate acc→prod)
- [x] Pipeline promote dev → acc déclenché (run 2d981c35)
- [x] Step Build : success (192 MB, 54s)
- [x] Step BackupDb : skipped (pas de DB steps dans ce run)
- [x] Step MigrateDb : skipped
- [x] Step Deploy : success (201 MB déployé en 5s)
- [x] Step HealthCheck : success (auto-passed)
- [x] https://www.acc.mynetwk.biz → 200

## Pipeline www : ACC → PROD
- [x] Pipeline promote acc → prod déclenché (run a9d3c6b7)
- [x] Step Build : success (artifact réutilisé, 0.8s)
- [x] Step Deploy : success (201 MB, 5s)
- [x] Step HealthCheck : success (auto-passed)
- [x] https://www.prod.mynetwk.biz → 200

## UI Maker Portal
- [x] Page Pipelines affiche les runs (in-memory, perdu au restart — TODO: persister dans store)
- [x] Clic sur un run → PipelineDetail avec steps (vérifié avant restart)
- [x] PipelineConfig accessible et sauvegarde OK (routes REST ajoutées, plus de 405)

## Notes
- Les runs sont in-memory dans PipelineEngine — perdus au restart de hr-orchestrator
- TODO futur : le promote() devrait aussi persister dans PipelineStore
- Node.js doit être installé dans les containers cibles (prod n'avait pas node)
- Le package.json des apps Next.js doit avoir `pnpm.onlyBuiltDependencies` pour les modules natifs
- Le health check auto-passe actuellement (pas de health_url injecté) — TODO: injecter l'URL réelle
