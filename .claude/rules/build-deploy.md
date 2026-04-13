# Build & Deploy — Workflow Makefile

Référence rapide pour savoir **quoi lancer selon ce qui a été modifié**. Complète `CLAUDE.md` (qui liste les commandes) en précisant les déclencheurs et les pièges.

## Matrice "Quoi modifier → Quoi lancer"

| Modification | Build préalable | Deploy |
|---|---|---|
| `crates/api/`, `crates/api/homeroute` | — | `make deploy-prod` |
| `crates/edge/*` (proxy, acme, auth, tunnel) | — | `make deploy-edge` |
| `crates/orchestrator/*` (apps, git, db, mcp) | — | `make deploy-orchestrator` |
| `crates/netcore/*` (dns, dhcp, adblock, ipv6) | — | `make deploy-netcore` |
| `crates/shared/*` (hr-common, hr-ipc) | — | tous les services concernés |
| `web/src/**` (React/Vite) | **`make web`** | `make deploy-prod` |
| `crates/agents/hr-host-agent/` | `make host-agent` | `make host-agent-prod` (sur CloudMaster) |
| `store_flutter/` | — | `make store` (build + copie APK + version.json) |
| `scripts/setup-studio.sh` | — | `make deploy-studio` |

## ⚠️ Piège frontend (CRITIQUE)

`make deploy-prod` **ne lance pas** `npm run build` — il rsync `web/dist/` tel quel. Toute modif sous `web/src/` exige :

```bash
make web && make deploy-prod
```

Oublier `make web` pousse l'ancien bundle en prod. Après deploy frontend : vérifier visuellement dans le navigateur (SW cache-first peut masquer le résultat).

## hr-host-agent (tourne sur CloudMaster)

- `make host-agent` : incrémente automatiquement le patch dans `crates/agents/hr-host-agent/Cargo.toml` puis build release
- `make host-agent-prod` : **exécuté sur CloudMaster lui-même** — stop systemd → copie `/usr/local/bin/hr-host-agent` → start → check actif
- Pas de déploiement vers 10.0.0.254 — l'agent vit sur la machine de dev
- Ne pas éditer la version dans `Cargo.toml` à la main : le bump est automatique

## App Store Flutter

- `make store` : auto-bump `versionCode` (`store_flutter/android/app/build.gradle.kts`) et `versionName` (`pubspec.yaml`), build APK release, copie vers `/opt/homeroute/data/store/client/homeroute-store.apk` + `version.json`
- Pré-requis : `export PATH=/ssd_pool/flutter/bin:$PATH`
- Servi via `/api/store/client/apk`
- Build **toujours sur CloudMaster**, jamais sur le routeur

## Studio (code-server)

- `make deploy-studio` : rsync `scripts/setup-studio.sh` sur prod et l'exécute en sudo (idempotent)
- À relancer uniquement après modification du script ou première install

## Cibles génériques

| Cible | Usage |
|---|---|
| `make all` | Build complet (netcore + edge + orchestrator + server + web), sans deploy |
| `make test` | `cargo test` dans `crates/` |
| `make clean` | Nettoie `crates/target` et `web/dist` |

## Safeguards du Makefile

- `check-not-prod` bloque `make deploy` (local) si homeroute ne tourne pas — évite de builder sur le routeur
- `check-prod` vérifie que 10.0.0.20 est joignable avant chaque `make deploy-*`
- Deploy **toujours depuis CloudMaster**, jamais depuis le routeur

## Après chaque deploy

```bash
curl -s http://10.0.0.254:4000/api/health | jq
```

Puis test fonctionnel des endpoints touchés (cf. `testing.md`).
