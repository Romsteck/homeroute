use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use hr_db::engine::DataverseEngine;
use hr_db::query::{Filter, Pagination, query_rows};
use hr_db::schema::{ColumnDefinition, FieldType};

/// Schema description for a single SQLite table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<TableColumn>,
    pub row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub unique: bool,
}

impl From<&ColumnDefinition> for TableColumn {
    fn from(c: &ColumnDefinition) -> Self {
        Self {
            name: c.name.clone(),
            field_type: c.field_type.clone(),
            required: c.required,
            unique: c.unique,
        }
    }
}

/// Result of a SELECT query: column names + rows as JSON values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
    pub total: u64,
}

/// Per-app SQLite database manager. One database per app at `{apps_root}/{slug}/db.sqlite`.
#[derive(Clone)]
pub struct DbManager {
    apps_root: PathBuf,
    engines: Arc<RwLock<HashMap<String, Arc<Mutex<DataverseEngine>>>>>,
}

impl DbManager {
    pub fn new(apps_root: impl Into<PathBuf>) -> Self {
        let apps_root = apps_root.into();
        info!(path = %apps_root.display(), "DbManager initialized");
        Self {
            apps_root,
            engines: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Resolve the SQLite file path for an app.
    pub async fn db_path(&self, slug: &str) -> PathBuf {
        self.apps_root.join(slug).join("db.sqlite")
    }

    /// List all user-defined tables (excluding `_dv_*` metadata tables).
    pub async fn list_tables(&self, slug: &str) -> Result<Vec<String>> {
        let engine = self.get_engine(slug).await?;
        let engine = engine.lock().await;
        let schema = engine.get_schema().map_err(|e| anyhow!("{e}"))?;
        Ok(schema.tables.into_iter().map(|t| t.name).collect())
    }

    /// Describe a single table (columns + row count).
    pub async fn describe_table(&self, slug: &str, table: &str) -> Result<TableSchema> {
        let engine = self.get_engine(slug).await?;
        let engine = engine.lock().await;
        let table_def = engine
            .get_table(table)
            .map_err(|e| anyhow!("{e}"))?
            .ok_or_else(|| anyhow!("table not found: {table}"))?;
        let row_count = engine.count_rows(table).unwrap_or(0);
        Ok(TableSchema {
            name: table_def.name,
            columns: table_def.columns.iter().map(TableColumn::from).collect(),
            row_count,
        })
    }

    /// Run a parameterised SELECT against the app DB.
    /// `sql` should be a SELECT statement; `params` are bound positionally.
    pub async fn query(&self, slug: &str, sql: &str, params: Vec<Value>) -> Result<QueryResult> {
        let trimmed = sql.trim_start();
        if !trimmed.to_lowercase().starts_with("select") {
            return Err(anyhow!("only SELECT statements are allowed"));
        }
        let engine = self.get_engine(slug).await?;
        let engine = engine.lock().await;
        let conn = engine.connection();

        let mut stmt = conn.prepare(sql).map_err(|e| anyhow!("prepare: {e}"))?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let bound: Vec<Box<dyn rusqlite::types::ToSql>> = params.iter().map(json_to_sql).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = bound.iter().map(|b| b.as_ref()).collect();

        let names_for_map = column_names.clone();
        let mapped = stmt
            .query_map(rusqlite::params_from_iter(refs.iter()), move |row| {
                let mut obj = serde_json::Map::new();
                for (i, name) in names_for_map.iter().enumerate() {
                    obj.insert(name.clone(), sqlite_value_to_json(row, i));
                }
                Ok(Value::Object(obj))
            })
            .map_err(|e| anyhow!("query: {e}"))?;

        let mut rows = Vec::new();
        for row in mapped {
            rows.push(row.map_err(|e| anyhow!("row: {e}"))?);
        }
        let total = rows.len() as u64;
        Ok(QueryResult {
            columns: column_names,
            rows,
            total,
        })
    }

    /// Execute a mutation (INSERT/UPDATE/DELETE) against the app DB.
    /// Returns the number of affected rows.
    pub async fn execute(&self, slug: &str, sql: &str, params: Vec<Value>) -> Result<usize> {
        let engine = self.get_engine(slug).await?;
        let engine = engine.lock().await;
        let conn = engine.connection();

        let bound: Vec<Box<dyn rusqlite::types::ToSql>> = params.iter().map(json_to_sql).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = bound.iter().map(|b| b.as_ref()).collect();

        let rows_affected = conn
            .execute(sql, rusqlite::params_from_iter(refs.iter()))
            .map_err(|e| anyhow!("execute: {e}"))?;
        info!(app_slug = slug, rows_affected, "DB execute ok");
        Ok(rows_affected)
    }

    /// Take a timestamped snapshot copy of the app DB.
    /// Returns the backup file path.
    pub async fn snapshot(&self, slug: &str) -> Result<PathBuf> {
        let db_path = self.db_path(slug).await;
        if !db_path.exists() {
            return Err(anyhow!("no database for app '{slug}'"));
        }

        {
            let engine = self.get_engine(slug).await?;
            let engine = engine.lock().await;
            engine
                .connection()
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| anyhow!("wal checkpoint: {e}"))?;
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_path = db_path.with_file_name(format!("db.sqlite.bak.{timestamp}"));
        tokio::fs::copy(&db_path, &backup_path)
            .await
            .with_context(|| format!("copy {} -> {}", db_path.display(), backup_path.display()))?;
        info!(app_slug = slug, backup = %backup_path.display(), "snapshot created");
        Ok(backup_path)
    }

    /// Convenience: list rows from a table with filters/pagination.
    pub async fn select_rows(
        &self,
        slug: &str,
        table: &str,
        filters: &[Filter],
        pagination: &Pagination,
    ) -> Result<QueryResult> {
        let engine = self.get_engine(slug).await?;
        let engine = engine.lock().await;
        let rows = query_rows(engine.connection(), table, filters, pagination)
            .map_err(|e| anyhow!("{e}"))?;
        let columns = rows
            .first()
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let total = engine.count_rows(table).unwrap_or(0);
        Ok(QueryResult {
            columns,
            rows,
            total,
        })
    }

    async fn get_engine(&self, slug: &str) -> Result<Arc<Mutex<DataverseEngine>>> {
        {
            let engines = self.engines.read().await;
            if let Some(e) = engines.get(slug) {
                return Ok(e.clone());
            }
        }
        let db_path = self.db_path(slug).await;
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let engine =
            DataverseEngine::open(&db_path).map_err(|e| anyhow!("open db for '{slug}': {e}"))?;
        let engine = Arc::new(Mutex::new(engine));
        let mut engines = self.engines.write().await;
        engines.insert(slug.to_string(), engine.clone());
        info!(app_slug = slug, path = %db_path.display(), "db opened");
        Ok(engine)
    }
}

fn json_to_sql(v: &Value) -> Box<dyn rusqlite::types::ToSql> {
    match v {
        Value::Null => Box::new(rusqlite::types::Null),
        Value::Bool(b) => Box::new(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        Value::String(s) => Box::new(s.clone()),
        other => Box::new(other.to_string()),
    }
}

fn sqlite_value_to_json(row: &rusqlite::Row<'_>, idx: usize) -> Value {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx) {
        Ok(ValueRef::Null) => Value::Null,
        Ok(ValueRef::Integer(i)) => Value::from(i),
        Ok(ValueRef::Real(f)) => Value::from(f),
        Ok(ValueRef::Text(t)) => Value::from(String::from_utf8_lossy(t).to_string()),
        Ok(ValueRef::Blob(b)) => Value::from(format!("<blob {} bytes>", b.len())),
        Err(_) => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn db_path_layout() {
        let dir = TempDir::new().unwrap();
        let mgr = DbManager::new(dir.path().to_path_buf());
        let p = mgr.db_path("trader").await;
        assert_eq!(p, dir.path().join("trader").join("db.sqlite"));
    }

    #[tokio::test]
    async fn open_and_list_tables_empty() {
        let dir = TempDir::new().unwrap();
        let mgr = DbManager::new(dir.path().to_path_buf());
        let tables = mgr.list_tables("trader").await.unwrap();
        assert!(tables.is_empty());
    }

    #[tokio::test]
    async fn snapshot_creates_backup() {
        let dir = TempDir::new().unwrap();
        let mgr = DbManager::new(dir.path().to_path_buf());
        // open to create the file
        mgr.list_tables("trader").await.unwrap();
        let backup = mgr.snapshot("trader").await.unwrap();
        assert!(backup.exists());
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("db.sqlite.bak.")
        );
    }

    #[tokio::test]
    async fn query_rejects_non_select() {
        let dir = TempDir::new().unwrap();
        let mgr = DbManager::new(dir.path().to_path_buf());
        let res = mgr.query("trader", "DELETE FROM x", vec![]).await;
        assert!(res.is_err());
    }
}
