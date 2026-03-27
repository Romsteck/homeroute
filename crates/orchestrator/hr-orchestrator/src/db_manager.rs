//! Centralized database manager for all HomeRoute applications.
//!
//! Each app gets its own SQLite database at `{db_dir}/{app_id}.db`.
//! Engines are lazily opened and cached.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use hr_db::engine::DataverseEngine;
use hr_db::query::*;
use hr_db::schema::*;
use hr_registry::protocol::DataverseQueryRequest;

pub struct DbManager {
    engines: RwLock<HashMap<String, Arc<Mutex<DataverseEngine>>>>,
    db_dir: PathBuf,
}

impl DbManager {
    pub fn new(db_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&db_dir).ok();
        info!(path = %db_dir.display(), "DbManager initialized");
        Self {
            engines: RwLock::new(HashMap::new()),
            db_dir,
        }
    }

    /// Get or lazily open the engine for an app.
    pub async fn get_engine(&self, app_id: &str) -> Result<Arc<Mutex<DataverseEngine>>, String> {
        // Fast path: already cached
        {
            let engines = self.engines.read().await;
            if let Some(engine) = engines.get(app_id) {
                return Ok(engine.clone());
            }
        }

        // Slow path: open and cache
        let db_path = self.db_dir.join(format!("{}.db", app_id));
        let engine = DataverseEngine::open(&db_path)
            .map_err(|e| format!("Failed to open database for '{}': {}", app_id, e))?;
        let engine = Arc::new(Mutex::new(engine));

        let mut engines = self.engines.write().await;
        engines.insert(app_id.to_string(), engine.clone());
        info!(app_id, path = %db_path.display(), "Database opened");
        Ok(engine)
    }

    /// Execute a DataverseQueryRequest against an app's database.
    pub async fn execute_query(
        &self,
        app_id: &str,
        query: DataverseQueryRequest,
    ) -> Result<Value, String> {
        let engine = self.get_engine(app_id).await?;
        let engine = engine.lock().await;

        match query {
            DataverseQueryRequest::QueryRows {
                table_name,
                filters,
                limit,
                offset,
                order_by,
                order_desc,
            } => {
                let parsed_filters: Vec<Filter> = filters
                    .iter()
                    .filter_map(|f| serde_json::from_value(f.clone()).ok())
                    .collect();
                let pagination = Pagination {
                    limit,
                    offset,
                    order_by,
                    order_desc,
                };
                let rows =
                    query_rows(engine.connection(), &table_name, &parsed_filters, &pagination)
                        .map_err(|e| e.to_string())?;
                let total = engine.count_rows(&table_name).unwrap_or(0);
                Ok(json!({ "rows": rows, "total": total }))
            }
            DataverseQueryRequest::InsertRows { table_name, rows } => {
                let count =
                    insert_rows(engine.connection(), &table_name, &rows).map_err(|e| e.to_string())?;
                Ok(json!({ "inserted": count }))
            }
            DataverseQueryRequest::UpdateRows {
                table_name,
                updates,
                filters,
            } => {
                let parsed_filters: Vec<Filter> = filters
                    .iter()
                    .filter_map(|f| serde_json::from_value(f.clone()).ok())
                    .collect();
                let count =
                    update_rows(engine.connection(), &table_name, &updates, &parsed_filters)
                        .map_err(|e| e.to_string())?;
                Ok(json!({ "updated": count }))
            }
            DataverseQueryRequest::DeleteRows {
                table_name,
                filters,
            } => {
                let parsed_filters: Vec<Filter> = filters
                    .iter()
                    .filter_map(|f| serde_json::from_value(f.clone()).ok())
                    .collect();
                let count = delete_rows(engine.connection(), &table_name, &parsed_filters)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "deleted": count }))
            }
            DataverseQueryRequest::CountRows {
                table_name,
                filters,
            } => {
                if filters.is_empty() {
                    let count = engine.count_rows(&table_name).map_err(|e| e.to_string())?;
                    Ok(json!({ "count": count }))
                } else {
                    let parsed_filters: Vec<Filter> = filters
                        .iter()
                        .filter_map(|f| serde_json::from_value(f.clone()).ok())
                        .collect();
                    let rows = query_rows(
                        engine.connection(),
                        &table_name,
                        &parsed_filters,
                        &Pagination {
                            limit: u64::MAX,
                            ..Default::default()
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(json!({ "count": rows.len() }))
                }
            }
            DataverseQueryRequest::GetMigrations => {
                let rows = query_rows(
                    engine.connection(),
                    "_dv_migrations",
                    &[],
                    &Pagination {
                        limit: 1000,
                        offset: 0,
                        order_by: Some("id".to_string()),
                        order_desc: true,
                    },
                )
                .map_err(|e| e.to_string())?;
                Ok(json!({ "migrations": rows }))
            }
        }
    }

    /// Get the full schema for an app's database.
    pub async fn get_schema(&self, app_id: &str) -> Result<Value, String> {
        let db_path = self.db_dir.join(format!("{}.db", app_id));
        if !db_path.exists() {
            return Err(format!("No database for app '{}'", app_id));
        }

        let engine = self.get_engine(app_id).await?;
        let engine = engine.lock().await;
        let schema = engine.get_schema().map_err(|e| e.to_string())?;
        let db_size = DataverseEngine::db_size_bytes(&db_path);

        let tables: Vec<Value> = schema
            .tables
            .iter()
            .map(|t| {
                let row_count = engine.count_rows(&t.name).unwrap_or(0);
                json!({
                    "name": t.name,
                    "slug": t.slug,
                    "columns": t.columns.iter().map(|c| json!({
                        "name": c.name,
                        "field_type": serde_json::to_string(&c.field_type)
                            .unwrap_or_default().trim_matches('"'),
                        "required": c.required,
                        "unique": c.unique,
                        "choices": c.choices,
                    })).collect::<Vec<_>>(),
                    "row_count": row_count,
                })
            })
            .collect();

        let relations: Vec<Value> = schema
            .relations
            .iter()
            .map(|r| {
                json!({
                    "from_table": r.from_table,
                    "from_column": r.from_column,
                    "to_table": r.to_table,
                    "to_column": r.to_column,
                    "relation_type": serde_json::to_string(&r.relation_type)
                        .unwrap_or_default().trim_matches('"'),
                })
            })
            .collect();

        Ok(json!({
            "appId": app_id,
            "tables": tables,
            "relations": relations,
            "version": schema.version,
            "dbSizeBytes": db_size,
        }))
    }

    /// Get overview of all apps that have databases.
    pub async fn overview(&self) -> Result<Value, String> {
        let mut apps = Vec::new();

        let entries = std::fs::read_dir(&self.db_dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "db") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    match self.get_schema(stem).await {
                        Ok(schema) => apps.push(schema),
                        Err(_) => continue,
                    }
                }
            }
        }

        Ok(json!({ "apps": apps }))
    }

    /// List all app IDs that have databases.
    pub async fn list_apps(&self) -> Vec<String> {
        let mut apps = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.db_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "db") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        apps.push(stem.to_string());
                    }
                }
            }
        }
        apps
    }

    /// Get the database directory path.
    pub fn db_dir(&self) -> &Path {
        &self.db_dir
    }
}
