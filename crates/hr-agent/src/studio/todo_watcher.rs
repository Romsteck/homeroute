use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::StudioBridge;
use super::types::WsOutMessage;

/// Directory where MCP todo_save writes per-session notification files.
const STUDIO_TODOS_DIR: &str = "/tmp/studio-todos";

/// Start the JSONL todo watcher.
/// Watches PROJECTS_DIRS for .jsonl modifications (agent mode) and
/// /tmp/studio-todos/ for .json notifications (CLI terminal mode).
pub fn start(studio: Arc<StudioBridge>) {
    tokio::spawn(async move {
        if let Err(e) = run_watcher(studio).await {
            error!("Todo watcher failed: {e}");
        }
    });
}

async fn run_watcher(studio: Arc<StudioBridge>) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<Event>(256);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    warn!("File watcher error: {e}");
                }
            }
        },
        notify::Config::default().with_poll_interval(Duration::from_secs(2)),
    )?;

    // Watch all known project directories (for agent-mode JSONL files)
    for dir in super::sessions::PROJECTS_DIRS {
        let path = std::path::Path::new(dir);
        if path.is_dir() {
            if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                warn!("Failed to watch {}: {e}", dir);
            } else {
                info!("Todo watcher: watching {}", dir);
            }
        }
    }

    // Watch /tmp/studio-todos/ for CLI terminal mode notifications
    let notify_dir = std::path::Path::new(STUDIO_TODOS_DIR);
    let _ = std::fs::create_dir_all(notify_dir);
    if let Err(e) = watcher.watch(notify_dir, RecursiveMode::NonRecursive) {
        warn!("Failed to watch {}: {e}", STUDIO_TODOS_DIR);
    } else {
        info!("Todo watcher: watching {}", STUDIO_TODOS_DIR);
    }

    // Debounce: track last processed time per file
    let mut last_processed: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();
    let debounce_duration = Duration::from_millis(500);

    loop {
        match rx.recv().await {
            Some(event) => {
                // Only care about modifications and creates
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) => {}
                    _ => continue,
                }

                for path in &event.paths {
                    let ext = path.extension().and_then(|e| e.to_str());

                    // Determine if this is a JSONL (agent mode) or JSON (CLI notification)
                    let is_jsonl = ext == Some("jsonl");
                    let is_cli_notification = ext == Some("json")
                        && path.parent().and_then(|p| p.to_str()) == Some(STUDIO_TODOS_DIR);

                    if !is_jsonl && !is_cli_notification {
                        continue;
                    }

                    // Debounce
                    let now = tokio::time::Instant::now();
                    if let Some(last) = last_processed.get(path) {
                        if now.duration_since(*last) < debounce_duration {
                            continue;
                        }
                    }
                    last_processed.insert(path.clone(), now);

                    // Extract session_id from filename stem
                    let session_id = match path.file_stem().and_then(|s| s.to_str()) {
                        Some(s) => s.to_string(),
                        None => continue,
                    };

                    if is_cli_notification {
                        // CLI mode: parse the JSON file directly as todos
                        if let Some(todos) = parse_studio_todos_file(path) {
                            debug!("CLI todo update for session {}: {} items", session_id, todos.len());
                            studio.broadcast_all(&WsOutMessage::TodoUpdate {
                                session_id,
                                todos,
                            }).await;
                        }
                    } else {
                        // Agent mode: parse JSONL for last TodoWrite block
                        if let Some(todos) = extract_last_todos(path) {
                            debug!("Todo update for session {}: {} todos", session_id, todos.len());
                            studio.broadcast_all(&WsOutMessage::TodoUpdate {
                                session_id,
                                todos,
                            }).await;
                        }
                    }
                }
            }
            None => {
                warn!("Todo watcher channel closed");
                break;
            }
        }
    }

    // Keep watcher alive (moved into this scope)
    drop(watcher);
    Ok(())
}

/// Parse /tmp/studio-todos/{session_id}.json — can be flat array or phased object.
fn parse_studio_todos_file(path: &PathBuf) -> Option<Vec<serde_json::Value>> {
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Phased structure: {"phases": [...]}
    if let Some(phases) = val.get("phases").and_then(|p| p.as_array()) {
        // Flatten phases into a single todo list for the frontend
        let mut todos = Vec::new();
        for phase in phases {
            let name = phase.get("name").and_then(|n| n.as_str()).unwrap_or("Phase");
            let status = phase.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
            if let Some(phase_todos) = phase.get("todos").and_then(|t| t.as_array()) {
                for todo in phase_todos {
                    let mut t = todo.clone();
                    // Prefix content with phase name for context
                    if let Some(obj) = t.as_object_mut() {
                        let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        obj.insert("content".to_string(), serde_json::json!(format!("[{}] {}", name, content)));
                    }
                    todos.push(t);
                }
            } else {
                // Phase itself as a todo item
                todos.push(serde_json::json!({
                    "content": name,
                    "status": status,
                    "activeForm": name,
                }));
            }
        }
        Some(todos)
    } else if let Some(arr) = val.as_array() {
        // Flat todo array
        Some(arr.clone())
    } else {
        None
    }
}

/// Extract the last TodoWrite todos from a JSONL file by reading from the end.
fn extract_last_todos(path: &PathBuf) -> Option<Vec<serde_json::Value>> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut last_todos: Option<Vec<serde_json::Value>> = None;

    // Read all lines and find the last one containing a TodoWrite
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.is_empty() {
            continue;
        }

        // Parse the JSONL line
        let val: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Claude Code JSONL format for tool use:
        // {"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[...]}}]}}
        if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(content) = val.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                        && block.get("name").and_then(|n| n.as_str()) == Some("TodoWrite")
                    {
                        if let Some(todos) = block.get("input").and_then(|i| i.get("todos")).and_then(|t| t.as_array()) {
                            last_todos = Some(todos.clone());
                        }
                    }
                }
            }
        }
    }

    last_todos
}
