use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};

use crate::transport;

/// Default timeout for IPC requests (2 seconds).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Generic IPC client for any service using JSON-line over Unix socket.
///
/// Type parameters:
/// - `Req`: the request enum/struct sent to the service
/// - `Resp`: the response struct returned by the service
pub struct IpcClient<Req, Resp> {
    socket_path: PathBuf,
    default_timeout: Duration,
    _phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> IpcClient<Req, Resp>
where
    Req: Serialize,
    Resp: DeserializeOwned,
{
    /// Create a new client with the default 2-second timeout.
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            default_timeout: DEFAULT_TIMEOUT,
            _phantom: PhantomData,
        }
    }

    /// Create a new client with a custom default timeout.
    pub fn with_timeout(socket_path: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket_path: socket_path.into(),
            default_timeout: timeout,
            _phantom: PhantomData,
        }
    }

    /// Returns the path to the Unix socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a request using the default timeout.
    pub async fn request(&self, req: &Req) -> Result<Resp> {
        transport::request(&self.socket_path, req, self.default_timeout).await
    }

    /// Send a request with a custom timeout (overrides default).
    pub async fn request_with_timeout(&self, req: &Req, timeout: Duration) -> Result<Resp> {
        transport::request(&self.socket_path, req, timeout).await
    }
}
