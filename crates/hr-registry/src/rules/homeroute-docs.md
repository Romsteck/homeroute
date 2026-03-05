# Documentation App

L'application {{slug}} utilise un système de documentation intégré accessible via les outils MCP `docs`.

## Règles obligatoires

### Avant de modifier
- **TOUJOURS** appeler `get_docs` avant de créer ou modifier des écrans, features ou flux
- Lire la documentation existante pour comprendre le contexte et éviter les duplications
- Vérifier les `related_flows` et `related_tables` pour maintenir la cohérence

### Après modification
- **TOUJOURS** mettre à jour la documentation quand :
  - Un nouvel écran ou page est créé → `upsert_screen`
  - Un écran existant est modifié significativement → `upsert_screen`
  - Un écran est supprimé → `delete_screen`
  - Un nouveau flux utilisateur est implémenté → `upsert_flow`
  - Un flux est modifié → `upsert_flow`
  - Des tables Dataverse sont ajoutées/modifiées → mettre à jour `related_tables` des écrans concernés

### Style de documentation
- Descriptions **orientées utilisateur**, pas techniques
- Bon : "Page permettant aux utilisateurs de gérer leur profil et préférences"
- Mauvais : "Composant React avec useState qui fetch /api/users"
- Les features décrivent **ce que l'utilisateur peut faire**, pas l'implémentation
- Bon : "Filtrage des produits par catégorie et prix"
- Mauvais : "Composant FilterBar avec props onChange"

### App overview
Remplir `update_app_info` dès le début du projet :
- `name` : nom de l'application
- `description` : tagline courte (1 phrase)
- `business_context` : paragraphe expliquant le problème résolu et la valeur
- `target_users` : liste des personas cibles

## Outils MCP disponibles (serveur docs)

| Outil | Usage |
|-------|-------|
| `get_docs` | Lire la doc (section: all, app, screens, flows) |
| `update_app_info` | Modifier l'overview de l'app |
| `upsert_screen` | Créer ou mettre à jour un écran |
| `delete_screen` | Supprimer un écran |
| `upsert_flow` | Créer ou mettre à jour un flux |
| `delete_flow` | Supprimer un flux |
| `list_screens` | Lister les écrans (résumé) |
| `list_flows` | Lister les flux (résumé) |
