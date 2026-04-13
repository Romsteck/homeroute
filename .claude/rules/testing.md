# Testing — Règles obligatoires

## Principe

Après chaque fonctionnalité ou mise à jour déployée en production, **tester de bout en bout** tous les endpoints et processus associés. Ne jamais considérer une tâche comme terminée sans vérification.

## Après chaque deploy

### 1. Tester les endpoints créés ou modifiés

```bash
# Vérifier que le service répond
curl -s http://10.0.0.254:4000/api/health | jq

# Tester chaque endpoint touché avec des paramètres réels
curl -s http://10.0.0.254:4000/api/<route> | jq

# Vérifier les codes de retour (200, 201, 400, 404, 500)
curl -s -o /dev/null -w "%{http_code}" http://10.0.0.254:4000/api/<route>
```

### 2. Tester les processus associés

- **IPC** : si une route appelle l'orchestrator/edge/netcore, vérifier que la réponse IPC arrive
- **WebSocket** : si un event est ajouté, vérifier qu'il apparaît dans les logs WebSocket
- **Background tasks** : si un poller ou une tâche est modifiée, vérifier dans les logs qu'elle tourne
- **Frontend** : si une page est modifiée, vérifier que le build web passe (`make web`)

### 3. Vérifier les logs

```bash
curl -s 'http://10.0.0.254:4000/api/logs?limit=10' | jq '.logs[] | {level, service, message}'
```

Les nouveaux endpoints doivent apparaître dans les logs. Les erreurs doivent être visibles.

## Protection des données existantes

### JAMAIS casser les données en place

- **Ne pas modifier** les fichiers de config en production pour tester (`dns-dhcp-config.json`, `reverseproxy-config.json`, etc.)
- **Ne pas supprimer** d'entrées existantes (containers, apps, DNS records, proxy rules)
- **Ne pas redémarrer** hr-edge ou hr-netcore sans raison — impact réseau immédiat

### Créer des données de test isolées

- Utiliser des noms explicitement temporaires : préfixe `_test-` ou `_tmp-`
- Exemples : `_test-app`, `_test-dns-record`, `_tmp-proxy-rule`
- Ne jamais réutiliser un slug ou un nom existant

### Nettoyer après les tests

- **Toujours** supprimer les données de test après vérification
- Vérifier que la suppression n'a pas d'effet de bord
- Confirmer avec un dernier appel API que l'état est propre

### Cas particuliers

| Ressource | Comment tester sans impact |
|-----------|---------------------------|
| DNS records | Créer un record `_test-xxx`, vérifier, supprimer |
| Proxy rules | Créer une règle vers un domaine inexistant, vérifier, supprimer |
| Containers/envs | Ne PAS créer de container de test — trop lourd, tester via l'API read-only |
| DB/SQLite | Lire seulement, ne jamais insérer/modifier des données de prod |
| Certificats ACME | Ne PAS déclencher de renouvellement — rate limits Let's Encrypt |
| Services systemd | Ne PAS restart sauf si c'est le service qu'on déploie |

## Quand tester manuellement vs automatiquement

| Situation | Action |
|-----------|--------|
| Nouvel endpoint API | `curl` + vérifier response body + status code |
| Modification d'un endpoint | `curl` avec les mêmes paramètres qu'avant + les nouveaux |
| Nouveau WebSocket event | Ouvrir la page concernée dans le browser, vérifier les logs |
| Modification IPC | Vérifier via l'API qui appelle l'IPC (pas d'accès direct au socket) |
| Modification frontend | `make web` doit passer, vérifier visuellement si critique |
| Background task | Vérifier dans les logs après 1-2 cycles (5s pour flush, 60s pour pollers) |

## Résumé dans le message final

Après les tests, inclure dans le résumé :
- Liste des endpoints testés avec résultat (OK/KO)
- Données de test créées et confirmées supprimées
- Erreurs ou warnings observés dans les logs