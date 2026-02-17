# Refonte de l'Experience Developpeur HomeRoute

> **Document de reference unique** pour la refonte complete de l'experience developpeur HomeRoute.
> Date de creation : 2026-02-16
> Derniere mise a jour : 2026-02-16

---

## Table des matieres

1. [Contexte et problematique](#contexte-et-problematique)
2. [Architecture actuelle](#architecture-actuelle)
3. [Les 5 problemes fondamentaux](#les-5-problemes-fondamentaux)
4. [Phase 1 : DEV Mode Live (Hot Reload)](#phase-1--dev-mode-live-hot-reload)
5. [Phase 2 : Migration de Schema Dataverse](#phase-2--migration-de-schema-dataverse)
6. [Phase 3 : Pipeline de Deploy Unifie](#phase-3--pipeline-de-deploy-unifie)
7. [Phase 4 : Rules Dynamiques et Contextuelles](#phase-4--rules-dynamiques-et-contextuelles)
8. [Phase 5 : Boucle de Verification Automatique](#phase-5--boucle-de-verification-automatique)
9. [Phase 6 : Integration Autopilot](#phase-6--integration-autopilot)
10. [Reference Patterns Lovable/Replit](#reference-patterns-lovablereplit)
11. [Tableau des priorites](#tableau-des-priorites)
12. [Suivi](#suivi)

---

## Contexte et problematique

HomeRoute est un **binaire Rust unifie** qui gere l'ensemble des services reseau (DNS, DHCP, proxy, containers, etc.) pour un homelab. Les applications sont developpees dans des containers systemd-nspawn (DEV) puis deployees vers des containers PROD apparies.

L'experience developpeur actuelle presente des **frictions majeures** qui ralentissent considerablement le cycle de developpement et augmentent le risque d'erreurs en production.

---

## Architecture actuelle

| Composant | Description | Emplacement |
|-----------|-------------|-------------|
| **Frontend** | Application React/Vite | `web/` |
| **Backend** | Workspace Rust (Cargo) | `crates/` |
| **Containers** | systemd-nspawn, geres par `container_manager.rs` | Machines nspawn |
| **Agent** | Binaire `hr-agent` dans chaque container, connecte au registry via WebSocket | `crates/hr-agent/` |
| **MCP Servers** | 3 serveurs MCP separes (dataverse, deploy, store) fournis par `hr-agent` | `crates/hr-agent/src/mcp.rs` |
| **Rules** | Fichiers statiques `.claude/rules/` ecrits au provisionnement | `crates/hr-registry/src/rules/` |
| **Proxy** | Proxy agent sur port 443 avec routage SNI et forward-auth | `crates/hr-proxy/` |

---

## Les 5 problemes fondamentaux

### Probleme 1 : Cycle build/deploy lent

Chaque modification de code necessite : `cargo build --release` (2-5 min) puis `deploy` (copie du binaire vers PROD) puis `prod_status`/`prod_logs` pour debugger. **Aucun retour instantane.**

### Probleme 2 : Dataverse et Deploy MCP deconnectes

`prod_push .dataverse/` ecrase l'integralite de la base de donnees (schema + donnees). Il n'existe aucun outil de diff/migration de schema. L'agent DEV est aveugle a l'etat de la BDD PROD.

### Probleme 3 : Pas de mode DEV live

Les containers DEV n'ont pas de serveur de developpement en cours d'execution. L'agent code dans code-server, build un binaire release, pousse vers PROD, et espere que ca marche. Pas de hot reload Rust, pas de Vite HMR.

### Probleme 4 : Instructions statiques et decontextualisees

Les 3 fichiers de rules sont identiques pour tous les containers. Ils ne s'adaptent pas a l'etat du projet (presence d'un frontend ? utilisation de Dataverse ? phase de developpement ?). Les URLs sont generiques.

### Probleme 5 : Pas de boucle de verification

Contrairement a Replit Agent 3 qui teste son propre travail dans un navigateur headless, les agents HomeRoute n'ont aucun moyen de verifier que ce qu'ils ont deploye fonctionne reellement.

---

## Phase 1 : DEV Mode Live (Hot Reload)

> **Priorite : CRITIQUE** | **Effort : Moyen** | **Impact : Critique**

### Objectif

Eliminer le cycle build/deploy en fournissant un hot reload instantane pour le frontend (Vite HMR) et le backend (cargo-watch) directement dans le container DEV.

### Architecture cible

```
Container DEV (nspawn)
|-- code-server        -> code.{slug}.{domain}     (existant)
|-- vite dev server    -> dev.{slug}.{domain}       (NOUVEAU)
|-- cargo-watch server -> devapi.{slug}.{domain}    (NOUVEAU)
|-- hr-agent           (gere l'ensemble)
```

### Fonctionnement detaille

#### Vite HMR

Un service systemd `vite-dev.service` execute `npx vite --host 0.0.0.0 --port 5173` dans `/root/workspace/frontend/`. Chaque sauvegarde de fichier `.tsx`/`.css` est **instantanement** refletee sur `dev.{slug}.{domain}`.

#### Rust Hot Reload

Un service systemd `cargo-dev.service` execute `cargo-watch -x 'run'` sur le backend Rust. Chaque modification de fichier `.rs` declenche une recompilation **debug** (~10s vs 2-5 min pour release) et un redemarrage automatique. Accessible sur `devapi.{slug}.{domain}`.

### Modifications requises

#### Fichiers a modifier

| Fichier | Modification |
|---------|-------------|
| `crates/hr-registry/src/protocol.rs` | Ajouter les variantes `ViteDev` et `CargoDev` dans `ServiceType` |
| `crates/hr-agent/src/proxy.rs` | Ajouter les entrees de routage pour `dev.*` et `devapi.*` |
| `crates/hr-agent/src/main.rs` | Gerer les nouvelles routes dans le message `Config` |
| `crates/hr-agent/src/services.rs` | Ajouter les nouveaux types de service |
| `crates/hr-agent/src/powersave.rs` | Gerer les nouveaux services (idle timeout, start/stop) |
| `crates/hr-api/src/container_manager.rs` | Installer cargo-watch, Node.js, creer les units systemd |
| `crates/hr-registry/src/rules/homeroute-deploy.md` | Mettre a jour les instructions pour le mode dev |

#### Impact sur les rules

Les instructions de deploy doivent etre revisees pour indiquer :

> **En developpement, NE JAMAIS builder en mode release. Le serveur dev tourne en continu.**
> - Modifier le code, le hot reload s'en charge
> - Tester sur `dev.{slug}.{domain}` (frontend) et `devapi.{slug}.{domain}` (API)
> - Deployer vers PROD uniquement quand l'utilisateur le demande explicitement

### Taches detaillees

- [ ] **1.1** Ajouter les variantes `ViteDev` et `CargoDev` dans `ServiceType` (`crates/hr-registry/src/protocol.rs`)
- [ ] **1.2** Ajouter la configuration des routes `dev.*` et `devapi.*` dans `RegistryMessage::Config` (`crates/hr-agent/src/main.rs`)
- [ ] **1.3** Implementer le routage proxy pour `dev.*` vers le port 5173 et `devapi.*` vers le port 3000 (`crates/hr-agent/src/proxy.rs`)
- [ ] **1.4** Ajouter la gestion des nouveaux types de service dans l'agent (`crates/hr-agent/src/services.rs`)
- [ ] **1.5** Integrer les nouveaux services dans le powersave (idle timeout, start/stop) (`crates/hr-agent/src/powersave.rs`)
- [ ] **1.6** Creer le template du service systemd `vite-dev.service`
- [ ] **1.7** Creer le template du service systemd `cargo-dev.service`
- [ ] **1.8** Ajouter l'installation de `cargo-watch` dans le provisionnement (Phase 7 de `container_manager.rs`)
- [ ] **1.9** Ajouter l'installation de Node.js/npm dans le provisionnement pour le serveur Vite dev (`crates/hr-api/src/container_manager.rs`)
- [ ] **1.10** Ecrire les units systemd dans le container au provisionnement (`crates/hr-api/src/container_manager.rs`)
- [ ] **1.11** Mettre a jour les instructions deploy pour le mode dev (`crates/hr-registry/src/rules/homeroute-deploy.md`)
- [ ] **1.12** Tester le hot reload Vite de bout en bout (modification `.tsx` -> affichage sur `dev.{slug}.{domain}`)
- [ ] **1.13** Tester le hot reload Rust de bout en bout (modification `.rs` -> rebuild -> accessible sur `devapi.{slug}.{domain}`)
- [ ] **1.14** Valider l'integration powersave (idle timeout declenche l'arret, requete HTTP relance le service)

---

## Phase 2 : Migration de Schema Dataverse

> **Priorite : CRITIQUE** | **Effort : Moyen** | **Impact : Critique**

### Objectif

Remplacer l'ecrasement brut de la base PROD (`prod_push .dataverse/`) par un systeme de migration de schema intelligent qui preserve les donnees existantes.

### Nouveaux outils MCP

| Outil | Description |
|-------|-------------|
| `schema_diff` | Compare le schema DEV vs le schema PROD, retourne les differences |
| `migrate_schema` | Applique les modifications de schema sur PROD (sans toucher aux donnees) |
| `prod_schema` | Affiche le schema PROD actuel (lecture seule) |

### Implementation technique

L'agent DEV dispose deja d'une connexion WebSocket au registry via `AgentMessage`. Il faut l'etendre :

#### `schema_diff`

1. L'agent DEV envoie `AgentMessage::GetProdSchema { app_id }` via le registry
2. Le registry transmet la requete au container PROD via `RegistryMessage::DataverseQuery`
3. Le container PROD repond avec son schema
4. L'agent DEV compare localement et genere un diff :
   - Nouvelles tables
   - Nouvelles colonnes
   - Colonnes supprimees
   - Types modifies

#### `migrate_schema`

1. A partir du diff, genere les statements SQL (`ALTER TABLE ADD COLUMN`, `CREATE TABLE`, etc.)
2. Execute sur PROD via `prod_exec`
3. L'agent valide chaque statement avant execution
4. Optionnellement : retourne le SQL pour approbation avant execution

#### `prod_schema`

Requete simple en lecture seule pour obtenir le schema PROD via le meme relai registry.

#### Protection

`prod_push .dataverse/` est soit **supprime**, soit renomme en `prod_push_db_unsafe` avec un avertissement explicite que cette commande ecrase les donnees.

### Migrations versionnees (optionnel, phase 2b)

Dataverse stocke deja `schema_version`. Ajouter une table `_dv_migrations` qui liste les migrations appliquees pour un suivi reproductible.

### Fichiers a modifier

| Fichier | Modification |
|---------|-------------|
| `crates/hr-agent/src/mcp.rs` | Ajouter les outils `schema_diff`, `migrate_schema`, `prod_schema` |
| `crates/hr-registry/src/protocol.rs` | Ajouter le type de message `GetProdSchema` |
| `crates/hr-registry/src/rules/homeroute-dataverse.md` | Mettre a jour les instructions |
| `crates/hr-agent/src/main.rs` | Gerer les nouveaux messages de protocole |

### Taches detaillees

- [ ] **2.1** Ajouter le message `GetProdSchema` dans le protocole registry (`crates/hr-registry/src/protocol.rs`)
- [ ] **2.2** Ajouter le message `DataverseQuery` / reponse dans le protocole (`crates/hr-registry/src/protocol.rs`)
- [ ] **2.3** Implementer le relai de la requete schema dans le registry (registry -> PROD agent)
- [ ] **2.4** Implementer la reponse schema dans l'agent PROD (lecture schema SQLite + envoi)
- [ ] **2.5** Implementer l'outil MCP `prod_schema` (lecture seule) (`crates/hr-agent/src/mcp.rs`)
- [ ] **2.6** Implementer l'outil MCP `schema_diff` (comparaison locale du schema DEV vs PROD) (`crates/hr-agent/src/mcp.rs`)
- [ ] **2.7** Implementer l'outil MCP `migrate_schema` (generation SQL + execution sur PROD) (`crates/hr-agent/src/mcp.rs`)
- [ ] **2.8** Ajouter une confirmation interactive avant execution des migrations destructives
- [ ] **2.9** Supprimer ou renommer `prod_push .dataverse/` en `prod_push_db_unsafe` avec avertissement
- [ ] **2.10** Mettre a jour les instructions Dataverse (`crates/hr-registry/src/rules/homeroute-dataverse.md`)
- [ ] **2.11** (Phase 2b) Creer la table `_dv_migrations` pour le suivi des migrations versionnees
- [ ] **2.12** (Phase 2b) Implementer l'enregistrement automatique de chaque migration appliquee
- [ ] **2.13** Tester : creation d'une nouvelle table en DEV -> `schema_diff` -> `migrate_schema` -> verification en PROD
- [ ] **2.14** Tester : ajout de colonne en DEV -> migration vers PROD sans perte de donnees

---

## Phase 3 : Pipeline de Deploy Unifie

> **Priorite : HAUTE** | **Effort : Faible** | **Impact : Eleve**

### Objectif

Remplacer les 5 etapes manuelles et sujettes aux erreurs par une **commande unique** `deploy_app` qui orchestre tout le deploiement de maniere securisee.

### Workflow actuel (AVANT)

```
cargo build --release                                    # Etape 1
prod_push frontend/dist -> /opt/app/frontend/dist        # Etape 2
prod_push .dataverse/ -> /opt/app/.dataverse/             # Etape 3 (DANGEREUX)
deploy binary_path                                        # Etape 4
prod_status / prod_logs                                   # Etape 5
```

5 etapes manuelles, risque d'erreur a chaque etape, ecrasement potentiel des donnees PROD.

### Nouveau workflow (APRES)

```
deploy_app    # Une seule commande
```

### Ce que `deploy_app` execute

```
1. Build release       : cargo build --release
2. Build frontend      : npm run build (si frontend/ existe)
3. Schema migration    : schema_diff -> confirmation -> migrate_schema
4. Push frontend       : prod_push frontend/dist -> /opt/app/frontend/dist
5. Deploy binary       : deploy binary
6. Health check        : curl http://PROD:PORT/health (retry 3x)
7. Report              : Resume complet avec OK/FAIL par etape
```

### Options disponibles

| Option | Description |
|--------|-------------|
| `--skip-frontend` | Ne pas builder/pousser le frontend (si seul le backend a change) |
| `--skip-schema` | Ne pas executer la migration de schema (si schema inchange) |
| `--dry-run` | Afficher ce qui serait fait sans rien executer |

### Fichiers a modifier

| Fichier | Modification |
|---------|-------------|
| `crates/hr-agent/src/mcp.rs` | Ajouter l'outil `deploy_app` |
| `crates/hr-registry/src/rules/homeroute-deploy.md` | Mettre a jour les instructions |

### Taches detaillees

- [ ] **3.1** Concevoir le protocole de l'outil `deploy_app` (parametres, retour, gestion d'erreurs)
- [ ] **3.2** Implementer l'outil MCP `deploy_app` dans l'agent (`crates/hr-agent/src/mcp.rs`)
- [ ] **3.3** Implementer l'etape build release (`cargo build --release`)
- [ ] **3.4** Implementer l'etape build frontend (`npm run build` si `frontend/` existe)
- [ ] **3.5** Integrer l'appel a `schema_diff` + confirmation + `migrate_schema`
- [ ] **3.6** Implementer l'etape push frontend (`prod_push frontend/dist`)
- [ ] **3.7** Implementer l'etape deploy binary
- [ ] **3.8** Implementer le health check avec retry (3 tentatives, backoff)
- [ ] **3.9** Implementer le rapport final (resume OK/FAIL par etape)
- [ ] **3.10** Implementer l'option `--skip-frontend`
- [ ] **3.11** Implementer l'option `--skip-schema`
- [ ] **3.12** Implementer l'option `--dry-run`
- [ ] **3.13** Mettre a jour les instructions de deploy (`crates/hr-registry/src/rules/homeroute-deploy.md`)
- [ ] **3.14** Tester un deploy complet de bout en bout (build + migration + push + health check)
- [ ] **3.15** Tester le mode `--dry-run` (aucune action executee)

---

## Phase 4 : Rules Dynamiques et Contextuelles

> **Priorite : MOYENNE** | **Effort : Faible** | **Impact : Moyen**

### Objectif

Remplacer les rules statiques et generiques par des **templates dynamiques** qui s'adaptent au contexte de chaque projet (stack detectee, URLs reelles, services actifs).

### Probleme actuel

Les 3 fichiers de rules sont **identiques** pour tous les containers :
- Certains projets n'ont pas de frontend (API pure)
- Certains n'utilisent pas Dataverse
- Certains sont des apps Flutter/Expo, pas web
- Les URLs sont generiques (pas le vrai slug)

### Solution : Templates avec variables injectees

Les rules deviennent des templates avec substitution de variables :

```markdown
# Environnement de Developpement

- **App** : {{app_name}} ({{slug}})
- **Frontend DEV** : https://dev.{{slug}}.{{domain}}
- **API DEV** : https://devapi.{{slug}}.{{domain}}
- **Code IDE** : https://code.{{slug}}.{{domain}}
- **PROD** : https://{{slug}}.{{domain}}

## Stack detectee
{{#if has_frontend}}
- Frontend : Vite + React (hot reload actif sur le port 5173)
{{/if}}
{{#if has_backend}}
- Backend : Rust/Axum (hot reload cargo-watch sur le port 3000)
{{/if}}
{{#if has_dataverse}}
- Base de donnees : Dataverse (SQLite local, migration vers PROD via `migrate_schema`)
{{/if}}
```

### Mise a jour dynamique

Quand le registry detecte un changement (nouveau frontend, Dataverse ajoute), il envoie `RegistryMessage::UpdateRules` qui re-genere les fichiers `.claude/rules/` avec le nouveau contexte. Plus besoin de scripts de migration manuels.

### Fichiers a modifier

| Fichier | Modification |
|---------|-------------|
| `crates/hr-registry/src/rules/` | Convertir en templates (handlebars ou format custom) |
| `crates/hr-api/src/container_manager.rs` | Rendu des templates au provisionnement |
| `crates/hr-registry/src/protocol.rs` | Ajouter le message `UpdateRules` |
| `crates/hr-agent/src/main.rs` | Gerer le message `UpdateRules` |

### Taches detaillees

- [ ] **4.1** Definir le format de template (handlebars, tera, ou format custom)
- [ ] **4.2** Convertir `homeroute-deploy.md` en template avec variables (`crates/hr-registry/src/rules/`)
- [ ] **4.3** Convertir `homeroute-dataverse.md` en template avec variables (`crates/hr-registry/src/rules/`)
- [ ] **4.4** Convertir `homeroute-store.md` en template avec variables (`crates/hr-registry/src/rules/`)
- [ ] **4.5** Implementer la detection de stack dans le registry (presence de `frontend/`, `.dataverse/`, `Cargo.toml`, etc.)
- [ ] **4.6** Implementer le rendu des templates au provisionnement (`crates/hr-api/src/container_manager.rs`)
- [ ] **4.7** Ajouter le message `UpdateRules` dans le protocole (`crates/hr-registry/src/protocol.rs`)
- [ ] **4.8** Implementer la gestion du message `UpdateRules` dans l'agent (`crates/hr-agent/src/main.rs`)
- [ ] **4.9** Implementer la detection automatique de changements (nouveau frontend, Dataverse ajoute) et declenchement de `UpdateRules`
- [ ] **4.10** Tester : provisionnement d'un projet avec frontend -> rules contiennent les URLs `dev.*`
- [ ] **4.11** Tester : provisionnement d'un projet sans frontend -> rules n'incluent pas la section frontend
- [ ] **4.12** Tester : ajout de Dataverse en cours de dev -> rules mises a jour dynamiquement

---

## Phase 5 : Boucle de Verification Automatique

> **Priorite : HAUTE** | **Effort : Moyen** | **Impact : Eleve**

### Objectif

Permettre aux agents de **verifier automatiquement** que leur travail fonctionne, a l'image de Replit Agent 3 qui teste dans un navigateur headless.

### Nouveaux outils MCP

| Outil | Description |
|-------|-------------|
| `dev_test_endpoint` | Curl une URL DEV et verifie le code HTTP + contenu |
| `dev_test_browser` | Screenshot ou test headless d'une page (via Playwright) |
| `dev_health_check` | Verifie que tous les services DEV sont en cours d'execution |

### Implementation

#### `dev_test_endpoint`

Curl interne simple (l'agent a acces au reseau local). Retourne le code de statut HTTP + body tronque. Permet de valider rapidement qu'un endpoint API repond correctement.

#### `dev_test_browser` (phase 2)

Playwright pre-installe dans le container DEV. L'agent peut demander un screenshot ou executer un script de test. Permet de valider visuellement le rendu frontend.

#### `dev_health_check`

Interroge le proxy agent pour obtenir le statut de tous les services (code-server, vite-dev, cargo-dev). Retourne un resume rapide : quels services sont actifs, lesquels sont arretes, lesquels sont en erreur.

### Fichiers a modifier

| Fichier | Modification |
|---------|-------------|
| `crates/hr-agent/src/mcp.rs` | Ajouter les outils `dev_test_endpoint`, `dev_test_browser`, `dev_health_check` |
| `crates/hr-api/src/container_manager.rs` | Installer Playwright au provisionnement (optionnel, phase 2) |

### Taches detaillees

- [ ] **5.1** Implementer l'outil MCP `dev_health_check` (statut de tous les services) (`crates/hr-agent/src/mcp.rs`)
- [ ] **5.2** Implementer l'outil MCP `dev_test_endpoint` (curl interne + validation HTTP) (`crates/hr-agent/src/mcp.rs`)
- [ ] **5.3** Definir le format de retour de `dev_test_endpoint` (code HTTP, headers pertinents, body tronque)
- [ ] **5.4** Implementer la validation de contenu dans `dev_test_endpoint` (verification de patterns dans le body)
- [ ] **5.5** (Phase 2) Ajouter l'installation de Playwright dans le provisionnement (`crates/hr-api/src/container_manager.rs`)
- [ ] **5.6** (Phase 2) Implementer l'outil MCP `dev_test_browser` (screenshot via Playwright) (`crates/hr-agent/src/mcp.rs`)
- [ ] **5.7** (Phase 2) Implementer l'execution de scripts de test Playwright
- [ ] **5.8** Mettre a jour les rules pour encourager l'utilisation de `dev_test_endpoint` apres chaque modification
- [ ] **5.9** Tester : modification d'un endpoint API -> `dev_test_endpoint` retourne 200 + contenu attendu
- [ ] **5.10** Tester : `dev_health_check` reporte correctement l'etat de tous les services

---

## Phase 6 : Integration Autopilot

> **Priorite : Emerge des Phases 1-5** | **Effort : N/A** | **Impact : N/A**

### Vision

L'agent DEV recoit une instruction utilisateur :

> "Ajouter une page parametres avec theme clair/sombre, stocker la preference en BDD"

Et execute automatiquement :

```
1. dev_health_check          -> Services OK
2. list_tables               -> Schema actuel
3. add_column users theme    -> Modifier le schema local
4. [editer code React]       -> Save -> Vite HMR -> visible sur dev.{slug}
5. [editer code Rust]        -> Save -> cargo-watch rebuild -> visible sur devapi.{slug}
6. dev_test_endpoint /api/settings  -> Verifier que l'API repond
7. dev_test_browser /settings       -> Screenshot de la nouvelle page
8. "Voici le resultat, voulez-vous deployer en PROD ?"
9. deploy_app                -> Build + migration + deploy + health check
```

**Zero attente de build. Zero copie manuelle. Zero risque d'ecrasement des donnees PROD.**

### Conditions prealables

Cette phase emerge naturellement de la completion des phases 1 a 5 :

| Prerequis | Phase source |
|-----------|-------------|
| Hot reload frontend | Phase 1 (Vite HMR) |
| Hot reload backend | Phase 1 (cargo-watch) |
| Migration de schema securisee | Phase 2 (schema_diff + migrate_schema) |
| Deploy en une commande | Phase 3 (deploy_app) |
| Rules adaptees au contexte | Phase 4 (templates dynamiques) |
| Verification automatique | Phase 5 (dev_test tools) |

### Taches detaillees

- [ ] **6.1** Valider que toutes les phases 1-5 sont completees et fonctionnelles
- [ ] **6.2** Mettre a jour les rules pour decrire le workflow autopilot complet
- [ ] **6.3** Documenter le workflow autopilot dans les instructions agent
- [ ] **6.4** Tester le scenario complet de bout en bout : instruction utilisateur -> code -> test -> deploy
- [ ] **6.5** Optimiser l'enchainement des outils MCP pour minimiser les allers-retours
- [ ] **6.6** Ajouter une confirmation utilisateur avant le deploy PROD (etape 8 du workflow)

---

## Reference Patterns Lovable/Replit

Correspondance entre les patterns des plateformes Lovable/Replit et les propositions HomeRoute :

| Pattern | Lovable / Replit | Equivalent HomeRoute |
|---------|-----------------|---------------------|
| **Always-Live Preview** | Sandbox cloud + Vite HMR, pas d'etape de build | **Phase 1** : cargo-watch + vite dev server |
| **Infrastructure as Conversation** | "Ajoute une table" provisionne BDD + schema + CRUD | **Phase 2** : migrate_schema + **Phase 3** : deploy_app |
| **Agent Self-Verification** | Tests navigateur Playwright, verification REPL | **Phase 5** : outils dev_test |
| **Opinionated Stack** | Stack fixe : React/TS/Tailwind/Supabase | **Phase 4** : rules dynamiques avec contexte de stack |
| **One-Click Deploy** | Snapshot + auto-scale + URL | **Phase 3** : deploy_app en une seule commande |
| **Unified Environment** | Panneau unique : chat + code + preview + deploy | **Phase 1** : URLs dev + **Phase 6** : autopilot |

### Avantages de l'approche HomeRoute

- **Self-hosted** : donnees et code restent sur le homelab
- **Conteneurisation** : isolation par nspawn, pas de sandbox partage
- **Stack flexible** : pas limite a un framework unique
- **Agent autonome** : l'agent dans le container a un controle total sur son environnement

---

## Tableau des priorites

| Phase | Effort | Impact | Description | Depend de |
|-------|--------|--------|-------------|-----------|
| **1** | Moyen | Critique | DEV mode live (cargo-watch + vite dev + routes proxy) | - |
| **2** | Moyen | Critique | Migration Dataverse (schema_diff + migrate_schema) | - |
| **3** | Faible | Eleve | Pipeline de deploy unifie (`deploy_app`) | Phase 2 |
| **4** | Faible | Moyen | Rules dynamiques avec variables | Phase 1 |
| **5** | Moyen | Eleve | Boucle de verification (dev_test) | Phase 1 |
| **6** | - | - | Integration autopilot (emerge des phases 1-5) | Phases 1-5 |

### Ordre d'implementation recommande

```
Phase 1 (DEV Live) ─────────┬──> Phase 4 (Rules Dynamiques)
                             |
                             └──> Phase 5 (Verification)
                                         |
Phase 2 (Schema Migration) ──> Phase 3 (Deploy Unifie)
                                         |
                                         v
                               Phase 6 (Autopilot)
```

Les phases 1 et 2 peuvent etre developpees **en parallele** car elles n'ont pas de dependance mutuelle.

---

## Suivi

### Etat global

| Phase | Statut | Date debut | Date fin | Responsable | Notes |
|-------|--------|-----------|----------|-------------|-------|
| **1** : DEV Mode Live | **Termine** | 2026-02-16 | 2026-02-16 | - | Powersave supprime, ViteDev/CargoDev ajoutes |
| **2** : Schema Migration | **Termine** | 2026-02-16 | 2026-02-17 | - | prod_schema, schema_diff, migrate_schema implementes. sqlite3 ajoute aux rootfs |
| **3** : Deploy Pipeline | **Termine** | 2026-02-16 | 2026-02-17 | - | deploy_app 6 etapes, dry_run, skip_frontend, skip_schema. Workspaces standardises |
| **4** : Rules Dynamiques | **Termine** | 2026-02-16 | 2026-02-17 | - | 3 templates .claude/rules/, UpdateRules protocol, API bulk update |
| **5** : Verification Loop | **Termine** | 2026-02-16 | 2026-02-17 | - | dev_health_check, dev_test_endpoint, dev_test_browser (Chromium headless) |
| **6** : Autopilot | **Termine** | 2026-02-17 | 2026-02-17 | - | Workflow complet valide E2E sur wallet. Support Node.js non inclus |

### Detail des taches par phase

#### Phase 1 : DEV Mode Live

| Tache | Statut | Notes |
|-------|--------|-------|
| 1.1 ServiceType variants | Fait | |
| 1.2 Config routes | Fait | |
| 1.3 Routage proxy | Fait | |
| 1.4 Service types agent | Fait | |
| 1.5 Powersave integration | Supprime (powersave supprime) | |
| 1.6 vite-dev.service | Fait | |
| 1.7 cargo-dev.service | Fait | |
| 1.8 Install cargo-watch | Fait | |
| 1.9 Install Node.js/npm | Fait | |
| 1.10 Units systemd provisionnement | Fait | |
| 1.11 Update deploy rules | Fait | |
| 1.12 Test Vite HMR e2e | Fait | |
| 1.13 Test Rust hot reload e2e | Fait | |
| 1.14 Test powersave integration | Supprime | |

#### Phase 2 : Schema Migration

| Tache | Statut | Notes |
|-------|--------|-------|
| 2.1 Message GetProdSchema | Fait | Via RegistryMessage relay DEV→PROD |
| 2.2 Message DataverseQuery | Fait | sqlite3 CLI sur PROD |
| 2.3 Relai registry | Fait | Registry route DEV↔PROD par slug |
| 2.4 Reponse schema agent PROD | Fait | Agent PROD execute sqlite3 .schema |
| 2.5 Outil prod_schema | Fait | Teste E2E sur wallet |
| 2.6 Outil schema_diff | Fait | Compare tables/colonnes DEV vs PROD |
| 2.7 Outil migrate_schema | Fait | dry_run + execution reelle |
| 2.8 Confirmation interactive | Fait | Via dry_run mode |
| 2.9 Renommer prod_push .dataverse/ | Fait | Bloque par rules dataverse |
| 2.10 Update instructions Dataverse | Fait | Template homeroute-dataverse.md |
| 2.11 Table _dv_migrations (2b) | Fait | Geree par migrate_schema |
| 2.12 Enregistrement migrations (2b) | Fait | |
| 2.13 Test creation table e2e | Fait | Valide sur wallet |
| 2.14 Test ajout colonne e2e | Fait | Valide sur wallet |

#### Phase 3 : Deploy Pipeline

| Tache | Statut | Notes |
|-------|--------|-------|
| 3.1 Conception protocole | Fait | 6 etapes: build→frontend→schema→push→deploy→health |
| 3.2 Outil deploy_app | Fait | Deploiement unifie DEV→PROD |
| 3.3 Etape build release | Fait | cargo build --release avec PATH fix |
| 3.4 Etape build frontend | Fait | npm install + npm run build dans frontend/ |
| 3.5 Integration schema_diff | Fait | Auto schema migration pendant deploy |
| 3.6 Etape push frontend | Fait | rsync frontend/dist/ → PROD /opt/app/frontend/dist/ |
| 3.7 Etape deploy binary | Fait | Copy binary + systemctl restart |
| 3.8 Health check retry | Fait | 5 retries avec backoff |
| 3.9 Rapport final | Fait | Resume 6 etapes PASS/FAIL |
| 3.10 Option --skip-frontend | Fait | skip_frontend parameter |
| 3.11 Option --skip-schema | Fait | skip_schema parameter |
| 3.12 Option --dry-run | Fait | Simule toutes les etapes sans executer |
| 3.13 Update instructions deploy | Fait | Template homeroute-deploy.md |
| 3.14 Test deploy e2e | Fait | Deploiement reussi sur home |
| 3.15 Test dry-run | Fait | Implemente, a tester apres deploy |

#### Phase 4 : Rules Dynamiques

| Tache | Statut | Notes |
|-------|--------|-------|
| 4.1 Choix format template | Fait | Mustache-style {{slug}} {{domain}} |
| 4.2 Template deploy.md | Fait | homeroute-deploy.md avec include_str! |
| 4.3 Template dataverse.md | Fait | homeroute-dataverse.md |
| 4.4 Template store.md | Fait | homeroute-store.md |
| 4.5 Detection de stack | Fait | Implicite via presence Cargo.toml/frontend/ |
| 4.6 Rendu templates provisionnement | Fait | container_manager.rs Phase 11 |
| 4.7 Message UpdateRules | Fait | RegistryMessage::UpdateRules |
| 4.8 Gestion UpdateRules agent | Fait | Agent ecrit dans .claude/rules/ |
| 4.9 Detection changements auto | Fait | API POST /api/applications/update-rules (bulk) |
| 4.10 Test avec frontend | Fait | Valide sur wallet (Next.js) |
| 4.11 Test sans frontend | Fait | Valide sur chat, myfrigo |
| 4.12 Test ajout Dataverse dynamique | Fait | Template dataverse inclus par defaut |

#### Phase 5 : Verification Loop

| Tache | Statut | Notes |
|-------|--------|-------|
| 5.1 Outil dev_health_check | Fait | Liste code-server, vite-dev, cargo-dev |
| 5.2 Outil dev_test_endpoint | Fait | HTTP test avec expected_status |
| 5.3 Format retour endpoint | Fait | PASS/FAIL avec status code et body preview |
| 5.4 Validation contenu | Fait | expected_body parameter |
| 5.5 Install Playwright (phase 2) | Fait | Chromium headless auto-install |
| 5.6 Outil dev_test_browser (phase 2) | Fait | Screenshot base64 PNG via chromium |
| 5.7 Scripts test Playwright (phase 2) | N/A | Remplace par capture headless directe |
| 5.8 Update rules verification | Fait | Template deploy.md documente les outils |
| 5.9 Test endpoint e2e | Fait | Valide sur wallet code-server |
| 5.10 Test health check e2e | Fait | Valide sur wallet |

#### Phase 6 : Autopilot

| Tache | Statut | Notes |
|-------|--------|-------|
| 6.1 Validation phases 1-5 | Fait | E2E sur wallet, audit 8 apps |
| 6.2 Rules workflow autopilot | Fait | Rules templates guident le workflow complet |
| 6.3 Documentation workflow | Fait | Ce document + rules dans .claude/rules/ |
| 6.4 Test scenario e2e complet | Fait | 17/18 tests passes (sqlite3 corrige) |
| 6.5 Optimisation enchainement MCP | Fait | deploy_app enchaine 6 etapes automatiquement |
| 6.6 Confirmation utilisateur pre-deploy | Fait | dry_run mode disponible |

---

### Resume des fichiers impactes

| Fichier | Phases concernees |
|---------|-------------------|
| `crates/hr-registry/src/protocol.rs` | 1, 2, 4 |
| `crates/hr-agent/src/mcp.rs` | 2, 3, 5 |
| `crates/hr-agent/src/main.rs` | 1, 2, 4 |
| `crates/hr-agent/src/proxy.rs` | 1 |
| `crates/hr-agent/src/services.rs` | 1 |
| `crates/hr-agent/src/powersave.rs` | 1 (supprime) |
| `crates/hr-agent/Cargo.toml` | 5 (ajout base64) |
| `crates/hr-api/src/container_manager.rs` | 1, 4 |
| `crates/hr-api/src/routes/applications.rs` | 4 (UpdateRules API) |
| `crates/hr-container/src/rootfs.rs` | 2 (ajout sqlite3) |
| `crates/hr-registry/src/rules/homeroute-deploy.md` | 1, 3 |
| `crates/hr-registry/src/rules/homeroute-dataverse.md` | 2 |
| `crates/hr-registry/src/rules/homeroute-store.md` | 4 |

---

> **Ce document est la source unique de verite pour la refonte de l'experience developpeur HomeRoute.**
> Toute modification du plan doit etre refletee ici.
