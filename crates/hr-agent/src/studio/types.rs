use serde::{Deserialize, Serialize};

/// A single console log entry forwarded from the browser dev preview.
#[derive(Debug, Deserialize)]
pub struct ConsoleLogEntry {
    pub level: String,
    pub message: String,
    pub timestamp: u64,
}

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
        /// Optional attached images (base64-encoded)
        #[serde(default)]
        images: Option<Vec<ImageAttachment>>,
    },
    Abort {
        #[serde(default)]
        session_id: Option<String>,
    },
    GetActiveStreams,
    ListSessions,
    GetSession {
        session_id: String,
        #[serde(default = "default_limit")]
        limit: usize,
    },
    DeleteSession {
        session_id: String,
    },
    ListDirectory {
        path: String,
    },
    ReadFile {
        path: String,
    },
    SubmitToken {
        token: String,
    },
    GetAuthStatus,
    UnlinkAuth,
    ConsoleLogs {
        logs: Vec<ConsoleLogEntry>,
    },
    TerminalStart {
        session_id: String,
        #[serde(default)]
        resume_session: Option<String>,
        #[serde(default)]
        model: Option<String>,
        cols: u16,
        rows: u16,
    },
    TerminalData {
        session_id: String,
        data: String, // base64
    },
    TerminalResize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
}

fn default_limit() -> usize {
    50
}

fn default_mode() -> String {
    "default".to_string()
}

/// Server → Client WebSocket messages.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutMessage {
    Stream {
        data: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    SessionMessages {
        messages: Vec<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    ActiveStreams {
        session_ids: Vec<String>,
    },
    DirectoryListing {
        path: String,
        entries: Vec<FileEntry>,
    },
    FileContent {
        path: String,
        content: String,
        size: u64,
        truncated: bool,
    },
    AuthStatus {
        authenticated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        method: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subscription_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
    TodoUpdate {
        session_id: String,
        todos: Vec<serde_json::Value>,
    },
    TerminalData {
        session_id: String,
        data: String, // base64
    },
    TerminalDone {
        session_id: String,
    },
}

/// Base64-encoded image attachment from the client.
#[derive(Debug, Deserialize)]
pub struct ImageAttachment {
    /// Base64-encoded image data
    pub data: String,
    /// MIME type (e.g. "image/png", "image/jpeg")
    #[serde(default = "default_media_type", rename = "mediaType")]
    pub media_type: String,
}

fn default_media_type() -> String {
    "image/png".to_string()
}

/// A single entry in a directory listing.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    /// "file" or "directory"
    pub kind: String,
    pub size: u64,
}

/// Session metadata.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub project: String,
    pub last_modified: u64,
    pub message_count: usize,
    /// First user message summary (truncated)
    pub summary: String,
    #[serde(default = "default_session_type")]
    pub session_type: String,
}

#[allow(dead_code)] // Used by serde(default)
fn default_session_type() -> String {
    "agent".to_string()
}
