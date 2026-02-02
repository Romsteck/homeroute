# Multi-Agents HomeRoute — Suivi d'avancement

## Résumé

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | hr-lxd + hr-registry (fondation) | En attente |
| Phase 2 | hr-agent (binaire sidecar) | En attente |
| Phase 3 | Frontend + nettoyage reverse proxy | En attente |
| Phase 4 | Auto-update + hardening | En attente |

---

## Phase 1 : hr-lxd + hr-registry

### 1A. hr-lxd

- [ ] Créer `crates/hr-lxd/Cargo.toml`
- [ ] Créer `crates/hr-lxd/src/lib.rs`
- [ ] Créer `crates/hr-lxd/src/client.rs` — LxdClient (create/delete/push/exec)
- [ ] Créer `crates/hr-lxd/src/profile.rs` — profil `homeroute-agent` (br-lan)

### 1B. hr-registry

- [ ] Créer `crates/hr-registry/Cargo.toml`
- [ ] Créer `crates/hr-registry/src/lib.rs`
- [ ] Créer `crates/hr-registry/src/types.rs` — Application, AgentStatus, RegistryState
- [ ] Créer `crates/hr-registry/src/protocol.rs` — AgentMessage / RegistryMessage
- [ ] Créer `crates/hr-registry/src/state.rs` — AgentRegistry
- [ ] Créer `crates/hr-registry/src/cloudflare.rs` — upsert/delete AAAA

### 1C. Intégration

- [ ] Modifier `crates/Cargo.toml` — ajouter hr-lxd, hr-registry aux members
- [ ] Modifier `crates/homeroute/Cargo.toml` — deps
- [ ] Modifier `crates/hr-api/Cargo.toml` — dep hr-registry
- [ ] Modifier `crates/hr-api/src/state.rs` — champ registry
- [ ] Créer `crates/hr-api/src/routes/applications.rs` — API REST + WebSocket
- [ ] Modifier `crates/hr-api/src/routes/mod.rs` — pub mod applications
- [ ] Modifier `crates/hr-api/src/lib.rs` — nest /applications
- [ ] Modifier `crates/hr-common/src/events.rs` — AgentStatusEvent
- [ ] Modifier `crates/homeroute/src/main.rs` — init registry, LXD profile, spawn tasks
- [ ] Modifier `crates/hr-api/src/routes/ddns.rs` — refactorer vers cloudflare partagé
- [ ] `cargo build --release` compile

---

## Phase 2 : hr-agent

- [ ] Créer `crates/hr-agent/Cargo.toml`
- [ ] Créer `crates/hr-agent/src/main.rs` — entry point, reconnexion backoff
- [ ] Créer `crates/hr-agent/src/config.rs` — AgentConfig (TOML)
- [ ] Créer `crates/hr-agent/src/connection.rs` — client WebSocket
- [ ] Créer `crates/hr-agent/src/proxy.rs` — HTTPS reverse proxy SNI multi-domaines
- [ ] Créer `crates/hr-agent/src/ipv6.rs` — ip addr add/del
- [ ] Modifier `crates/Cargo.toml` — ajouter hr-agent, tokio-tungstenite, toml
- [ ] Modifier `crates/hr-api/src/routes/auth.rs` — endpoint forward-check
- [ ] `cargo build --release -p hr-agent` compile
- [ ] Test E2E : LXC créé → agent connecté → TLS multi-domaine fonctionne

---

## Phase 3 : Frontend + nettoyage

- [ ] Créer `web/src/pages/Applications.jsx`
- [ ] Modifier `web/src/api/client.js` — endpoints /api/applications/*
- [ ] Modifier `web/src/App.jsx` — route /applications
- [ ] Modifier `web/src/components/Sidebar.jsx` — lien Applications
- [ ] Modifier `web/src/pages/ReverseProxy.jsx` — supprimer onglet Applications
- [ ] Supprimer `web/src/components/ApplicationCard.jsx`
- [ ] Modifier `crates/hr-api/src/routes/reverseproxy.rs` — supprimer apps/envs
- [ ] Modifier `crates/hr-api/src/routes/ws.rs` — broadcast agent events
- [ ] `npm run build` compile
- [ ] Test : page /applications crée/supprime des apps avec LXC

---

## Phase 4 : Auto-update + hardening

- [ ] Créer `crates/hr-agent/src/update.rs`
- [ ] Endpoints version/binary dans applications.rs
- [ ] Prefix watcher dans main.rs
- [ ] Tokens argon2 + timeout auth WS
- [ ] Persistance atomique
- [ ] Backoff reconnexion agent
- [ ] Cert renewal background
- [ ] Tests intégration complète