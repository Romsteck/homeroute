# Dataverse Database

Ce workspace utilise Dataverse comme base de données. Le serveur MCP `dataverse` opère sur la **DB locale** de ce conteneur (`/root/workspace/.dataverse/app.db`).

## Architecture DB
- **DEV (ici)**: DB locale pour développer le schéma et tester les données (`/root/workspace/.dataverse/app.db`)
- **PROD**: DB de production avec les vraies données (`/opt/app/.dataverse/app.db`)
- Les modifications MCP affectent **uniquement la DB locale** — PROD n'est jamais modifiée directement
- Pour déployer le schéma vers PROD: utiliser `prod_push` avec le dossier `.dataverse/`

## Règles
- TOUJOURS utiliser les outils MCP Dataverse pour interagir avec la base de données locale
- JAMAIS modifier `/root/workspace/.dataverse/app.db` directement (pas de sqlite3 CLI, pas de scripts SQL ad-hoc)
- JAMAIS pousser la DB vers PROD sans que l'utilisateur l'ait explicitement demandé
- Le code applicatif runtime (sur PROD) utilise une librairie SQLite connectée à `/opt/app/.dataverse/app.db`

## Outils disponibles

### Consultation
- `list_tables` — Lister toutes les tables avec nombre de lignes
- `describe_table` — Schéma complet d'une table (colonnes, types, contraintes)
- `get_schema` — Schéma complet de la DB (tables, colonnes, relations)
- `get_db_info` — Statistiques (taille, nombre de tables, total lignes)
- `count_rows` — Compter les lignes avec filtres optionnels

### Schéma (DDL)
- `create_table` — Créer une table (id, created_at, updated_at sont automatiques)
- `add_column` / `remove_column` — Modifier la structure d'une table
- `create_relation` — Créer une relation entre tables (FK)
- `drop_table` — Supprimer une table (nécessite confirm=true)

### Données (CRUD)
- `query_data` — Lire des lignes avec filtres et pagination
- `insert_data` — Insérer des lignes
- `update_data` — Modifier des lignes selon filtres
- `delete_data` — Supprimer des lignes selon filtres

## Procédure
1. Avant toute opération, appeler `list_tables` pour voir l'état actuel
2. Utiliser `describe_table` pour comprendre le schéma avant de le modifier
3. Pour les changements de schéma, utiliser les outils DDL
4. Pour les opérations de données, utiliser les outils CRUD
5. Quand le schéma est prêt et que l'utilisateur le demande, déployer vers PROD avec `prod_push` (dossier `.dataverse/`)
