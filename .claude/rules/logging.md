# Logging — Règles obligatoires

## Principes

HomeRoute utilise un système de logging centralisé (`hr-common::logging`) avec stockage tiered (RAM → SQLite). Chaque log porte sa **provenance** (crate, module, fonction, fichier, ligne).

## Règles pour le code Rust

### Tout nouveau code DOIT logger

- **Entrée de handler API** : `info!("description de l'action")` avec contexte (params, user)
- **IPC calls** : `info!` avant l'appel, `warn!`/`error!` si échec, avec durée
- **Opérations d'écriture** : config changes, file writes, DB mutations → `info!` avec détails
- **Erreurs** : `error!` avec le contexte complet (quoi, pourquoi, quel input)
- **Cas limites/fallbacks** : `warn!` quand un fallback est utilisé ou un cas inattendu survient

### Utiliser les spans tracing pour la provenance

```rust
#[tracing::instrument(skip(state))]
async fn my_handler(State(state): State<ApiState>, ...) -> impl IntoResponse {
    info!("Démarrage de l'opération X");
    // ...
}
```

Le `#[tracing::instrument]` crée un span nommé automatiquement d'après la fonction, ce qui alimente le champ `function` dans LogSource. **Toujours** utiliser `skip()` pour les paramètres volumineux (state, body).

### Structured logging obligatoire

Utiliser les champs structurés tracing, pas juste des strings :
```rust
// BON
info!(method = %method, path = %path, status = status, duration_ms = elapsed, "HTTP request");

// MAUVAIS
info!("HTTP request {} {} -> {} in {}ms", method, path, status, elapsed);
```

Les champs structurés alimentent le champ `data` JSON du LogEntry et sont filtrables/cherchables.

### Niveaux de log

| Niveau | Usage |
|--------|-------|
| `error!` | Échec qui empêche l'opération de se terminer, perte de données, état corrompu |
| `warn!` | Situation inattendue mais gérée, fallback utilisé, timeout, retry |
| `info!` | Opération normale significative : requête HTTP, IPC call, changement de config, action utilisateur |
| `debug!` | Détails utiles pour le diagnostic : valeurs intermédiaires, branches de décision |
| `trace!` | Très verbeux : contenu complet des requêtes/réponses, itérations de boucle |

### Ne PAS logger

- Mots de passe, tokens, secrets — JAMAIS en clair dans les logs
- Contenu volumineux (body HTTP complet, fichiers) — utiliser `trace!` si vraiment nécessaire
- Boucles serrées (polling toutes les 2s) — seulement si changement de valeur

## Règles pour le code existant

Quand on modifie un handler ou une fonction existante, ajouter du logging si absent. Ne pas créer un PR séparé — le logging fait partie de chaque changement.

## Vérification après deploy

Après chaque `make deploy-*`, vérifier que les logs apparaissent :
```bash
curl -s http://10.0.0.254:4000/api/logs?limit=5 | jq
```