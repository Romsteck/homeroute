# Environnement de Developpement — Stack Next.js

## URLs importantes

| Service | URL |
|---------|-----|
| IDE (code-server) | `https://code.{{slug}}.{{domain}}` |
| Application DEV (Next.js) | `https://dev.{{slug}}.{{domain}}` |
| Studio (cet agent) | `https://studio.{{slug}}.{{domain}}` |
| Production | `https://{{slug}}.{{domain}}` |

## Structure du workspace

```
/root/workspace/
├── package.json          # Config Next.js + dependances
├── next.config.js        # Config Next.js (ou .ts)
├── app/                  # App Router (ou pages/ pour Pages Router)
├── public/               # Assets statiques
├── .env.local            # Variables d'environnement locales (non commitees)
├── .dataverse/           # Base SQLite locale (ignoree par git)
└── .claude/              # Config agents (rules, config, mcp)
```

## Services systemd dans le container

| Service | Role | Port |
|---------|------|------|
| `code-server.service` | IDE Visual Studio Code | 13337 |
| `nextjs-dev.service` | Serveur de developpement Next.js | 3000 |

**Note** : Next.js gere frontend ET routes API dans le meme process (pas de service Rust separe).

## Regles de developpement

1. **Ne jamais** modifier les fichiers dans `/opt/app/` (reserve a la production)
2. **Toujours** travailler dans `/root/workspace/`
3. **Tester** sur `https://dev.{{slug}}.{{domain}}` (hot-reload automatique)
4. **Ne jamais** deployer en production sans demande explicite
5. **Routes API** : creer dans `app/api/` (App Router) ou `pages/api/` (Pages Router)

## Gestion des services

```bash
# Demarrer le serveur Next.js dev
systemctl start nextjs-dev

# Verifier le statut
systemctl status nextjs-dev

# Voir les logs en temps reel
journalctl -u nextjs-dev -f

# Redemarrer apres changement de config
systemctl restart nextjs-dev
```

## Variables d'environnement

- **Build-time** (exposees au browser) : prefixe `NEXT_PUBLIC_`
- **Runtime-only** (serveur uniquement) : sans prefixe
- Fichier local : `/root/workspace/.env.local` (non commite)

## Git et deploiement

- Remote git : `http://10.0.0.254:4000/api/git/repos/{{slug}}.git`
- Push declenche la synchronisation avec le repo production
- Ne pas commiter `.env.local`, `.next/`, `node_modules/`

## Outils MCP disponibles

- **dev_health_check** : verifie que le serveur Next.js repond
- **dev_test_endpoint** : teste un endpoint API
- **deploy_app** : deploie en production (sur demande explicite seulement)
- **prod_status**, **prod_logs** : monitoring production

## Workflow standard

1. `systemctl status nextjs-dev` — verifier que le serveur tourne
2. Modifier le code dans `/root/workspace/`
3. Hot-reload automatique — tester sur `https://dev.{{slug}}.{{domain}}`
4. Commiter les changements avec git
5. Deployer en production uniquement sur demande explicite
