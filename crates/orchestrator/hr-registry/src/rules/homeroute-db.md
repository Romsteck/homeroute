# Database

Ce workspace ({{slug}}) dispose d'une base de donnees geree par HomeRoute.

## Architecture

La base de donnees est centralisee sur le routeur HomeRoute, geree par hr-orchestrator.
Chaque application a sa propre base SQLite isolee.

Les outils MCP sont accessibles via le serveur `homeroute` (HTTP).

## Outils MCP (via homeroute)

Tous les outils necessitent le parametre `app_id` (slug de l'app, ex: "{{slug}}").

### Consultation
- `db.list_tables` — Lister les tables avec nombre de lignes
- `db.describe_table` — Schema complet d'une table
- `db.get_schema` — Schema complet de la DB
- `db.get_db_info` — Statistiques (taille, version)
- `db.count_rows` — Compter les lignes
- `db.overview` — Vue globale de toutes les apps

### Schema (DDL)
- `db.create_table` — Creer une table (id, created_at, updated_at automatiques)
- `db.add_column` / `db.remove_column` — Modifier la structure
- `db.create_relation` — Creer une relation (FK)
- `db.drop_table` — Supprimer une table (confirm=true)

### Donnees (CRUD)
- `db.query_data` — Lire avec filtres et pagination
- `db.insert_data` — Inserer des lignes
- `db.update_data` — Modifier selon filtres
- `db.delete_data` — Supprimer selon filtres

## Regles
- TOUJOURS utiliser les outils MCP db.* pour interagir avec la base de donnees
- JAMAIS modifier les fichiers SQLite directement
- Les modifications sont immediates (pas de distinction DEV/PROD pour la DB)
