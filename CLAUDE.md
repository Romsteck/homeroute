# Notes pour Claude Code

## Gestion du serveur

- **NE PAS utiliser PM2** - L'utilisateur gère le serveur lui-même
- Ne pas essayer de démarrer/redémarrer l'API automatiquement
- Le build frontend (`npm run build`) peut être lancé, mais pas le démarrage des services

## Architecture

- **Frontend**: React + Vite dans `/web`
- **Backend**: Express.js dans `/api`
- Les fichiers buildés du frontend vont dans `/web/dist`

## Reverse Proxy (Caddy)

- Utilise uniquement des certificats individuels Let's Encrypt (HTTP challenge)
- Pas de wildcard certificate
- Le domaine de base sert uniquement de raccourci pour les sous-domaines
- Caddy API sur `localhost:2019`

## Commandes utiles

```bash
# Build frontend
cd /ssd_pool/server-dashboard/web && npm run build

# Test import backend
cd /ssd_pool/server-dashboard/api && node -e "import('./src/index.js')"
```
