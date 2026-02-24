use std::collections::HashSet;
use tracing::{debug, warn};

use super::types::SessionInfo;

const PROJECTS_DIRS: &[&str] = &[
    "/home/studio/.claude/projects",
    "/root/.claude/projects",
];

/// List all Claude Code sessions from all known projects directories.
pub fn list_sessions() -> Vec<SessionInfo> {
    let mut sessions = Vec::new();
    let mut seen_ids = HashSet::new();

    for projects_dir in PROJECTS_DIRS {
        let projects_path = std::path::Path::new(projects_dir);
        if !projects_path.is_dir() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(projects_path) {
            for entry in entries.flatten() {
                let project_path = entry.path();
                if !project_path.is_dir() {
                    continue;
                }
                let project_name = entry.file_name().to_string_lossy().to_string();

                // Look for .jsonl files directly in the project directory
                if let Ok(files) = std::fs::read_dir(&project_path) {
                    for file in files.flatten() {
                        let file_path = file.path();
                        if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                            continue;
                        }
                        let session_id = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();

                        if session_id.is_empty() || !seen_ids.insert(session_id.clone()) {
                            continue;
                        }

                        let last_modified = file_path
                            .metadata()
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        // Count lines and extract summary from first user message
                        let (message_count, summary) = extract_session_info(&file_path);

                        sessions.push(SessionInfo {
                            session_id,
                            project: project_name.clone(),
                            last_modified,
                            message_count,
                            summary,
                        });
                    }
                }
            }
        }
    }

    // Sort by last_modified descending
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    sessions
}

/// Delete a session file. Returns true if successful.
pub fn delete_session(session_id: &str) -> bool {
    for projects_dir in PROJECTS_DIRS {
        let projects_path = std::path::Path::new(projects_dir);
        if !projects_path.is_dir() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(projects_path) {
            for entry in entries.flatten() {
                let file_path = entry.path().join(format!("{}.jsonl", session_id));
                if file_path.is_file() {
                    match std::fs::remove_file(&file_path) {
                        Ok(()) => {
                            debug!("Deleted session file: {}", file_path.display());
                            return true;
                        }
                        Err(e) => {
                            warn!("Failed to delete session {}: {}", session_id, e);
                            return false;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Read messages from a specific session.
pub fn get_session_messages(session_id: &str, limit: usize) -> Vec<serde_json::Value> {
    for projects_dir in PROJECTS_DIRS {
        let projects_path = std::path::Path::new(projects_dir);
        if !projects_path.is_dir() {
            continue;
        }

        // Search for the session file across all project directories
        if let Ok(entries) = std::fs::read_dir(projects_path) {
            for entry in entries.flatten() {
                let file_path = entry.path().join(format!("{}.jsonl", session_id));
                if file_path.is_file() {
                    return read_jsonl_messages(&file_path, limit);
                }
            }
        }
    }

    vec![]
}

/// Extract message count and first user message summary from a JSONL session file.
fn extract_session_info(path: &std::path::Path) -> (usize, String) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (0, String::new()),
    };

    let mut count = 0;
    let mut summary = String::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        count += 1;

        // Extract summary from first user message
        // Claude JSONL format: {"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}
        if summary.is_empty() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

                if msg_type == "user" {
                    // Extract text from message.content (string or array)
                    if let Some(text) = val
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| {
                            // -p mode: content is a plain string
                            if let Some(s) = c.as_str() {
                                Some(s.to_string())
                            }
                            // Interactive mode: content is an array of blocks
                            else if let Some(arr) = c.as_array() {
                                arr.iter().find_map(|item| {
                                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                        item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                                    } else {
                                        None
                                    }
                                })
                            } else {
                                None
                            }
                        })
                    {
                        // Skip IDE context tags
                        let clean = text
                            .lines()
                            .find(|l| !l.trim().is_empty() && !l.contains("<ide_") && !l.contains("</ide_"))
                            .unwrap_or(&text)
                            .trim();
                        let truncated = if clean.len() > 80 {
                            format!("{}...", &clean[..80])
                        } else {
                            clean.to_string()
                        };
                        summary = truncated.replace('\n', " ");
                    }
                }
            }
        }
    }

    (count, summary)
}

fn read_jsonl_messages(path: &std::path::Path, limit: usize) -> Vec<serde_json::Value> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let all_messages: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    // Return last N messages
    if all_messages.len() > limit {
        all_messages[all_messages.len() - limit..].to_vec()
    } else {
        all_messages
    }
}
