pub mod transport;
pub mod generic;
pub mod events;
pub mod types;
pub mod client;
pub mod server;
pub mod edge;
pub mod orchestrator;

// Backward-compatible re-exports
pub use client::NetcoreClient;
pub use generic::IpcClient;
pub use edge::EdgeClient;
pub use orchestrator::OrchestratorClient;
pub use types::*;
