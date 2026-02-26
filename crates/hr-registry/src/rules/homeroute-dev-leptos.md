# Environnement de Developpement — Stack Leptos (Rust SSR + WASM)

## URLs importantes

| Service | URL |
|---------|-----|
| IDE (code-server) | `https://code.{{slug}}.{{domain}}` |
| Application DEV (Leptos SSR) | `https://dev.{{slug}}.{{domain}}` |
| Studio (cet agent) | `https://studio.{{slug}}.{{domain}}` |
| Production | `https://{{slug}}.{{domain}}` |

## Structure du workspace

```
/root/workspace/
├── Cargo.toml            # Config projet Leptos + dependances
├── src/
│   ├── main.rs           # Point d'entree serveur (SSR)
│   ├── app.rs            # Composant racine Leptos
│   └── lib.rs            # Hydration client (WASM)
├── style/                # Fichiers CSS/SCSS
├── public/               # Assets statiques
├── .dataverse/           # Base SQLite locale (ignoree par git)
└── .claude/              # Config agents (rules, config, mcp)
```

## Services systemd dans le container

| Service | Role | Port |
|---------|------|------|
| `code-server.service` | IDE Visual Studio Code | 13337 |
| `cargo-leptos-dev.service` | Serveur de developpement Leptos (SSR + WASM hot-reload) | 3000 |

**Note** : Leptos gere le SSR et le WASM dans un seul processus via `cargo-leptos watch`. Pas de Vite, pas de proxy, pas de service Rust separe.

## Regles de developpement

1. **Ne jamais** modifier les fichiers dans `/opt/app/` (reserve a la production)
2. **Toujours** travailler dans `/root/workspace/`
3. **Tester** sur `https://dev.{{slug}}.{{domain}}` (hot-reload automatique)
4. **Ne jamais** deployer en production sans demande explicite
5. **Ne jamais** lancer `cargo-leptos` manuellement — utiliser le service systemd

## Gestion des services

```bash
# Demarrer le serveur Leptos dev
sudo systemctl start cargo-leptos-dev

# Verifier le statut
systemctl status cargo-leptos-dev

# Voir les logs en temps reel
journalctl -u cargo-leptos-dev -f

# Redemarrer apres changement de config
sudo systemctl restart cargo-leptos-dev
```

## Git et deploiement

- Remote git : `http://10.0.0.254:4000/api/git/repos/{{slug}}.git`
- Push declenche la synchronisation avec le repo production
- Ne pas commiter `target/`, `.dataverse/`, `.env`

## Outils MCP disponibles

- **dev_health_check** : verifie que le serveur Leptos repond
- **dev_test_endpoint** : teste un endpoint API
- **deploy_app** : deploie en production (sur demande explicite seulement)
- **prod_status**, **prod_logs** : monitoring production

## Workflow standard

1. `systemctl status cargo-leptos-dev` — verifier que le serveur tourne
2. Modifier le code dans `/root/workspace/`
3. Hot-reload automatique — tester sur `https://dev.{{slug}}.{{domain}}`
4. Commiter les changements avec git
5. Deployer en production uniquement sur demande explicite

## Resolution de problemes

| Probleme | Solution |
|----------|----------|
| Port 3000 CLOSED | `sudo systemctl restart cargo-leptos-dev` puis `systemctl status cargo-leptos-dev` |
| Hot-reload ne fonctionne pas | `sudo systemctl restart cargo-leptos-dev` |
| Erreur de compilation Rust | Verifier les logs : `journalctl -u cargo-leptos-dev -n 50` |
| WASM build echoue | Verifier que `wasm32-unknown-unknown` est installe : `rustup target list --installed` |
| code-server inaccessible | `sudo systemctl restart code-server` |
