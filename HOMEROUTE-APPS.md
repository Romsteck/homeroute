# Cycle de vie des applications HomeRoute

Guide complet du workflow applicatif, de la creation a la suppression.

## Table des matieres

1. [Architecture des fichiers](#1-architecture-des-fichiers)
2. [Creation d'une application](#2-creation-dune-application)
3. [Registre et etat](#3-registre-et-etat)
4. [Proxy routing](#4-proxy-routing)
5. [Suppression d'une application](#5-suppression-dune-application)
6. [Apps externes (non-conteneur)](#6-apps-externes-non-conteneur)
7. [Reference rapide des operations](#7-reference-rapide-des-operations)

---

## 1. Architecture des fichiers

### Fichiers de persistance

| Fichier | Chemin | Role |
|---------|--------|------|
| **agent-registry.json** | `/var/lib/server-dashboard/agent-registry.json` | Registre des applications (etat, tokens, config frontend) |
| **containers-v2.json** | `/var/lib/server-dashboard/containers-v2.json` | Conteneurs nspawn (volumes, stack, storage path) |
| **rust-proxy-config.json** | `/var/lib/server-dashboard/rust-proxy-config.json` | Routes statiques du reverse proxy |
| **reverseproxy-config.json** | `/var/lib/server-dashboard/reverseproxy-config.json` | Hosts standalone (source UI, synchronise vers rust-proxy-config) |
| **app-routes.json** | `/opt/homeroute/data/app-routes.json` | Routes dynamiques des apps (domaine -> IP:port, persiste sur disque) |
| **hosts.json** | `/data/hosts.json` | Hosts distants (MAC, IP, interface LAN) |

### Relations entre fichiers

```
containers-v2.json          agent-registry.json          app-routes.json
  ContainerV2Record    <-->    Application             -->  AppRoute
  (meme id/UUID)               (meme container_name)       (domaine -> IP:port)
                                     |
                                     v
                            rust-proxy-config.json
                            (routes statiques, independantes)
```

**Regle cle** : `ContainerV2Record.id == Application.id`. Les deux fichiers partagent le meme UUID et le meme `container_name`.

---

## 2. Creation d'une application

### Flux complet

La creation passe par l'API REST, qui communique avec l'orchestrateur via IPC Unix socket.

```
UI/API  -->  POST /api/containers  -->  hr-api
                                          |
                                    IPC /run/hr-orchestrator.sock
                                          |
                                    OrchestratorRequest::CreateContainer
                                          |
                              +-----------+-----------+
                              |                       |
                      ContainerManager          AgentRegistry
                     (containers-v2.json)    (agent-registry.json)
```

### Etapes detaillees

#### 1. Requete API

`POST /api/containers` avec body JSON :
```json
{
  "name": "My App",
  "slug": "myapp",
  "host_id": "local",
  "environment": "production",
  "stack": "next-js"
}
```

Stacks disponibles : `next-js`, `leptos-rust`, `axum-vite`.

#### 2. Creation dans le registre (`AgentRegistry.create_application_headless`)

1. Genere un UUID pour l'app
2. Genere un token aleatoire de 32 bytes (64 chars hex)
3. Hash le token avec Argon2id (seul le hash est stocke)
4. Cree l'entree `Application` :
   - `container_name` : `hr-v2-{slug}-prod` ou `hr-v2-{slug}-dev`
   - `status` : `Deploying`
   - `enabled` : `true`
   - `frontend.target_port` : `3000` (defaut)
5. Persiste dans `agent-registry.json`
6. Retourne l'app + le token en clair (une seule fois)

#### 3. Creation dans le container manager

1. Cree l'entree `ContainerV2Record` dans `containers-v2.json` (meme UUID)
2. Status initial : `Deploying`

#### 4. Deploiement du conteneur nspawn

**Conteneur production** (simplifie) :
1. Bootstrap rootfs Ubuntu 24.04 via `debootstrap --variant=minbase noble`
2. Ecrit le fichier `.nspawn` dans `/etc/systemd/nspawn/{name}.nspawn`
3. Configure le reseau (bridge ou macvlan)
4. Copie le binaire `hr-agent` depuis `/opt/homeroute/data/agent-binaries/hr-agent`
5. Ecrit `/etc/hr-agent.toml` avec le token
6. Cree le service systemd `hr-agent.service` (Restart=always)
7. Demarre le conteneur via `machinectl start`
8. Installe les dependances (curl, ca-certificates, git, node.js si NextJs)
9. Status -> `Running`

**Conteneur developpement** (complet) : memes etapes + code-server, Claude Code CLI, Rust toolchain, workspace, Chrome.

#### 5. Connexion de l'agent

Quand `hr-agent` demarre dans le conteneur :
1. Se connecte en WebSocket a `ws://10.0.0.254:4001/agents/ws`
2. Envoie `Auth { token, service_name, version, ipv4 }`
3. Le registre verifie le token (Argon2 verify)
4. Status -> `Connected`
5. Le registre pousse la config (base_domain, slug, frontend, etc.)
6. L'agent publie ses routes via `PublishRoutes`
7. Le registre enregistre les routes dans hr-edge via IPC `SetAppRoute`
8. Le registre ajoute les DNS records locaux via hr-netcore IPC

#### 6. Creation automatique dev+prod

Si on cree un conteneur **dev** avec `linked_app_id`, le systeme peut automatiquement creer le conteneur **prod** associe. Les deux sont lies via `linked_app_id` (bidirectionnel).

### Fichier .nspawn genere

```ini
[Exec]
Boot=yes
PrivateUsers=no

[Network]
Bridge=br-lan

[Files]
Bind=/var/lib/machines/hr-v2-myapp-prod-workspace:/root/workspace
```

---

## 3. Registre et etat

### Structure Application

```json
{
  "id": "uuid-...",
  "name": "My App",
  "slug": "myapp",
  "host_id": "local",
  "environment": "production",
  "linked_app_id": null,
  "enabled": true,
  "container_name": "hr-v2-myapp-prod",
  "token_hash": "$argon2id$...",
  "ipv4_address": "10.0.0.122",
  "status": "connected",
  "last_heartbeat": "2026-03-16T12:00:00Z",
  "agent_version": "0.6.1",
  "created_at": "2026-01-15T10:00:00Z",
  "frontend": {
    "target_port": 3000,
    "auth_required": false,
    "allowed_groups": [],
    "local_only": false
  },
  "code_server_enabled": true,
  "stack": "next-js"
}
```

### Etats de l'agent (`AgentStatus`)

| Status | Signification |
|--------|---------------|
| `Pending` | Conteneur cree, agent pas encore connecte |
| `Deploying` | Deploiement en cours |
| `Connected` | Agent connecte et heartbeat actif |
| `Disconnected` | Heartbeat manquant ou derniere connexion WebSocket fermee |
| `Error` | Erreur de recuperation ou probleme conteneur |

### Flag `enabled`

- Stocke dans `Application.enabled` dans `agent-registry.json`
- **Mis a `true`** quand on demarre un conteneur (`POST /api/containers/{id}/start`)
- **Mis a `false`** quand on arrete un conteneur (`POST /api/containers/{id}/stop`)
- Peut etre toggle via `PUT /api/applications/{id}` (champ `enabled`)
- Un conteneur `enabled: false` est **ignore** par le container_watcher (pas de tentative de relance)

### Container Watcher (auto-recovery)

Le `ContainerWatcher` surveille les conteneurs **production** et **enabled** :

**Parametres** :
- Intervalle de verification : **60 secondes**
- Timeout heartbeat : **120 secondes** (2 min sans heartbeat = stale)
- Rate limit : **3 tentatives par heure** par conteneur
- Delai initial : 30 secondes au demarrage

**Logique de decision** :
```
Pour chaque application :
  1. Skip si environment != Production
  2. Skip si enabled == false
  3. Skip si status == Deploying ou Pending
  4. Verifier le heartbeat :
     - Jamais recu ? Stale si cree il y a >240s
     - Recu ? Stale si >120s depuis le dernier
  5. Si stale ET rate limit pas atteint :
     a. machinectl terminate {container}
     b. ip link delete vb-{container}
     c. systemctl restart systemd-nspawn@{container}.service
```

### Recuperation au demarrage

Au boot de l'orchestrateur :
1. Charge `agent-registry.json` et `containers-v2.json`
2. Si le registre est corrompu/vide mais les conteneurs existent, reconstruit le registre depuis les fichiers `/etc/hr-agent.toml` dans les rootfs
3. Redemarre tous les conteneurs locaux qui avaient `status == Running`
4. Quand un host distant se reconnecte, envoie `StartContainer` pour ses conteneurs

### Connexions multiples

Un agent peut avoir **plusieurs connexions WebSocket simultanees** (agent principal + MCP tools + IDE). Le registre maintient un `active_count`. Les routes ne sont supprimees que quand la **derniere** connexion se ferme.

---

## 4. Proxy routing

### Vue d'ensemble

hr-edge (port 443) recoit les requetes HTTPS et les route selon cette priorite :

```
Requete HTTPS
  |
  v
1. Management ? (proxy.{base} ou auth.{base})
   OUI -> localhost:4000 (API) ou localhost:4001 (orchestrator)
   NON |
       v
2. App route dynamique ? (HashMap en memoire + app-routes.json)
   OUI -> IP:port du conteneur (ex: 10.0.0.122:3000)
   NON |
       v
3. Route statique ? (rust-proxy-config.json)
   OUI -> target_host:target_port
   NON |
       v
4. 404 Domain Not Found
```

### Routes dynamiques (apps)

Quand un agent se connecte et publie ses routes (`PublishRoutes`), l'orchestrateur envoie a hr-edge via IPC :

```rust
EdgeRequest::SetAppRoute {
    domain: "myapp.mynetwk.biz",
    app_id: "uuid-...",
    host_id: "local",
    target_ip: "10.0.0.122",
    target_port: 3000,
    auth_required: false,
    allowed_groups: [],
    local_only: false,
}
```

- Les routes sont stockees en memoire dans un `HashMap<String, AppRoute>`
- Persistees sur disque dans `/opt/homeroute/data/app-routes.json`
- Rechargees au demarrage de hr-edge
- Un conteneur prod `myapp` genere le domaine `myapp.{base_domain}`
- Un conteneur dev `myapp` genere les domaines `dev.myapp.{base_domain}`, `code.myapp.{base_domain}`, `studio.myapp.{base_domain}`

### Routes statiques

Definies dans `rust-proxy-config.json` :

```json
{
  "routes": [
    {
      "id": "route-uuid",
      "domain": "test.mynetwk.biz",
      "backend": "rust",
      "target_host": "10.0.0.10",
      "target_port": 8080,
      "local_only": false,
      "require_auth": false,
      "enabled": true,
      "cert_id": "auto-generated"
    }
  ]
}
```

### Difference entre routes dynamiques et statiques

| Aspect | Routes dynamiques (apps) | Routes statiques |
|--------|--------------------------|------------------|
| Source | Agent publie ses routes | reverseproxy-config.json / rust-proxy-config.json |
| Persistance | app-routes.json | rust-proxy-config.json |
| Lifecycle | Liees au conteneur, supprimees quand l'agent se deconnecte | Manuelles, persistent indefiniment |
| Gestion | Automatique via l'agent | Via l'UI ou l'API |
| Certificats | Wildcard per-app via ACME | Per-route via CA locale ou ACME |
| Priorite | Haute (verifiee en premier) | Basse (fallback) |

### Domaines generes par une app

L'objet `Application` calcule ses domaines via la methode `domains()` :

- **Production** : `{slug}.{base_domain}` (ex: `myapp.mynetwk.biz`)
- **Developpement** :
  - `dev.{slug}.{base_domain}` (app)
  - `code.{slug}.{base_domain}` (code-server, si enabled)
  - `studio.{slug}.{base_domain}` (studio)

Chaque app a aussi un **certificat wildcard** : `*.{slug}.{base_domain}` (obtenu via ACME DNS-01 Cloudflare).

### Certificats TLS

hr-edge utilise un `SniResolver` pour le TLS :
1. Cherche un certificat exact pour le domaine
2. Cherche un wildcard parent (`*.myapp.mynetwk.biz` -> `*.mynetwk.biz`)
3. Fallback au certificat global si sous-domaine du base_domain
4. Rejette les domaines externes sans certificat

Rechargement des certificats via `SIGHUP` (`systemctl reload hr-edge`) ou via IPC `EdgeRequest::ReloadConfig`.

### DNS local

Quand un agent publie ses routes, le registre ajoute des records DNS locaux via hr-netcore IPC (`DnsAddStaticRecord`). Ceci permet aux clients LAN de resoudre les domaines directement vers l'IP du conteneur.

---

## 5. Suppression d'une application

### Via l'API (methode recommandee)

`DELETE /api/containers/{id}`

### Flux complet de suppression

```
DELETE /api/containers/{id}
  |
  v
hr-api :
  1. Recupere l'app depuis l'orchestrateur
  2. Supprime les routes du proxy (edge IPC: RemoveAppRoute pour chaque domaine)
  3. Supprime les DNS records locaux (netcore IPC: DnsRemoveStaticRecordsByValue)
  |
  v
OrchestratorRequest::DeleteApplication :
  4. Envoie Shutdown a l'agent (si connecte)
  5. Supprime le certificat wildcard per-app (ACME)
  6. Supprime les DNS records Cloudflare (A + AAAA pour *.{slug}.{base})
  7. Nettoie le linked_app_id sur l'app partenaire (si liee)
  8. Supprime l'entree de agent-registry.json
  |
  v
OrchestratorRequest::DeleteContainer :
  9.  machinectl terminate {container}
  10. Attend 2 secondes
  11. Supprime le rootfs : /var/lib/machines/{container}
  12. Supprime le workspace : /var/lib/machines/{container}-workspace
  13. Supprime le fichier .nspawn : /etc/systemd/nspawn/{container}.nspawn
  14. Supprime le symlink si storage custom
  15. Supprime l'entree de containers-v2.json
```

### Fichiers nettoyes

| Fichier/Ressource | Action |
|--------------------|--------|
| `agent-registry.json` | Entree supprimee |
| `containers-v2.json` | Entree supprimee |
| `app-routes.json` | Routes supprimees (via edge IPC) |
| `/var/lib/machines/{container}/` | Rootfs supprime |
| `/var/lib/machines/{container}-workspace/` | Workspace supprime |
| `/etc/systemd/nspawn/{container}.nspawn` | Unite supprimee |
| DNS local (hr-netcore) | Records supprimes |
| DNS Cloudflare | Records A + AAAA wildcard supprimes |
| Certificat ACME wildcard | Supprime |

### Garantie de non-retour

Apres suppression :
- Le conteneur nspawn est termine et son rootfs supprime -> `machinectl start` echouera
- L'unite `.nspawn` est supprimee -> `systemctl restart systemd-nspawn@...` echouera
- L'entree registre est supprimee -> le container_watcher ne tentera pas de relancer
- Les routes proxy sont supprimees -> le domaine retourne 404
- Les DNS sont supprimes -> le domaine ne resout plus

**L'app ne reviendra pas apres un restart des services.**

### Edition manuelle (non recommandee)

Si l'API n'est pas disponible, il faut nettoyer manuellement :
1. `machinectl terminate hr-v2-{slug}-prod`
2. `rm -rf /var/lib/machines/hr-v2-{slug}-prod`
3. `rm -rf /var/lib/machines/hr-v2-{slug}-prod-workspace`
4. `rm /etc/systemd/nspawn/hr-v2-{slug}-prod.nspawn`
5. Editer `agent-registry.json` : supprimer l'entree
6. Editer `containers-v2.json` : supprimer l'entree
7. `systemctl reload hr-edge` (pour recharger les routes)
8. Nettoyer les DNS Cloudflare manuellement si necessaire

---

## 6. Apps externes (non-conteneur)

### Oui, c'est possible

HomeRoute supporte le routing vers des hosts externes via les **routes statiques** du reverse proxy.

### Methode 1 : Via l'UI (reverseproxy-config.json)

L'UI gere `reverseproxy-config.json`. Un host est defini ainsi :

```json
{
  "id": "host-uuid",
  "enabled": true,
  "subdomain": "files",
  "customDomain": "",
  "targetHost": "10.0.0.15",
  "targetPort": 3000,
  "localOnly": false,
  "requireAuth": false
}
```

Ceci cree la route `files.mynetwk.biz` -> `10.0.0.15:3000`.

Quand sauvegarde via l'UI, le systeme :
1. Met a jour `reverseproxy-config.json`
2. Synchronise vers `rust-proxy-config.json` (transforme les hosts en `RouteConfig`)
3. Envoie `EdgeRequest::ReloadConfig` via IPC -> hr-edge recharge sa config

### Methode 2 : Via l'API REST

```bash
# Ajouter un host
curl -X POST http://10.0.0.254:4000/api/reverseproxy/hosts \
  -H 'Content-Type: application/json' \
  -d '{
    "subdomain": "files",
    "targetHost": "10.0.0.15",
    "targetPort": 3000,
    "enabled": true,
    "localOnly": false,
    "requireAuth": false
  }'

# Recharger le proxy
curl -X POST http://10.0.0.254:4000/api/reverseproxy/reload
```

### Methode 3 : Edition directe de rust-proxy-config.json

Ajouter dans le tableau `routes` :

```json
{
  "id": "custom-files-route",
  "domain": "files.mynetwk.biz",
  "backend": "rust",
  "target_host": "10.0.0.15",
  "target_port": 3000,
  "local_only": false,
  "require_auth": false,
  "enabled": true
}
```

Puis : `systemctl reload hr-edge` (SIGHUP)

### Limitations des apps externes

- Pas de gestion automatique des certificats (il faut un cert_id ou utiliser le wildcard global)
- Pas de health monitoring (pas d'agent, donc pas de heartbeat)
- Pas de container_watcher (pas de relance automatique)
- Pas de deploiement automatise
- Pas de terminal WebSocket
- Pas de metriques agent

### Exemple concret : files.mynetwk.biz -> 10.0.0.15:3000

```bash
# 1. Ajouter la route
curl -X POST http://10.0.0.254:4000/api/reverseproxy/hosts \
  -H 'Content-Type: application/json' \
  -d '{"subdomain":"files","targetHost":"10.0.0.15","targetPort":3000,"enabled":true,"localOnly":false,"requireAuth":false}'

# 2. Verifier
curl -s http://10.0.0.254:4000/api/reverseproxy/hosts | jq

# 3. Le proxy est automatiquement recharge
# 4. Tester
curl -k https://files.mynetwk.biz/
```

---

## 7. Reference rapide des operations

### API endpoints principaux

| Operation | Methode | Endpoint |
|-----------|---------|----------|
| Lister les apps | GET | `/api/containers` |
| Creer une app | POST | `/api/containers` |
| Supprimer une app | DELETE | `/api/containers/{id}` |
| Demarrer | POST | `/api/containers/{id}/start` |
| Arreter | POST | `/api/containers/{id}/stop` |
| Mettre a jour | PUT | `/api/containers/{id}` |
| Executer commande | POST | `/api/applications/{id}/exec` |
| Terminal WebSocket | GET | `/api/containers/{id}/terminal` |
| Deployer en prod | POST | `/api/applications/{id}/deploy` |
| Lister routes proxy | GET | `/api/reverseproxy/hosts` |
| Ajouter route externe | POST | `/api/reverseproxy/hosts` |
| Recharger proxy | POST | `/api/reverseproxy/reload` |

### IPC sockets

| Socket | Requetes principales |
|--------|---------------------|
| `/run/hr-orchestrator.sock` | CreateContainer, DeleteContainer, StartContainer, StopContainer, ListContainers, DeployToProduction |
| `/run/hr-edge.sock` | SetAppRoute, RemoveAppRoute, ReloadConfig, AcmeRequestAppWildcard |
| `/run/hr-netcore.sock` | DnsAddStaticRecord, DnsRemoveStaticRecordsByValue |

### Timeouts et constantes

| Constante | Valeur |
|-----------|--------|
| Heartbeat agent | 90s avant marquage stale |
| Container watcher check | 60s |
| Container watcher timeout | 120s |
| Max recovery/heure | 3 |
| IPC timeout defaut | 30s |
| IPC timeout long | 120s |
| Agent auth timeout | 5s |
| Connexions multiples | Oui (active_count) |

### Fichiers dans le conteneur

| Fichier | Role |
|---------|------|
| `/etc/hr-agent.toml` | Config agent (token, interface, port) |
| `/etc/systemd/system/hr-agent.service` | Service systemd de l'agent |
| `/etc/systemd/network/80-container.network` | Config reseau (DHCP) |
| `/etc/resolv.conf` | DNS (10.0.0.254 + 8.8.8.8, immutable) |
