use serde::{Deserialize, Serialize};

/// Client → Server WebSocket messages.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsInMessage {
    Prompt {
        prompt: String,
        #[serde(default)]
        session_id: Option<String>,
        /// Permission mode: "default", "plan", "acceptEdits"
        #[serde(default = "default_mode")]
        mode: String,
        /// Optional model override (e.g. "claude-sonnet-4-6", "claude-opus-4-6")
        #[serde(default)]
        model: Option<String>,
    },
    Abort,
    ListSessions,
    GetSession {
        session_id: String,
        #[serde(default = "default_limit")]
        limit: usize,
    },
    DeleteSession {
        session_id: String,
    },
}

fn default_limit() -> usize {
    50
}

fn default_mode() -> String {
    "default".to_string()
}

/// Server → Client WebSocket messages.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutMessage {
    Stream {
        data: serde_json::Value,
    },
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Error {
        message: String,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    SessionMessages {
        messages: Vec<serde_json::Value>,
    },
    Busy {
        message: String,
    },
}

/// Session metadata.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub project: String,
    pub last_modified: u64,
    pub message_count: usize,
    /// First user message summary (truncated)
    pub summary: String,
}
