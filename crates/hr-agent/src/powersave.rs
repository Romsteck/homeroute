//! Service state tracking and manual command handling.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use hr_registry::protocol::{ServiceAction, ServiceState, ServiceType};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::services::ServiceManager;

/// Notification when a service state changes.
#[derive(Debug, Clone)]
pub struct ServiceStateChange {
    pub service_type: ServiceType,
    pub new_state: ServiceState,
}

/// Manages service state tracking and manual commands.
pub struct PowersaveManager {
    /// Service manager for starting/stopping services.
    service_mgr: Arc<RwLock<ServiceManager>>,

    /// Current state of code-server.
    code_server_state: RwLock<ServiceState>,
    /// Current state of app services.
    app_state: RwLock<ServiceState>,
    /// Current state of db services.
    db_state: RwLock<ServiceState>,
}

impl PowersaveManager {
    /// Create a new powersave manager.
    pub fn new(service_mgr: Arc<RwLock<ServiceManager>>) -> Self {
        Self {
            service_mgr,
            code_server_state: RwLock::new(ServiceState::Stopped),
            app_state: RwLock::new(ServiceState::Stopped),
            db_state: RwLock::new(ServiceState::Stopped),
        }
    }

    /// Get the current state of a service type.
    pub fn get_state(&self, service_type: ServiceType) -> ServiceState {
        match service_type {
            ServiceType::CodeServer => *self.code_server_state.read().unwrap(),
            ServiceType::App => *self.app_state.read().unwrap(),
            ServiceType::Db => *self.db_state.read().unwrap(),
        }
    }

    /// Set the state of a service type.
    fn set_state(&self, service_type: ServiceType, state: ServiceState) {
        match service_type {
            ServiceType::CodeServer => *self.code_server_state.write().unwrap() = state,
            ServiceType::App => *self.app_state.write().unwrap() = state,
            ServiceType::Db => *self.db_state.write().unwrap() = state,
        }
    }

    /// Handle a manual service command from the registry.
    pub async fn handle_command(
        &self,
        service_type: ServiceType,
        action: ServiceAction,
        state_tx: &mpsc::Sender<ServiceStateChange>,
    ) {
        // Clone the manager to avoid holding the guard across await
        let mgr = self.service_mgr.read().unwrap().clone();

        match action {
            ServiceAction::Start => {
                info!(service_type = ?service_type, "Manual start command");

                self.set_state(service_type, ServiceState::Starting);

                if let Err(e) = mgr.start(service_type).await {
                    error!(service_type = ?service_type, error = %e, "Failed to start service");
                    self.set_state(service_type, ServiceState::Stopped);
                } else {
                    // Brief delay to verify the process didn't immediately crash
                    // (e.g. binary not found = exit 203/EXEC)
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let actual = mgr.get_state(service_type).await;
                    if actual == ServiceState::Running {
                        self.set_state(service_type, ServiceState::Running);
                    } else {
                        warn!(service_type = ?service_type, actual_state = ?actual,
                              "Service failed to stay running after start");
                        self.set_state(service_type, actual);
                    }
                }

                let _ = state_tx
                    .send(ServiceStateChange {
                        service_type,
                        new_state: self.get_state(service_type),
                    })
                    .await;
            }
            ServiceAction::Stop => {
                info!(service_type = ?service_type, "Manual stop command");
                self.set_state(service_type, ServiceState::Stopping);

                if let Err(e) = mgr.stop(service_type).await {
                    error!(service_type = ?service_type, error = %e, "Failed to stop service");
                }
                self.set_state(service_type, ServiceState::Stopped);

                let _ = state_tx
                    .send(ServiceStateChange {
                        service_type,
                        new_state: ServiceState::Stopped,
                    })
                    .await;
            }
        }
    }

    /// Refresh service states from systemd.
    pub async fn refresh_states(&self) {
        // Clone the manager to avoid holding the guard across await
        let mgr = self.service_mgr.read().unwrap().clone();

        let state = mgr.get_state(ServiceType::CodeServer).await;
        *self.code_server_state.write().unwrap() = state;

        let state = mgr.get_state(ServiceType::App).await;
        *self.app_state.write().unwrap() = state;

        let state = mgr.get_state(ServiceType::Db).await;
        *self.db_state.write().unwrap() = state;
    }
}
