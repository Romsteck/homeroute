//! Per-app todos store (simple JSON file at `{apps_root}/{slug}/todos.json`).
//!
//! Live-updated via the `app_todos` broadcast channel on `EventBus` — the Studio
//! right-side panel subscribes through the WS API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use hr_common::events::{AppTodosEvent, EventBus};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, instrument, warn};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
}

impl TodoStatus {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "blocked" => Ok(Self::Blocked),
            other => Err(anyhow!("invalid todo status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: TodoStatus,
    #[serde(default)]
    pub status_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodosFile {
    #[serde(default)]
    pub todos: Vec<Todo>,
}

/// Per-app JSON todo store. One file per app at `{apps_root}/{slug}/todos.json`.
#[derive(Clone)]
pub struct TodosManager {
    apps_root: PathBuf,
    locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    events: Arc<EventBus>,
}

impl TodosManager {
    pub fn new(apps_root: impl Into<PathBuf>, events: Arc<EventBus>) -> Self {
        let apps_root = apps_root.into();
        info!(path = %apps_root.display(), "TodosManager initialized");
        Self {
            apps_root,
            locks: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    fn path_for(&self, slug: &str) -> PathBuf {
        self.apps_root.join(slug).join("todos.json")
    }

    async fn lock_for(&self, slug: &str) -> Arc<Mutex<()>> {
        {
            let guard = self.locks.read().await;
            if let Some(m) = guard.get(slug) {
                return m.clone();
            }
        }
        let mut guard = self.locks.write().await;
        guard
            .entry(slug.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn load(&self, slug: &str) -> Result<TodosFile> {
        let path = self.path_for(slug);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TodosFile::default()),
            Err(e) => Err(anyhow!("read {}: {e}", path.display())),
        }
    }

    async fn save(&self, slug: &str, file: &TodosFile) -> Result<()> {
        let path = self.path_for(slug);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let bytes = serde_json::to_vec_pretty(file)?;
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| anyhow!("write tmp {}: {e}", tmp.display()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| anyhow!("rename {}: {e}", path.display()))?;
        Ok(())
    }

    fn broadcast(&self, slug: &str, todos: &[Todo]) {
        let payload: Vec<serde_json::Value> = todos
            .iter()
            .map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null))
            .collect();
        let _ = self.events.app_todos.send(AppTodosEvent {
            slug: slug.to_string(),
            todos: payload,
        });
    }

    #[instrument(skip(self), fields(slug = %slug))]
    pub async fn list(&self, slug: &str, status: Option<TodoStatus>) -> Result<Vec<Todo>> {
        let lock = self.lock_for(slug).await;
        let _g = lock.lock().await;
        let file = self.load(slug).await?;
        let items: Vec<Todo> = match status {
            Some(s) => file.todos.into_iter().filter(|t| t.status == s).collect(),
            None => file.todos,
        };
        info!(count = items.len(), "todos listed");
        Ok(items)
    }

    #[instrument(skip(self), fields(slug = %slug))]
    pub async fn create(
        &self,
        slug: &str,
        name: String,
        description: Option<String>,
    ) -> Result<Todo> {
        if name.trim().is_empty() {
            return Err(anyhow!("todo name cannot be empty"));
        }
        let lock = self.lock_for(slug).await;
        let _g = lock.lock().await;
        let mut file = self.load(slug).await?;
        let now = Utc::now();
        let todo = Todo {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            description: description.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
            status: TodoStatus::Pending,
            status_reason: None,
            created_at: now,
            updated_at: now,
        };
        file.todos.push(todo.clone());
        self.save(slug, &file).await?;
        info!(id = %todo.id, name = %todo.name, "todo created");
        self.broadcast(slug, &file.todos);
        Ok(todo)
    }

    #[instrument(skip(self), fields(slug = %slug, id = %id))]
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        slug: &str,
        id: &str,
        name: Option<String>,
        description: Option<String>,
        status: Option<TodoStatus>,
        status_reason: Option<String>,
    ) -> Result<Todo> {
        let lock = self.lock_for(slug).await;
        let _g = lock.lock().await;
        let mut file = self.load(slug).await?;
        let idx = file
            .todos
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| anyhow!("todo {id} not found"))?;
        let t = &mut file.todos[idx];
        if let Some(n) = name {
            if !n.trim().is_empty() {
                t.name = n.trim().to_string();
            }
        }
        if let Some(d) = description {
            let trimmed = d.trim().to_string();
            t.description = if trimmed.is_empty() { None } else { Some(trimmed) };
        }
        if let Some(s) = status {
            t.status = s;
        }
        if let Some(r) = status_reason {
            let trimmed = r.trim().to_string();
            t.status_reason = if trimmed.is_empty() { None } else { Some(trimmed) };
        }
        t.updated_at = Utc::now();
        let todo = t.clone();
        self.save(slug, &file).await?;
        info!(status = ?todo.status, "todo updated");
        self.broadcast(slug, &file.todos);
        Ok(todo)
    }

    #[instrument(skip(self), fields(slug = %slug, id = %id))]
    pub async fn delete(&self, slug: &str, id: &str) -> Result<()> {
        let lock = self.lock_for(slug).await;
        let _g = lock.lock().await;
        let mut file = self.load(slug).await?;
        let before = file.todos.len();
        file.todos.retain(|t| t.id != id);
        if file.todos.len() == before {
            warn!("todo not found for delete");
            return Err(anyhow!("todo {id} not found"));
        }
        self.save(slug, &file).await?;
        info!("todo deleted");
        self.broadcast(slug, &file.todos);
        Ok(())
    }
}
