# Dataverse Database

Ce workspace ({{slug}}) utilise Dataverse comme base de donnees.

## Architecture DB
- **DEV (ici)**: DB locale (`/root/workspace/.dataverse/app.db`)
- **PROD ({{slug}}.{{domain}})**: DB de production (`/opt/app/.dataverse/app.db`)
- Les modifications MCP affectent **uniquement la DB locale** — PROD n'est jamais modifiee directement

## Regles
- TOUJOURS utiliser les outils MCP Dataverse pour interagir avec la base de donnees locale
- JAMAIS modifier `/root/workspace/.dataverse/app.db` directement
- JAMAIS pousser la DB vers PROD avec `prod_push .dataverse/` — utiliser `migrate_schema` a la place
- Le code applicatif runtime utilise une librairie SQLite connectee a `/opt/app/.dataverse/app.db`

## Outils schema (via MCP deploy)
- `prod_schema` — Afficher le schema PROD actuel (lecture seule)
- `schema_diff` — Comparer schema DEV vs PROD (tables, colonnes, types)
- `migrate_schema` — Appliquer les modifications de schema DEV → PROD (safe, sans toucher aux donnees)

## Outils locaux (via MCP dataverse)

### Consultation
- `list_tables` — Lister les tables avec nombre de lignes
- `describe_table` — Schema complet d'une table
- `get_schema` — Schema complet de la DB
- `get_db_info` — Statistiques
- `count_rows` — Compter les lignes

### Schema (DDL)
- `create_table` — Creer une table (id, created_at, updated_at automatiques)
- `add_column` / `remove_column` — Modifier la structure
- `create_relation` — Creer une relation (FK)
- `drop_table` — Supprimer une table (confirm=true)

### Donnees (CRUD)
- `query_data` — Lire avec filtres et pagination
- `insert_data` — Inserer des lignes
- `update_data` — Modifier selon filtres
- `delete_data` — Supprimer selon filtres

## Procedure de migration vers PROD
1. Developper le schema localement avec les outils DDL
2. Tester avec des donnees de dev via les outils CRUD
3. `schema_diff` — Voir les differences avec PROD
4. `migrate_schema` — Appliquer les modifications (ou `--dry_run` pour previsualiser)
5. Verifier sur PROD via `prod_schema`
