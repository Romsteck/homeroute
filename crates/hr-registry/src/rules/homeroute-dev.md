# Environnement de Developpement

Ce conteneur est l'environnement de developpement pour **{{slug}}**.

## URLs

| Service | URL | Port local |
|---------|-----|------------|
| **IDE** (code-server) | `https://code.{{slug}}.{{domain}}` | 13337 |
| **Frontend HMR** (Vite) | `https://dev.{{slug}}.{{domain}}` | 5173 |
| **API hot-reload** (cargo-watch) | `https://devapi.{{slug}}.{{domain}}` | 3000 |
| **Production** | `https://{{slug}}.{{domain}}` | — |

## Services systemd

Trois services gèrent les processus de développement :

| Service | Commande systemd | Description |
|---------|-------------------|-------------|
| `code-server.service` | `systemctl start/stop/restart code-server` | IDE VS Code dans le navigateur |
| `vite-dev.service` | `systemctl start/stop/restart vite-dev` | Serveur Vite avec Hot Module Replacement |
| `cargo-dev.service` | `systemctl start/stop/restart cargo-dev` | cargo-watch : recompilation Rust automatique sur changement |

### Etat des services

- `code-server` est **demarre automatiquement** au boot du conteneur
- `vite-dev` et `cargo-dev` sont **arretes par defaut** — les demarrer quand necessaire
- Utiliser `dev_health_check` (outil MCP) pour voir l'etat de tous les services et ports

### Demarrer le mode dev

```bash
# Frontend (React/Vue/Svelte via Vite)
systemctl start vite-dev

# Backend Rust (recompilation automatique)
systemctl start cargo-dev

# Verifier l'etat
dev_health_check
```

### Arreter les services

```bash
systemctl stop vite-dev
systemctl stop cargo-dev
```

## Structure du workspace

```
/root/workspace/
├── Cargo.toml              # Workspace Rust (racine)
├── frontend/               # Code frontend (npm/Vite)
│   ├── package.json
│   ├── src/
│   └── dist/               # Build frontend (genere par npm run build)
├── src/ ou server/         # Code backend Rust
├── .dataverse/
│   └── app.db              # Base de donnees locale (DEV)
└── .claude/
    └── rules/              # Ces fichiers de regles
```

## Regles de developpement

1. **NE JAMAIS lancer `cargo build` ou `cargo run` manuellement** — utiliser les services systemd
2. **NE JAMAIS modifier les fichiers dans `/opt/app/`** — c'est le repertoire de production
3. **Modifier le code dans `/root/workspace/`** — le hot-reload recompile automatiquement
4. **Tester sur les URLs dev** (`dev.*` et `devapi.*`) avant tout deploiement
5. **NE JAMAIS deployer sans demande explicite de l'utilisateur**

## Outils MCP de verification

| Outil | Description |
|-------|-------------|
| `dev_health_check` | Etat des 3 services + ports (13337, 5173, 3000) |
| `dev_test_endpoint` | Test HTTP d'un endpoint local (GET/POST, status attendu) |
| `dev_test_browser` | Capture d'ecran d'une page via Chromium headless (base64 PNG) |

### Utilisation

```
# Verifier que tout tourne
dev_health_check

# Tester un endpoint API
dev_test_endpoint url="http://localhost:3000/api/health" expected_status=200

# Capturer le rendu visuel du frontend
dev_test_browser url="http://localhost:5173" width=1280 height=720
```

## Workflow de developpement standard

1. **Demarrer les services** : `systemctl start vite-dev cargo-dev`
2. **Verifier** : `dev_health_check` — tous les services doivent etre `ACTIVE`
3. **Coder** : modifier les fichiers dans `/root/workspace/`
4. **Tester** : `dev_test_endpoint` sur les endpoints modifies
5. **Verifier visuellement** : `dev_test_browser` pour le rendu frontend
6. **Iterer** : les modifications sont appliquees automatiquement via hot-reload
7. **Deployer** : uniquement sur demande utilisateur, via `deploy_app`

## Resolution de problemes

| Probleme | Solution |
|----------|----------|
| Port 5173 CLOSED | `systemctl start vite-dev` puis `systemctl status vite-dev` |
| Port 3000 CLOSED | `systemctl start cargo-dev` puis `systemctl status cargo-dev` |
| Hot-reload ne fonctionne pas | `systemctl restart vite-dev` ou `systemctl restart cargo-dev` |
| Erreur de compilation Rust | Verifier les logs : `journalctl -u cargo-dev -n 50` |
| Erreur npm/Vite | Verifier les logs : `journalctl -u vite-dev -n 50` |
| code-server inaccessible | `systemctl restart code-server` |
