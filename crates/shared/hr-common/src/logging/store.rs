use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use rusqlite::params;
use tokio::sync::broadcast;

use super::query::{LogQuery, LogStats};
use super::ring_buffer::RingBuffer;
use super::types::*;

/// Builder for creating a LogEntry before an ID is assigned.
pub struct LogEntryBuilder {
    pub service: LogService,
    pub level: LogLevel,
    pub category: LogCategory,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub request_id: Option<String>,
    pub user_id: Option<String>,
    pub source: LogSource,
}

pub struct LogStore {
    hot: Arc<RwLock<RingBuffer>>,
    db: Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    next_id: Arc<AtomicU64>,
    log_tx: broadcast::Sender<LogEntry>,
}

impl LogStore {
    pub fn new(db_path: &Path) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                service TEXT NOT NULL,
                level TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                data TEXT,
                request_id TEXT,
                user_id TEXT,
                crate_name TEXT NOT NULL,
                module TEXT NOT NULL,
                function TEXT,
                file TEXT,
                line INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_logs_ts ON logs(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
            CREATE INDEX IF NOT EXISTS idx_logs_service ON logs(service);
            CREATE INDEX IF NOT EXISTS idx_logs_crate ON logs(crate_name);
            CREATE INDEX IF NOT EXISTS idx_logs_request_id ON logs(request_id);
            ",
        )?;

        // Recover next_id from existing data
        let max_id: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
            row.get(0)
        })?;
        let max_id = max_id as u64;

        let (log_tx, _) = broadcast::channel(512);

        Ok(Self {
            hot: Arc::new(RwLock::new(RingBuffer::new())),
            db: Arc::new(tokio::sync::Mutex::new(conn)),
            next_id: Arc::new(AtomicU64::new(max_id + 1)),
            log_tx,
        })
    }

    /// Push a new log entry. This is synchronous (called from tracing Layer).
    pub fn push(&self, builder: LogEntryBuilder) -> LogEntry {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            service: builder.service,
            level: builder.level,
            category: builder.category,
            message: builder.message,
            data: builder.data,
            request_id: builder.request_id,
            user_id: builder.user_id,
            source: builder.source,
        };

        // Push into ring buffer
        if let Ok(mut hot) = self.hot.write() {
            hot.push(entry.clone());
        }

        // Broadcast (ignore errors — no subscribers is fine)
        let _ = self.log_tx.send(entry.clone());

        entry
    }

    /// Flush unflushed entries from ring buffer to SQLite.
    pub async fn flush_to_db(&self) -> anyhow::Result<()> {
        let entries = {
            let mut hot = self
                .hot
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {e}"))?;
            hot.drain_since_flush()
        };

        if entries.is_empty() {
            return Ok(());
        }

        let db = self.db.lock().await;
        let tx = db.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO logs (id, timestamp, service, level, category, message, data, request_id, user_id, crate_name, module, function, file, line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;

            for entry in &entries {
                let data_json = entry.data.as_ref().map(|v| v.to_string());
                let line = entry.source.line.map(|l| l as i64);
                stmt.execute(params![
                    entry.id as i64,
                    entry.timestamp.to_rfc3339(),
                    entry.service.as_str(),
                    entry.level.as_str(),
                    entry.category.as_str(),
                    entry.message,
                    data_json,
                    entry.request_id,
                    entry.user_id,
                    entry.source.crate_name,
                    entry.source.module,
                    entry.source.function,
                    entry.source.file,
                    line,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Compact old log entries based on retention rules.
    pub async fn compact(&self) -> anyhow::Result<()> {
        let db = self.db.lock().await;
        let now = Utc::now();

        // > 1 year: delete everything
        let one_year_ago = now - chrono::Duration::days(365);
        db.execute(
            "DELETE FROM logs WHERE timestamp < ?1",
            params![one_year_ago.to_rfc3339()],
        )?;

        // > 6 months: delete everything except ERROR
        let six_months_ago = now - chrono::Duration::days(180);
        db.execute(
            "DELETE FROM logs WHERE timestamp < ?1 AND level != 'error'",
            params![six_months_ago.to_rfc3339()],
        )?;

        // > 30 days: delete TRACE, DEBUG, INFO
        let thirty_days_ago = now - chrono::Duration::days(30);
        db.execute(
            "DELETE FROM logs WHERE timestamp < ?1 AND level IN ('trace', 'debug', 'info')",
            params![thirty_days_ago.to_rfc3339()],
        )?;

        Ok(())
    }

    /// Query logs from SQLite with filters.
    pub async fn query(&self, filter: &LogQuery) -> anyhow::Result<Vec<LogEntry>> {
        let db = self.db.lock().await;

        let mut sql = String::from(
            "SELECT id, timestamp, service, level, category, message, data, request_id, user_id, crate_name, module, function, file, line FROM logs WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1u32;

        if let Some(ref levels) = filter.level {
            if !levels.is_empty() {
                let placeholders: Vec<String> = levels
                    .iter()
                    .map(|_| {
                        let p = format!("?{idx}");
                        idx += 1;
                        p
                    })
                    .collect();
                sql.push_str(&format!(" AND level IN ({})", placeholders.join(",")));
                for l in levels {
                    param_values.push(Box::new(l.as_str().to_string()));
                }
            }
        }

        if let Some(ref services) = filter.service {
            if !services.is_empty() {
                let placeholders: Vec<String> = services
                    .iter()
                    .map(|_| {
                        let p = format!("?{idx}");
                        idx += 1;
                        p
                    })
                    .collect();
                sql.push_str(&format!(" AND service IN ({})", placeholders.join(",")));
                for s in services {
                    param_values.push(Box::new(s.as_str().to_string()));
                }
            }
        }

        if let Some(ref categories) = filter.category {
            if !categories.is_empty() {
                let placeholders: Vec<String> = categories
                    .iter()
                    .map(|_| {
                        let p = format!("?{idx}");
                        idx += 1;
                        p
                    })
                    .collect();
                sql.push_str(&format!(" AND category IN ({})", placeholders.join(",")));
                for c in categories {
                    param_values.push(Box::new(c.as_str().to_string()));
                }
            }
        }

        if let Some(ref crate_name) = filter.crate_name {
            sql.push_str(&format!(" AND crate_name = ?{idx}"));
            idx += 1;
            param_values.push(Box::new(crate_name.clone()));
        }

        if let Some(ref module) = filter.module {
            sql.push_str(&format!(" AND module LIKE ?{idx}"));
            idx += 1;
            param_values.push(Box::new(format!("%{module}%")));
        }

        if let Some(ref source) = filter.source {
            sql.push_str(&format!(
                " AND (crate_name LIKE ?{} OR module LIKE ?{} OR function LIKE ?{})",
                idx,
                idx + 1,
                idx + 2
            ));
            idx += 3;
            let pat = format!("%{source}%");
            param_values.push(Box::new(pat.clone()));
            param_values.push(Box::new(pat.clone()));
            param_values.push(Box::new(pat));
        }

        if let Some(ref q) = filter.q {
            sql.push_str(&format!(" AND message LIKE ?{idx}"));
            idx += 1;
            param_values.push(Box::new(format!("%{q}%")));
        }

        if let Some(ref request_id) = filter.request_id {
            sql.push_str(&format!(" AND request_id = ?{idx}"));
            idx += 1;
            param_values.push(Box::new(request_id.clone()));
        }

        if let Some(ref user_id) = filter.user_id {
            sql.push_str(&format!(" AND user_id = ?{idx}"));
            idx += 1;
            param_values.push(Box::new(user_id.clone()));
        }

        if let Some(ref since) = filter.since {
            sql.push_str(&format!(" AND timestamp >= ?{idx}"));
            idx += 1;
            param_values.push(Box::new(since.to_rfc3339()));
        }

        if let Some(ref until) = filter.until {
            sql.push_str(&format!(" AND timestamp <= ?{idx}"));
            idx += 1;
            param_values.push(Box::new(until.to_rfc3339()));
        }

        sql.push_str(" ORDER BY id DESC");

        let limit = filter.limit.unwrap_or(100).min(1000);
        let offset = filter.offset.unwrap_or(0);
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));

        let _ = idx; // suppress unused warning

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let ts_str: String = row.get(1)?;
            let service_str: String = row.get(2)?;
            let level_str: String = row.get(3)?;
            let category_str: String = row.get(4)?;
            let data_str: Option<String> = row.get(6)?;
            let line: Option<i64> = row.get(13)?;

            let timestamp: DateTime<Utc> = ts_str.parse().unwrap_or_else(|_| Utc::now());

            Ok(LogEntry {
                id: id as u64,
                timestamp,
                service: service_str.parse().unwrap_or(LogService::Homeroute),
                level: level_str.parse().unwrap_or(LogLevel::Info),
                category: category_str.parse().unwrap_or(LogCategory::System),
                message: row.get(5)?,
                data: data_str.and_then(|s| serde_json::from_str(&s).ok()),
                request_id: row.get(7)?,
                user_id: row.get(8)?,
                source: LogSource {
                    crate_name: row.get(9)?,
                    module: row.get(10)?,
                    function: row.get(11)?,
                    file: row.get(12)?,
                    line: line.map(|l| l as u32),
                },
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get statistics about the log store.
    pub async fn stats(&self) -> anyhow::Result<LogStats> {
        let db = self.db.lock().await;

        let total: i64 = db.query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))?;
        let total = total as u64;

        let mut by_level = std::collections::HashMap::new();
        {
            let mut stmt = db.prepare("SELECT level, COUNT(*) FROM logs GROUP BY level")?;
            let rows = stmt.query_map([], |row| {
                let level: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((level, count as u64))
            })?;
            for row in rows {
                let (level, count) = row?;
                by_level.insert(level, count);
            }
        }

        let mut by_service = std::collections::HashMap::new();
        {
            let mut stmt = db.prepare("SELECT service, COUNT(*) FROM logs GROUP BY service")?;
            let rows = stmt.query_map([], |row| {
                let service: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((service, count as u64))
            })?;
            for row in rows {
                let (service, count) = row?;
                by_service.insert(service, count);
            }
        }

        // Get DB file size
        let db_size_bytes: i64 = db
            .query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let db_size_bytes = db_size_bytes as u64;

        let (hot_count, hot_capacity) = {
            let hot = self
                .hot
                .read()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {e}"))?;
            (hot.len(), hot.capacity())
        };

        Ok(LogStats {
            total,
            by_level,
            by_service,
            db_size_bytes,
            hot_count,
            hot_capacity,
        })
    }

    /// Subscribe to live log entries.
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.log_tx.subscribe()
    }
}
