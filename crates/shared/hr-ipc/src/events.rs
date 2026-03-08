use serde::{Deserialize, Serialize};

/// Serialized IPC event for cross-process distribution via EventBus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    /// Channel name (e.g. "cert_ready", "config_changed").
    pub channel: String,
    /// JSON payload — the actual event data.
    pub payload: serde_json::Value,
}

/// Subscription message for the event bus.
#[derive(Debug, Serialize, Deserialize)]
pub enum EventSubscription {
    Subscribe { channels: Vec<String> },
    Unsubscribe { channels: Vec<String> },
}
