# Migration Architecture Multi-Hôte HomeRoute

## Vue d'ensemble

Refonte majeure de l'architecture pour support multi-hôte avec simplification du routage.

## Changements majeurs

| Avant | Après |
|-------|-------|
| Reverse proxy par agent LXC (TLS, SNI, forward-auth) | hr-proxy centralise tout le routage |
| IPv6 dédiée par container + sync DNS par app | Wildcard DNS unique `*.base_domain` |
| TLS passthrough SNI dans main listener | TLS termination centralisée hr-proxy |
| Pages "Serveurs" + "WOL" séparées | Page "Hôtes" unifiée |
| Un seul hôte LXC (HomeRoute) | Multi-hôte avec hr-host-agent |
| Pas de migration LXC | Migration complète via WebSocket + progress |

## Phases

### Phase 1 : Simplification agent + proxy
- [x] 1.1 Supprimer proxy/IPv6/pages de hr-agent
- [x] 1.2 Simplifier protocole (retirer routes/certs/IPv6 du Config)
- [x] 1.3 App route map dans hr-proxy
- [x] 1.4 Retirer TLS passthrough du listener principal
- [x] 1.5 Retirer DNS/CF/firewall par app du registry
- [x] 1.6 Configurer wildcard DNS
- [x] 1.7 ActivityPing pour suivi inactivité powersave
- [x] 1.8 Wake-on-Demand pour services LXC

### Phase 2 : Fusion Server → Host
- [x] 2.1 Modèle de données Host
- [x] 2.2 Routes API /api/hosts/*
- [x] 2.3 Migration données servers.json → hosts.json
- [x] 2.4 Service monitoring hôtes
- [x] 2.5 Page React Hôtes (fusion Serveurs + WOL)
- [ ] 2.6 Supprimer ancien code (legacy /servers + /wol conservés temporairement)

### Phase 3 : Support multi-hôte
- [x] 3.1 host_id sur Application
- [x] 3.2 Crate hr-host-agent
- [x] 3.3 Registry multi-hôte
- [x] 3.4 WOD pour hôtes distants

### Phase 4 : Migration LXC
- [x] 4.1 Protocole migration dans hr-host-agent
- [x] 4.2 Endpoints API migration
- [x] 4.3 Événements progression temps réel
- [x] 4.4 UI migration avec progress bar

## Fichiers impactés

### Suppressions
- `crates/hr-agent/src/proxy.rs` (~878 lignes)
- `crates/hr-agent/src/ipv6.rs` (~134 lignes)
- `crates/hr-agent/src/pages.rs` (~240 lignes)
- `crates/hr-api/src/routes/servers.rs`
- `crates/hr-api/src/routes/wol.rs`
- `web/src/pages/Servers.jsx`, `web/src/pages/Wol.jsx`

### Modifications majeures
- `crates/hr-agent/src/main.rs` — retrait proxy, ajout ActivityPing
- `crates/hr-registry/src/state.rs` — retrait DNS/CF/firewall, ajout routes app
- `crates/hr-registry/src/protocol.rs` — simplification protocole
- `crates/hr-registry/src/types.rs` — retrait IPv6/certs, ajout host_id
- `crates/hr-proxy/src/handler.rs` — AppRoute map, WOD, ActivityPing
- `crates/homeroute/src/main.rs` — retrait SNI passthrough
- `crates/hr-common/src/events.rs` — HostStatusEvent, MigrationProgressEvent
- `web/src/App.jsx` — routes /hosts remplace /servers + /wol
- `web/src/api/client.js` — endpoints /api/hosts/*
- `web/src/components/Sidebar.jsx` — lien Hosts

### Créations
- `crates/hr-host-agent/` — nouveau crate agent hôte
- `crates/hr-api/src/routes/hosts.rs`
- `web/src/pages/Hosts.jsx` — page React unifiée hôtes
- `/data/hosts.json`

## Journal de progression

| Date | Phase | Action | Statut |
|------|-------|--------|--------|
| 2026-02-06 | 1.1 | Supprimé proxy.rs, ipv6.rs, pages.rs de hr-agent | Done |
| 2026-02-06 | 1.2 | Simplifié protocol.rs (retiré AgentRoute, Ipv6Update, CertUpdate, ajouté ActivityPing) | Done |
| 2026-02-06 | 1.2 | Simplifié types.rs (retiré ipv6_suffix, ipv6_address, cert_ids, cloudflare_record_ids) | Done |
| 2026-02-06 | 1.3 | Remplacé agent_passthrough par app_routes HashMap<String, AppRoute> dans hr-proxy | Done |
| 2026-02-06 | 1.4 | Retiré extract_sni() et branche SNI passthrough de main.rs | Done |
| 2026-02-06 | 1.5 | Nettoyé registry state.rs (retiré app_dns_store, firewall, acme, CF sync) | Done |
| 2026-02-06 | 1.5 | Retiré AppDnsStore de hr-dns | Done |
| 2026-02-06 | 1.5 | Mis à jour applications.rs (passthrough → AppRoute) | Done |
| 2026-02-06 | 1.5 | Integration: make all + make test OK (86 tests, 0 failures) | Done |
| 2026-02-06 | 1.6 | Wildcard DNS: supprimé 30 static records locaux + 15 CF per-app records | Done |
| 2026-02-06 | 1.6 | Créé *.code.mynetwk.biz AAAA CF record (proxied) pour code-server | Done |
| 2026-02-06 | 1.7 | ActivityPing: proxy envoie ping au registry après chaque requête app réussie | Done |
| 2026-02-06 | 1.8 | Wake-on-Demand: page HTML auto-refresh + ServiceCommand::Start sur connexion refusée | Done |
| 2026-02-06 | 1.8 | Déployé + testé: WOD déclenche démarrage service, app accessible après ~5s | Done |
| 2026-02-06 | 2.1 | Créé hosts.rs: modèle Host unifié (CRUD + power + schedules imbriqués) | Done |
| 2026-02-06 | 2.2 | Routes /api/hosts/* montées (coexiste avec legacy /servers + /wol) | Done |
| 2026-02-06 | 2.3 | Migration auto servers.json + wol-schedules.json → /data/hosts.json au démarrage | Done |
| 2026-02-06 | 2.4 | Monitoring: HostStatusEvent + hosts.json, scheduler lit schedules imbriqués | Done |
| 2026-02-06 | 2.5 | Page React Hosts.jsx unifiée + Sidebar: Hotes remplace Serveurs+WoL | Done |
| 2026-02-06 | 2.5 | WebSocket: hosts:status event + legacy servers:status maintenu | Done |
| 2026-02-06 | 2.5 | Déployé + testé: API /hosts OK, migration données OK, frontend build OK | Done |
| 2026-02-06 | 3.1 | Ajouté host_id sur Application (default "local") + Create/Update requests | Done |
| 2026-02-06 | 3.2 | Protocole host-agent: HostAgentMessage, HostRegistryMessage, HostMetrics, ContainerInfo | Done |
| 2026-02-06 | 3.3 | Registry: HostConnection tracking + /api/hosts/agent/ws WebSocket endpoint | Done |
| 2026-02-06 | 3.2 | Créé crate hr-host-agent: WebSocket client, heartbeat, metrics /proc, reconnect | Done |
| 2026-02-06 | 3.4 | WOD étendu: WoL magic packet pour hôtes offline, ServiceStart pour hôtes online | Done |
| 2026-02-06 | 3.x | Integration: make all OK (86 tests, 0 failures), make deploy OK, health OK | Done |
