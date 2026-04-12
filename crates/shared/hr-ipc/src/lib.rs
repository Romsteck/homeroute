pub mod client;
pub mod edge;
pub mod event_stream;
pub mod events;
pub mod generic;
pub mod orchestrator;
pub mod server;
pub mod transport;
pub mod types;

// Backward-compatible re-exports
pub use client::NetcoreClient;
pub use edge::EdgeClient;
pub use generic::IpcClient;
pub use orchestrator::OrchestratorClient;
pub use types::*;
