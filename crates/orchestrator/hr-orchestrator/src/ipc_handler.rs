use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{info, warn};

use hr_ipc::orchestrator::OrchestratorRequest;
use hr_ipc::server::IpcHandler;
use hr_ipc::types::IpcResponse;

use crate::container_manager::{
    ContainerManager, CreateContainerRequest, MigrationState,
    RenameContainerRequest, RenameState, UpdateContainerRequest,
};
use hr_common::events::{PowerAction, WakeResult};
use hr_git::GitService;
use hr_registry::protocol::{HostRegistryMessage, RegistryMessage};
use hr_registry::types::UpdateApplicationRequest;
use hr_registry::AgentRegistry;

pub struct OrchestratorHandler {
    pub registry: Arc<AgentRegistry>,
    pub container_manager: Arc<ContainerManager>,
    pub git: Arc<GitService>,
    pub migrations: Arc<RwLock<HashMap<String, MigrationState>>>,
    pub renames: Arc<RwLock<HashMap<String, RenameState>>>,
}

impl IpcHandler<OrchestratorRequest, IpcResponse> for OrchestratorHandler {
    async fn handle(&self, request: OrchestratorRequest) -> IpcResponse {
        match request {
            // ── Applications ─────────────────────────────────────
            OrchestratorRequest::ListApplications => {
                let apps = self.registry.list_applications().await;
                IpcResponse::ok_data(apps)
            }
            OrchestratorRequest::GetApplication { id } => {
                match self.registry.get_application(&id).await {
                    Some(app) => IpcResponse::ok_data(app),
                    None => IpcResponse::err("Application not found"),
                }
            }
            OrchestratorRequest::IsAgentConnected { app_id } => {
                let connected = self.registry.is_agent_connected(&app_id).await;
                IpcResponse::ok_data(connected)
            }

            // ── Applications extended ────────────────────────────
            OrchestratorRequest::UpdateApplication { id, request } => {
                let req: UpdateApplicationRequest = match serde_json::from_value(request) {
                    Ok(r) => r,
                    Err(e) => return IpcResponse::err(format!("Invalid update request: {e}")),
                };
                match self.registry.update_application(&id, req).await {
                    Ok(Some(app)) => IpcResponse::ok_data(app),
                    Ok(None) => IpcResponse::err("Application not found"),
                    Err(e) => IpcResponse::err(format!("Failed to update application: {e}")),
                }
            }
            OrchestratorRequest::DeleteApplication { id } => {
                match self.registry.remove_application(&id).await {
                    Ok(true) => IpcResponse::ok_empty(),
                    Ok(false) => IpcResponse::err("Application not found"),
                    Err(e) => IpcResponse::err(format!("Failed to delete application: {e}")),
                }
            }
            OrchestratorRequest::ExecInContainer { app_id, commands } => {
                // Look up the app to determine host_id and container_name
                let app = match self.registry.get_application(&app_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Application not found"),
                };

                if app.host_id == "local" {
                    // Local exec via machinectl shell
                    let cmd_str = commands.join(" ");
                    match tokio::process::Command::new("machinectl")
                        .args(["shell", &app.container_name, "/bin/bash", "-c", &cmd_str])
                        .output()
                        .await
                    {
                        Ok(out) => IpcResponse::ok_data(serde_json::json!({
                            "success": out.status.success(),
                            "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                            "stderr": String::from_utf8_lossy(&out.stderr).to_string(),
                        })),
                        Err(e) => IpcResponse::err(format!("Failed to exec: {e}")),
                    }
                } else {
                    // Remote exec via host-agent
                    match self.registry.exec_in_remote_container(
                        &app.host_id,
                        &app.container_name,
                        commands,
                    ).await {
                        Ok((success, stdout, stderr)) => IpcResponse::ok_data(serde_json::json!({
                            "success": success,
                            "stdout": stdout,
                            "stderr": stderr,
                        })),
                        Err(e) => IpcResponse::err(format!("Failed to exec: {e}")),
                    }
                }
            }
            OrchestratorRequest::ExecRemoteContainer { host_id, container_name, commands } => {
                match self.registry.exec_in_remote_container(
                    &host_id,
                    &container_name,
                    commands,
                ).await {
                    Ok((success, stdout, stderr)) => IpcResponse::ok_data(serde_json::json!({
                        "success": success,
                        "stdout": stdout,
                        "stderr": stderr,
                    })),
                    Err(e) => IpcResponse::err(format!("Failed to exec: {e}")),
                }
            }
            OrchestratorRequest::SendToAgent { app_id, message } => {
                let msg: RegistryMessage = match serde_json::from_value(message.clone()) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(app_id, error = %e, raw = %message, "SendToAgent: invalid message");
                        return IpcResponse::err(format!("Invalid message: {e}"));
                    }
                };
                let msg_type = message.get("type").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                info!(app_id = %app_id, msg_type = %msg_type, "IPC SendToAgent dispatching");
                match self.registry.send_to_agent(&app_id, msg).await {
                    Ok(()) => {
                        info!(app_id = %app_id, msg_type = %msg_type, "IPC SendToAgent delivered");
                        IpcResponse::ok_empty()
                    }
                    Err(e) => {
                        warn!(app_id = %app_id, error = %e, "IPC SendToAgent failed");
                        IpcResponse::err(format!("Failed to send to agent: {e}"))
                    }
                }
            }
            OrchestratorRequest::TriggerAgentUpdate { agent_ids } => {
                match self.registry.trigger_update(agent_ids).await {
                    Ok(result) => IpcResponse::ok_data(result),
                    Err(e) => IpcResponse::err(format!("Failed to trigger update: {e}")),
                }
            }
            OrchestratorRequest::GetAgentUpdateStatus => {
                match self.registry.get_update_status().await {
                    Ok(status) => IpcResponse::ok_data(status),
                    Err(e) => IpcResponse::err(format!("Failed to get update status: {e}")),
                }
            }
            OrchestratorRequest::FixAgentUpdate { app_id } => {
                let app = match self.registry.get_application(&app_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Application not found"),
                };

                if app.host_id == "local" {
                    match self.registry.fix_agent_via_exec(&app_id).await {
                        Ok(output) => {
                            info!(app_id = app_id, "Agent fixed via machinectl exec");
                            IpcResponse::ok_data(serde_json::json!({"success": true, "output": output}))
                        }
                        Err(e) => IpcResponse::err(format!("Failed to fix agent: {e}")),
                    }
                } else {
                    let api_port = self.container_manager.env.api_port;
                    let cmd = vec![
                        format!(
                            "curl -fsSL http://10.0.0.254:{}/api/applications/agents/binary \
                             -o /usr/local/bin/hr-agent.new && \
                             chmod +x /usr/local/bin/hr-agent.new && \
                             mv /usr/local/bin/hr-agent.new /usr/local/bin/hr-agent && \
                             systemctl restart hr-agent",
                            api_port
                        ),
                    ];
                    match self.registry.exec_in_remote_container(
                        &app.host_id,
                        &app.container_name,
                        cmd,
                    ).await {
                        Ok((true, stdout, _)) => {
                            info!(app_id = app_id, "Agent fixed via remote exec");
                            IpcResponse::ok_data(serde_json::json!({"success": true, "output": stdout}))
                        }
                        Ok((false, _, stderr)) => {
                            IpcResponse::err(format!("Fix command failed: {stderr}"))
                        }
                        Err(e) => IpcResponse::err(format!("Failed to fix agent: {e}")),
                    }
                }
            }
            OrchestratorRequest::UpdateAgentRules { app_ids } => {
                let apps = self.registry.list_applications().await;
                let target_apps: Vec<_> = match &app_ids {
                    Some(ids) => apps.into_iter().filter(|a| ids.contains(&a.id)).collect(),
                    None => apps,
                };

                let mut updated = 0u32;
                for app in &target_apps {
                    if self.registry.is_agent_connected(&app.id).await {
                        // Send UpdateRules via the registry's internal push_rules mechanism.
                        // The registry handles rendering rules templates per-app.
                        let msg = RegistryMessage::UpdateRules { rules: vec![] };
                        if self.registry.send_to_agent(&app.id, msg).await.is_ok() {
                            updated += 1;
                        }
                    }
                }

                IpcResponse::ok_data(serde_json::json!({
                    "updated": updated,
                    "total": target_apps.len(),
                }))
            }
            OrchestratorRequest::ResolveLinkedProd { dev_id } => {
                let dev_app = match self.registry.get_application(&dev_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Dev application not found"),
                };

                let linked_id = match &dev_app.linked_app_id {
                    Some(id) => id.clone(),
                    None => return IpcResponse::err("No linked production app"),
                };

                let prod_app = match self.registry.get_application(&linked_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Linked production app not found"),
                };

                IpcResponse::ok_data(serde_json::json!({
                    "prod_id": prod_app.id,
                    "container_name": prod_app.container_name,
                    "host_id": prod_app.host_id,
                }))
            }

            // ── Containers ───────────────────────────────────────
            OrchestratorRequest::ListContainers => {
                let containers = self.container_manager.list_containers().await;
                IpcResponse::ok_data(containers)
            }
            OrchestratorRequest::GetContainer { id } => {
                match self.container_manager.get_container(&id).await {
                    Some(container) => IpcResponse::ok_data(container),
                    None => IpcResponse::err("Container not found"),
                }
            }
            OrchestratorRequest::CreateContainer { request } => {
                // Validate the request parses correctly
                let _req: CreateContainerRequest = match serde_json::from_value(request) {
                    Ok(r) => r,
                    Err(e) => return IpcResponse::err(format!("Invalid create request: {e}")),
                };
                // TODO: Wire up create_container when the full deploy pipeline
                // is migrated to hr-orchestrator. For now, return not implemented.
                IpcResponse::err("Container creation not yet migrated to orchestrator")
            }
            OrchestratorRequest::StartContainer { id } => {
                match self.container_manager.start_container(&id).await {
                    Ok(true) => IpcResponse::ok_empty(),
                    Ok(false) => IpcResponse::err("Container not found"),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::StopContainer { id } => {
                match self.container_manager.stop_container(&id).await {
                    Ok(true) => IpcResponse::ok_empty(),
                    Ok(false) => IpcResponse::err("Container not found"),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::DeleteContainer { id } => {
                match self.container_manager.remove_container(&id).await {
                    Ok(true) => IpcResponse::ok_empty(),
                    Ok(false) => IpcResponse::err("Container not found"),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::UpdateContainer { id, request } => {
                let req: UpdateContainerRequest = match serde_json::from_value(request) {
                    Ok(r) => r,
                    Err(e) => return IpcResponse::err(format!("Invalid update request: {e}")),
                };
                match self.container_manager.update_container(&id, req).await {
                    Ok(true) => IpcResponse::ok_empty(),
                    Ok(false) => IpcResponse::err("Container not found"),
                    Err(e) => IpcResponse::err(e),
                }
            }

            // ── Container extended ───────────────────────────────
            OrchestratorRequest::MigrateContainer { id, target_host_id } => {
                match self.container_manager.migrate_container(
                    &id,
                    &target_host_id,
                    &self.migrations,
                ).await {
                    Ok(transfer_id) => IpcResponse::ok_data(serde_json::json!({
                        "transfer_id": transfer_id,
                    })),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::GetMigrationStatus { app_id } => {
                let migrations = self.migrations.read().await;
                // Find migration by app_id (migrations are keyed by transfer_id)
                let migration = migrations.values().find(|m| m.app_id == app_id);
                match migration {
                    Some(state) => IpcResponse::ok_data(state),
                    None => IpcResponse::err("No active migration for this application"),
                }
            }
            OrchestratorRequest::CancelMigration { app_id } => {
                let migrations = self.migrations.read().await;
                let migration = migrations.values().find(|m| m.app_id == app_id);
                match migration {
                    Some(state) => {
                        state.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                        info!(app_id = app_id, "Migration cancellation requested");
                        IpcResponse::ok_empty()
                    }
                    None => IpcResponse::err("No active migration for this application"),
                }
            }
            OrchestratorRequest::RenameContainer { id, request } => {
                let req: RenameContainerRequest = match serde_json::from_value(request) {
                    Ok(r) => r,
                    Err(e) => return IpcResponse::err(format!("Invalid rename request: {e}")),
                };
                match self.container_manager.rename_container(
                    &id,
                    req,
                    &self.renames,
                ).await {
                    Ok(rename_id) => IpcResponse::ok_data(serde_json::json!({
                        "rename_id": rename_id,
                    })),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::GetRenameStatus { app_id } => {
                let renames = self.renames.read().await;
                // Find rename by app_id (renames track app_ids in a Vec)
                let rename = renames.values().find(|r| r.app_ids.contains(&app_id));
                match rename {
                    Some(state) => IpcResponse::ok_data(state),
                    None => IpcResponse::err("No active rename for this application"),
                }
            }
            OrchestratorRequest::GetContainerConfig => {
                let config = self.container_manager.get_config().await;
                IpcResponse::ok_data(config)
            }
            OrchestratorRequest::UpdateContainerConfig { config } => {
                let cfg = match serde_json::from_value(config) {
                    Ok(c) => c,
                    Err(e) => return IpcResponse::err(format!("Invalid container config: {e}")),
                };
                match self.container_manager.update_config(cfg).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(e),
                }
            }

            // ── Deploy ───────────────────────────────────────────
            OrchestratorRequest::DeployToProduction { dev_id, binary_path } => {
                // Resolve linked prod app
                let dev_app = match self.registry.get_application(&dev_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Dev application not found"),
                };
                let linked_id = match &dev_app.linked_app_id {
                    Some(id) => id.clone(),
                    None => return IpcResponse::err("No linked production app"),
                };
                let prod_app = match self.registry.get_application(&linked_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Linked production app not found"),
                };

                // Verify binary file exists
                let binary_path_obj = std::path::Path::new(&binary_path);
                if !binary_path_obj.exists() {
                    return IpcResponse::err("Binary file not found at specified path");
                }

                info!(
                    dev_id = dev_id,
                    prod_id = prod_app.id,
                    container = prod_app.container_name,
                    "Deploy to production initiated via IPC"
                );

                // For local containers, copy binary via machinectl
                // For remote containers, the binary needs to be served via HTTP
                if prod_app.host_id == "local" {
                    // Copy binary into the container
                    let copy_result = tokio::process::Command::new("machinectl")
                        .args(["copy-to", &prod_app.container_name, &binary_path, "/opt/app/app.new"])
                        .output()
                        .await;

                    // Clean up temp file
                    let _ = tokio::fs::remove_file(&binary_path).await;

                    match copy_result {
                        Ok(out) if out.status.success() => {
                            // Atomically replace and restart
                            let swap_cmd = "chmod +x /opt/app/app.new && mv /opt/app/app.new /opt/app/app && systemctl restart app.service";
                            match tokio::process::Command::new("machinectl")
                                .args(["shell", &prod_app.container_name, "/bin/bash", "-c", swap_cmd])
                                .output()
                                .await
                            {
                                Ok(out) if out.status.success() => {
                                    IpcResponse::ok_data(serde_json::json!({"success": true, "message": "Deployed successfully"}))
                                }
                                Ok(out) => {
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    IpcResponse::err(format!("Restart failed: {stderr}"))
                                }
                                Err(e) => IpcResponse::err(format!("Restart exec failed: {e}")),
                            }
                        }
                        Ok(out) => {
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            IpcResponse::err(format!("Copy failed: {stderr}"))
                        }
                        Err(e) => IpcResponse::err(format!("machinectl copy-to failed: {e}")),
                    }
                } else {
                    // Remote deploy: binary must be served via API, then curl'd
                    let _ = tokio::fs::remove_file(&binary_path).await;
                    IpcResponse::err("Remote production deploy not yet implemented in orchestrator IPC")
                }
            }
            OrchestratorRequest::ProdPush { dev_id, dest_path, archive_path } => {
                // Resolve linked prod app
                let dev_app = match self.registry.get_application(&dev_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Dev application not found"),
                };
                let linked_id = match &dev_app.linked_app_id {
                    Some(id) => id.clone(),
                    None => return IpcResponse::err("No linked production app"),
                };
                let prod_app = match self.registry.get_application(&linked_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Linked production app not found"),
                };

                let archive = std::path::Path::new(&archive_path);
                if !archive.exists() {
                    return IpcResponse::err("Archive file not found");
                }

                if prod_app.host_id == "local" {
                    // Copy archive into container, extract, clean up
                    let tmp_dest = format!("/tmp/push-{}.tar", uuid::Uuid::new_v4());
                    let copy_result = tokio::process::Command::new("machinectl")
                        .args(["copy-to", &prod_app.container_name, &archive_path, &tmp_dest])
                        .output()
                        .await;

                    let _ = tokio::fs::remove_file(&archive_path).await;

                    match copy_result {
                        Ok(out) if out.status.success() => {
                            let extract_cmd = format!(
                                "mkdir -p '{}' && tar xf '{}' -C '{}' && rm -f '{}'",
                                dest_path, tmp_dest, dest_path, tmp_dest
                            );
                            match tokio::process::Command::new("machinectl")
                                .args(["shell", &prod_app.container_name, "/bin/bash", "-c", &extract_cmd])
                                .output()
                                .await
                            {
                                Ok(out) if out.status.success() => {
                                    IpcResponse::ok_data(serde_json::json!({"success": true}))
                                }
                                Ok(out) => {
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    IpcResponse::err(format!("Extract failed: {stderr}"))
                                }
                                Err(e) => IpcResponse::err(format!("Extract exec failed: {e}")),
                            }
                        }
                        Ok(out) => {
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            IpcResponse::err(format!("Copy failed: {stderr}"))
                        }
                        Err(e) => IpcResponse::err(format!("machinectl copy-to failed: {e}")),
                    }
                } else {
                    let _ = tokio::fs::remove_file(&archive_path).await;
                    IpcResponse::err("Remote prod push not yet implemented in orchestrator IPC")
                }
            }

            // ── Git ──────────────────────────────────────────────
            OrchestratorRequest::ListRepos => {
                match self.git.list_repos().await {
                    Ok(repos) => IpcResponse::ok_data(repos),
                    Err(e) => IpcResponse::err(format!("Failed to list repos: {e}")),
                }
            }
            OrchestratorRequest::GetRepo { slug } => {
                match self.git.get_repo(&slug).await {
                    Ok(Some(repo)) => IpcResponse::ok_data(repo),
                    Ok(None) => IpcResponse::err("Repository not found"),
                    Err(e) => IpcResponse::err(format!("Failed to get repo: {e}")),
                }
            }
            OrchestratorRequest::CreateRepo { slug } => {
                match self.git.create_repo(&slug).await {
                    Ok(path) => IpcResponse::ok_data(serde_json::json!({
                        "path": path.display().to_string()
                    })),
                    Err(e) => IpcResponse::err(format!("Failed to create repo: {e}")),
                }
            }
            OrchestratorRequest::DeleteRepo { slug } => {
                match self.git.delete_repo(&slug).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(format!("Failed to delete repo: {e}")),
                }
            }

            // ── Git extended ─────────────────────────────────────
            OrchestratorRequest::GetCommits { slug, limit } => {
                match self.git.get_commits(&slug, limit).await {
                    Ok(commits) => IpcResponse::ok_data(commits),
                    Err(e) => IpcResponse::err(format!("Failed to get commits: {e}")),
                }
            }
            OrchestratorRequest::GetBranches { slug } => {
                match self.git.get_branches(&slug).await {
                    Ok(branches) => IpcResponse::ok_data(branches),
                    Err(e) => IpcResponse::err(format!("Failed to get branches: {e}")),
                }
            }
            OrchestratorRequest::TriggerSync { slug } => {
                match self.git.trigger_sync(&slug).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(format!("Failed to sync: {e}")),
                }
            }
            OrchestratorRequest::SyncAll => {
                let config = match self.git.load_config().await {
                    Ok(c) => c,
                    Err(e) => return IpcResponse::err(format!("Failed to load git config: {e}")),
                };

                let mut synced = 0u32;
                let mut errors = Vec::new();
                for (slug, mirror) in &config.mirrors {
                    if mirror.enabled {
                        match self.git.trigger_sync(slug).await {
                            Ok(()) => synced += 1,
                            Err(e) => {
                                warn!(slug = slug, error = %e, "Mirror sync failed");
                                errors.push(format!("{}: {}", slug, e));
                            }
                        }
                    }
                }

                IpcResponse::ok_data(serde_json::json!({
                    "synced": synced,
                    "errors": errors,
                }))
            }
            OrchestratorRequest::GetSshKey => {
                match self.git.get_ssh_key().await {
                    Ok(info) => IpcResponse::ok_data(info),
                    Err(e) => IpcResponse::err(format!("Failed to get SSH key: {e}")),
                }
            }
            OrchestratorRequest::GenerateSshKey => {
                match self.git.generate_ssh_key().await {
                    Ok(info) => IpcResponse::ok_data(info),
                    Err(e) => IpcResponse::err(format!("Failed to generate SSH key: {e}")),
                }
            }
            OrchestratorRequest::GetGitConfig => {
                match self.git.load_config().await {
                    Ok(config) => IpcResponse::ok_data(config),
                    Err(e) => IpcResponse::err(format!("Failed to load git config: {e}")),
                }
            }
            OrchestratorRequest::UpdateGitConfig { config } => {
                let cfg: hr_git::types::GitConfig = match serde_json::from_value(config) {
                    Ok(c) => c,
                    Err(e) => return IpcResponse::err(format!("Invalid git config: {e}")),
                };

                // Save the config
                match self.git.save_config(&cfg).await {
                    Ok(()) => {
                        // Update mirrors based on config changes
                        for (slug, mirror) in &cfg.mirrors {
                            if mirror.enabled {
                                if let Err(e) = self.git.enable_mirror(slug, &cfg.github_org).await {
                                    warn!(slug = slug, error = %e, "Failed to enable mirror");
                                }
                            } else {
                                if let Err(e) = self.git.disable_mirror(slug).await {
                                    warn!(slug = slug, error = %e, "Failed to disable mirror");
                                }
                            }
                        }
                        IpcResponse::ok_empty()
                    }
                    Err(e) => IpcResponse::err(format!("Failed to save git config: {e}")),
                }
            }

            // ── Dataverse ────────────────────────────────────────
            OrchestratorRequest::DataverseQuery { app_id, query } => {
                // Parse the query JSON into the protocol type
                let dv_query = match serde_json::from_value(query) {
                    Ok(q) => q,
                    Err(e) => return IpcResponse::err(format!("Invalid dataverse query: {e}")),
                };
                match self.registry.dataverse_query(&app_id, dv_query).await {
                    Ok(result) => IpcResponse::ok_data(result),
                    Err(e) => IpcResponse::err(format!("Dataverse query failed: {e}")),
                }
            }
            OrchestratorRequest::DataverseGetSchema { app_id } => {
                let schemas = self.registry.dataverse_schemas.read().await;
                match schemas.get(&app_id) {
                    Some(schema) => IpcResponse::ok_data(schema),
                    None => IpcResponse::err("No cached schema for this application"),
                }
            }
            OrchestratorRequest::DataverseOverview => {
                let schemas = self.registry.dataverse_schemas.read().await;
                let apps: Vec<serde_json::Value> = schemas.values().cloned().collect();
                IpcResponse::ok_data(serde_json::json!({ "apps": apps }))
            }

            // ── Host operations ──────────────────────────────────
            OrchestratorRequest::ListHostConnections => {
                let conns = self.registry.host_connections.read().await;
                let hosts: Vec<serde_json::Value> = conns.iter().map(|(id, conn)| {
                    serde_json::json!({
                        "host_id": id,
                        "host_name": conn.host_name,
                        "connected_at": conn.connected_at.to_rfc3339(),
                        "last_heartbeat": conn.last_heartbeat.to_rfc3339(),
                        "version": conn.version,
                        "metrics": conn.metrics,
                        "containers": conn.containers,
                    })
                }).collect();
                IpcResponse::ok_data(hosts)
            }
            OrchestratorRequest::IsHostConnected { host_id } => {
                let connected = self.registry.is_host_connected(&host_id).await;
                IpcResponse::ok_data(connected)
            }
            OrchestratorRequest::GetHostPowerState { host_id } => {
                let state = self.registry.get_host_power_state(&host_id).await;
                IpcResponse::ok_data(state)
            }
            OrchestratorRequest::SendHostCommand { host_id, command } => {
                let msg: HostRegistryMessage = match serde_json::from_value(command) {
                    Ok(m) => m,
                    Err(e) => return IpcResponse::err(format!("Invalid host command: {e}")),
                };
                match self.registry.send_host_command(&host_id, msg).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::WakeHost { host_id } => {
                match self.registry.request_wake_host(&host_id).await {
                    Ok(result) => {
                        let action = match result {
                            WakeResult::WolSent => "wol_sent",
                            WakeResult::AlreadyOnline => "already_online",
                            WakeResult::AlreadyWaking => "already_waking",
                        };
                        IpcResponse::ok_data(serde_json::json!({"action": action}))
                    }
                    Err(e) => IpcResponse::err(e),
                }
            }
            OrchestratorRequest::HostPowerAction { host_id, action } => {
                let power_action = match action.as_str() {
                    "shutdown" => PowerAction::Shutdown,
                    "reboot" => PowerAction::Reboot,
                    "suspend" => PowerAction::Suspend,
                    _ => return IpcResponse::err(format!("Unknown power action: {action}")),
                };
                if let Err(e) = self.registry.request_power_action(&host_id, power_action).await {
                    return IpcResponse::err(e);
                }
                let msg = match action.as_str() {
                    "shutdown" => HostRegistryMessage::PowerOff,
                    "reboot" => HostRegistryMessage::Reboot,
                    _ => HostRegistryMessage::SuspendHost,
                };
                match self.registry.send_host_command(&host_id, msg).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(e),
                }
            }

            // ── Updates scan ─────────────────────────────────────
            OrchestratorRequest::ScanUpdates => {
                let count = self.registry.trigger_update_scan().await;
                IpcResponse::ok_data(serde_json::json!({"agents_scanned": count}))
            }
            OrchestratorRequest::GetScanResults => {
                let results = self.registry.scan_results.read().await;
                IpcResponse::ok_data(&*results)
            }
            OrchestratorRequest::StoreScanResult { target } => {
                if let (Some(id), Ok(t)) = (
                    target.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    serde_json::from_value::<hr_common::events::UpdateTarget>(target),
                ) {
                    self.registry.scan_results.write().await.insert(id, t);
                    IpcResponse::ok_empty()
                } else {
                    IpcResponse::err("invalid target")
                }
            }
        }
    }
}
