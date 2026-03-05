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

/// Start the JSONL todo watcher.
/// Watches all PROJECTS_DIRS for .jsonl file modifications and broadcasts
/// TodoUpdate messages when a TodoWrite block is detected.
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

    // Watch all known project directories
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
                    // Only process .jsonl files
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
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

                    // Parse the last TodoWrite block from the file
                    match extract_last_todos(path) {
                        Some(todos) => {
                            debug!("Todo update for session {}: {} todos", session_id, todos.len());
                            studio.broadcast_all(&WsOutMessage::TodoUpdate {
                                session_id,
                                todos,
                            }).await;
                        }
                        None => {
                            // No TodoWrite found — skip
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
