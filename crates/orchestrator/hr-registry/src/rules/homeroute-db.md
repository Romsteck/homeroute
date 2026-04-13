# Database

Ce workspace ({{slug}}) dispose d'une base de donnees geree par HomeRoute.

## Architecture

La base de donnees est centralisee sur le routeur HomeRoute, geree par hr-orchestrator.
Chaque application a sa propre base SQLite isolee.

Les outils MCP sont accessibles via le serveur `homeroute` (HTTP).

## Outils MCP (via homeroute)

Tous les outils necessitent le parametre `slug` (slug de l'app, ex: "{{slug}}").

### Consultation
- `db.tables` — Lister les tables
- `db.describe` — Schema d'une table (colonnes, relations, row count)
- `db.get_schema` — Schema complet de la DB (toutes les tables + relations)
- `db.overview` — Vue globale (nombre de tables, liste)
- `db.count_rows` — Compter les lignes d'une table

### Schema (DDL)
- `db.create_table` — Creer une table (id, created_at, updated_at automatiques)
- `db.add_column` / `db.remove_column` — Modifier la structure
- `db.create_relation` — Creer une relation (FK entre deux tables)
- `db.drop_table` — Supprimer une table
- `db.sync_schema` — Synchroniser les tables SQLite vers les metadonnees Dataverse

### Donnees (DML)
- `db.query` — Executer un SELECT avec params optionnels
- `db.execute` — Mutations (INSERT, UPDATE, DELETE) avec params

## Relations (foreign keys)

Les relations lient deux tables via une FK. Une fois creees, les queries peuvent
etre etendues automatiquement (LEFT JOIN) pour ramener les donnees liees.

Exemple de relation :
```json
{
  "from_table": "orders",
  "from_column": "user_id",
  "to_table": "users",
  "to_column": "id",
  "relation_type": "one_to_many",
  "cascade": { "on_delete": "restrict", "on_update": "cascade" }
}
```

Types supportes : `one_to_many`, `many_to_many`, `self_referential`.
Cascade actions : `cascade`, `set_null`, `restrict`.

## Regles
- TOUJOURS utiliser les outils MCP db.* pour interagir avec la base de donnees
- JAMAIS modifier les fichiers SQLite directement
- TOUJOURS declarer les relations (FK) via `db.create_relation` pour beneficier des JOINs automatiques
- Les modifications sont immediates (pas de distinction DEV/PROD pour la DB)
