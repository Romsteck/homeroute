---
name: logs
description: Affiche les logs de production HomeRoute — filtrage par service, niveau, ou recherche
disable-model-invocation: true
argument-hint: "[service|error|tail|query]"
allowed-tools: Bash(ssh *) Bash(curl *)
---

# Logs Production HomeRoute

Tu es le skill de consultation des logs HomeRoute en production (10.0.0.20).

## Argument : `$ARGUMENTS`

Interprète l'argument passé par l'utilisateur :

| Argument | Action |
|---|---|
| _(vide)_ | Derniers 20 logs tous services |
| `error` ou `errors` | Derniers logs niveau error/warn |
| `edge` | Logs du service hr-edge |
| `orchestrator` ou `orch` | Logs du service hr-orchestrator |
| `netcore` ou `dns` ou `dhcp` | Logs du service hr-netcore |
| `api` ou `homeroute` | Logs du service homeroute |
| `tail` ou `follow` ou `live` | Stream live (journalctl -f) des 4 services |
| `tail edge`, `tail api`, etc. | Stream live d'un service spécifique |
| Autre texte | Recherche dans les logs récents |

## Commandes

### Logs via API (préféré pour consultation)

```bash
# Derniers logs
curl -s 'http://10.0.0.20:4000/api/logs?limit=20' | python3 -m json.tool

# Filtré par service
curl -s 'http://10.0.0.20:4000/api/logs?limit=20&service=hr-edge' | python3 -m json.tool

# Filtré par niveau
curl -s 'http://10.0.0.20:4000/api/logs?limit=20&level=error' | python3 -m json.tool

# Recherche
curl -s 'http://10.0.0.20:4000/api/logs?limit=20&search=QUERY' | python3 -m json.tool
```

### Logs live via SSH (pour tail/follow)

```bash
# Tous les services
ssh romain@10.0.0.20 'sudo journalctl -u homeroute -u hr-edge -u hr-orchestrator -u hr-netcore -f --no-pager -n 30'

# Un service spécifique
ssh romain@10.0.0.20 'sudo journalctl -u SERVICE -f --no-pager -n 30'
```

## Présentation

- Afficher les logs de manière **lisible** : timestamp, service, niveau, message
- Mettre en évidence les `error` et `warn`
- Si beaucoup de logs, résumer les patterns (ex: "15 requêtes DNS normales, 2 erreurs proxy")
- Pour le mode `tail`/`follow`, utiliser le tool Monitor ou Bash avec timeout raisonnable (10s)

## Règles

- Ne JAMAIS modifier quoi que ce soit sur le serveur — lecture seule
- Ne JAMAIS redémarrer de services
- Prod host : `romain@10.0.0.20`
- API prod : `http://10.0.0.20:4000`
