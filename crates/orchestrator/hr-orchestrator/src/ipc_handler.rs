use std::sync::Arc;

use tracing::{info, warn};

use hr_ipc::orchestrator::OrchestratorRequest;
use hr_ipc::server::IpcHandler;
use hr_ipc::types::IpcResponse;

use hr_git::GitService;
use hr_registry::AgentRegistry;
use hr_registry::protocol::{HostRegistryMessage, RegistryMessage};
use hr_registry::types::UpdateApplicationRequest;

use hr_common::events::{PowerAction, WakeResult};

use crate::apps_handler::AppsContext;
use crate::backup_pipeline::BackupPipeline;

use hr_apps::{AppSupervisor, ContextGenerator, DbManager};

const BACKUP_SERVER_HOST_ID: &str = "877bcb76-4fb8-4164-940c-707201adf9bc";

pub struct OrchestratorHandler {
    pub registry: Arc<AgentRegistry>,
    pub git: Arc<GitService>,
    pub backup: Arc<BackupPipeline>,
    pub edge: Arc<hr_ipc::EdgeClient>,
    pub base_domain: String,
    pub app_supervisor: AppSupervisor,
    pub db_manager: DbManager,
    pub context_generator: Arc<ContextGenerator>,
    pub log_store: Arc<hr_common::logging::LogStore>,
}

impl OrchestratorHandler {
    fn apps_ctx(&self) -> AppsContext {
        AppsContext {
            supervisor: self.app_supervisor.clone(),
            db_manager: self.db_manager.clone(),
            context_generator: self.context_generator.clone(),
            edge: self.edge.clone(),
            git: self.git.clone(),
            base_domain: self.base_domain.clone(),
            log_store: self.log_store.clone(),
        }
    }
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
                let app = match self.registry.get_application(&app_id).await {
                    Some(a) => a,
                    None => return IpcResponse::err("Application not found"),
                };
                if app.host_id == "local" {
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
                    match self
                        .registry
                        .exec_in_remote_container(&app.host_id, &app.container_name, commands)
                        .await
                    {
                        Ok((success, stdout, stderr)) => IpcResponse::ok_data(serde_json::json!({
                            "success": success,
                            "stdout": stdout,
                            "stderr": stderr,
                        })),
                        Err(e) => IpcResponse::err(format!("Failed to exec: {e}")),
                    }
                }
            }
            OrchestratorRequest::ExecRemoteContainer {
                host_id,
                container_name,
                commands,
            } => {
                match self
                    .registry
                    .exec_in_remote_container(&host_id, &container_name, commands)
                    .await
                {
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
                let msg_type = message
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                info!(app_id = %app_id, msg_type = %msg_type, "IPC SendToAgent dispatching");
                match self.registry.send_to_agent(&app_id, msg).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(format!("Failed to send to agent: {e}")),
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
                        Ok(output) => IpcResponse::ok_data(
                            serde_json::json!({"success": true, "output": output}),
                        ),
                        Err(e) => IpcResponse::err(format!("Failed to fix agent: {e}")),
                    }
                } else {
                    let cmd = vec![format!(
                        "curl -fsSL http://10.0.0.254:4000/api/applications/agents/binary \
                             -o /usr/local/bin/hr-agent.new && \
                             chmod +x /usr/local/bin/hr-agent.new && \
                             mv /usr/local/bin/hr-agent.new /usr/local/bin/hr-agent && \
                             systemctl restart hr-agent"
                    )];
                    match self
                        .registry
                        .exec_in_remote_container(&app.host_id, &app.container_name, cmd)
                        .await
                    {
                        Ok((true, stdout, _)) => IpcResponse::ok_data(
                            serde_json::json!({"success": true, "output": stdout}),
                        ),
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

            // ── Git ──────────────────────────────────────────────
            OrchestratorRequest::ListRepos => match self.git.list_repos().await {
                Ok(repos) => IpcResponse::ok_data(repos),
                Err(e) => IpcResponse::err(format!("Failed to list repos: {e}")),
            },
            OrchestratorRequest::GetRepo { slug } => match self.git.get_repo(&slug).await {
                Ok(Some(repo)) => IpcResponse::ok_data(repo),
                Ok(None) => IpcResponse::err("Repository not found"),
                Err(e) => IpcResponse::err(format!("Failed to get repo: {e}")),
            },
            OrchestratorRequest::CreateRepo { slug } => match self.git.create_repo(&slug).await {
                Ok(path) => IpcResponse::ok_data(serde_json::json!({
                    "path": path.display().to_string()
                })),
                Err(e) => IpcResponse::err(format!("Failed to create repo: {e}")),
            },
            OrchestratorRequest::DeleteRepo { slug } => match self.git.delete_repo(&slug).await {
                Ok(()) => IpcResponse::ok_empty(),
                Err(e) => IpcResponse::err(format!("Failed to delete repo: {e}")),
            },
            OrchestratorRequest::GetCommits { slug, limit } => {
                match self.git.get_commits(&slug, limit).await {
                    Ok(commits) => IpcResponse::ok_data(commits),
                    Err(e) => IpcResponse::err(format!("Failed to get commits: {e}")),
                }
            }
            OrchestratorRequest::GetBranches { slug } => match self.git.get_branches(&slug).await {
                Ok(branches) => IpcResponse::ok_data(branches),
                Err(e) => IpcResponse::err(format!("Failed to get branches: {e}")),
            },
            OrchestratorRequest::TriggerSync { slug } => match self.git.trigger_sync(&slug).await {
                Ok(()) => IpcResponse::ok_empty(),
                Err(e) => IpcResponse::err(format!("Failed to sync: {e}")),
            },
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
            OrchestratorRequest::GetSshKey => match self.git.get_ssh_key().await {
                Ok(info) => IpcResponse::ok_data(info),
                Err(e) => IpcResponse::err(format!("Failed to get SSH key: {e}")),
            },
            OrchestratorRequest::GenerateSshKey => match self.git.generate_ssh_key().await {
                Ok(info) => IpcResponse::ok_data(info),
                Err(e) => IpcResponse::err(format!("Failed to generate SSH key: {e}")),
            },
            OrchestratorRequest::GetGitConfig => match self.git.load_config().await {
                Ok(config) => IpcResponse::ok_data(config),
                Err(e) => IpcResponse::err(format!("Failed to load git config: {e}")),
            },
            OrchestratorRequest::UpdateGitConfig { config } => {
                let cfg: hr_git::types::GitConfig = match serde_json::from_value(config) {
                    Ok(c) => c,
                    Err(e) => return IpcResponse::err(format!("Invalid git config: {e}")),
                };
                match self.git.save_config(&cfg).await {
                    Ok(()) => {
                        for (slug, mirror) in &cfg.mirrors {
                            if mirror.enabled {
                                if let Err(e) = self.git.enable_mirror(slug, &cfg.github_org).await
                                {
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

            // ── Host operations ──────────────────────────────────
            OrchestratorRequest::ListHostConnections => {
                let conns = self.registry.host_connections.read().await;
                let hosts: Vec<serde_json::Value> = conns
                    .iter()
                    .map(|(id, conn)| {
                        serde_json::json!({
                            "host_id": id,
                            "host_name": conn.host_name,
                            "connected_at": conn.connected_at.to_rfc3339(),
                            "last_heartbeat": conn.last_heartbeat.to_rfc3339(),
                            "version": conn.version,
                            "metrics": conn.metrics,
                            "containers": conn.containers,
                        })
                    })
                    .collect();
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
                if host_id == BACKUP_SERVER_HOST_ID
                    && (action == "shutdown" || action == "poweroff")
                {
                    return IpcResponse::ok_data(
                        serde_json::json!({"status": "blocked", "reason": "Cannot power off the local backup server"}),
                    );
                }
                let power_action = match action.as_str() {
                    "shutdown" => PowerAction::Shutdown,
                    "reboot" => PowerAction::Reboot,
                    _ => return IpcResponse::err(format!("Unknown power action: {action}")),
                };
                if let Err(e) = self
                    .registry
                    .request_power_action(&host_id, power_action)
                    .await
                {
                    return IpcResponse::err(e);
                }
                let msg = match action.as_str() {
                    "shutdown" => HostRegistryMessage::PowerOff,
                    "reboot" => HostRegistryMessage::Reboot,
                    _ => return IpcResponse::err(format!("Unknown power action: {action}")),
                };
                match self.registry.send_host_command(&host_id, msg).await {
                    Ok(()) => IpcResponse::ok_empty(),
                    Err(e) => IpcResponse::err(e),
                }
            }

            // ── Updates scan ─────────────────────────────────────
            OrchestratorRequest::ScanUpdates => {
                self.registry.refresh_latest_versions().await;
                let host_count = self.registry.trigger_host_update_scan().await;
                IpcResponse::ok_data(serde_json::json!({"agents_scanned": host_count}))
            }
            OrchestratorRequest::GetScanResults => {
                let results = self.registry.scan_results.read().await;
                IpcResponse::ok_data(&*results)
            }
            OrchestratorRequest::StoreScanResult { target } => {
                if let (Some(id), Ok(t)) = (
                    target
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    serde_json::from_value::<hr_common::events::UpdateTarget>(target),
                ) {
                    self.registry.scan_results.write().await.insert(id, t);
                    IpcResponse::ok_empty()
                } else {
                    IpcResponse::err("invalid target")
                }
            }

            // ── Agent metrics ────────────────────────────────────
            OrchestratorRequest::GetAgentMetrics => {
                let metrics = self.registry.get_all_metrics().await;
                IpcResponse::ok_data(metrics)
            }

            // ── Agent auth ───────────────────────────────────────
            OrchestratorRequest::AuthenticateAgentToken { token } => {
                match self.registry.authenticate_by_token(&token).await {
                    Some((app_id, slug)) => IpcResponse::ok_data(serde_json::json!({
                        "app_id": app_id,
                        "slug": slug,
                    })),
                    None => IpcResponse::err("Invalid token"),
                }
            }

            // ── Backup pipeline ───────────────────────────────────
            OrchestratorRequest::TriggerBackup => match self.backup.trigger().await {
                Ok(()) => IpcResponse::ok_data(serde_json::json!({
                    "message": "Backup pipeline started",
                    "status": "running"
                })),
                Err(e) => IpcResponse::err(e),
            },
            OrchestratorRequest::GetBackupStatus => {
                let status = self.backup.get_status().await;
                IpcResponse::ok_data(status)
            }
            OrchestratorRequest::GetBackupRepos => {
                let repos = self.backup.get_repos().await;
                IpcResponse::ok_data(repos)
            }
            OrchestratorRequest::GetBackupJobs => {
                let jobs = self.backup.get_jobs().await;
                IpcResponse::ok_data(jobs)
            }
            OrchestratorRequest::GetBackupProgress => {
                let progress = self.backup.get_progress().await;
                IpcResponse::ok_data(progress)
            }
            OrchestratorRequest::CancelBackup => match self.backup.cancel().await {
                Ok(()) => IpcResponse::ok_data(serde_json::json!({"message": "Backup cancelled"})),
                Err(e) => IpcResponse::err(e),
            },
            OrchestratorRequest::GetBackupLive => {
                let status = self.backup.get_status().await;
                let repos = self.backup.get_repos().await;
                let jobs = self.backup.get_jobs().await;
                let progress = self.backup.get_progress().await;
                IpcResponse::ok_data(serde_json::json!({
                    "status": serde_json::to_value(&status).unwrap_or_default(),
                    "repos": serde_json::to_value(&repos).unwrap_or_default(),
                    "jobs": serde_json::to_value(&jobs).unwrap_or_default(),
                    "progress": serde_json::to_value(&progress).unwrap_or_default(),
                }))
            }

            // ── Apps (V3 — direct supervision via hr-apps) ────────
            OrchestratorRequest::AppList => self.apps_ctx().list().await,
            OrchestratorRequest::AppGet { slug } => self.apps_ctx().get(&slug).await,
            OrchestratorRequest::AppCreate {
                slug,
                name,
                stack,
                has_db,
                visibility,
                run_command,
                build_command,
                health_path,
            } => {
                self.apps_ctx()
                    .create(
                        slug,
                        name,
                        stack,
                        has_db,
                        visibility,
                        run_command,
                        build_command,
                        health_path,
                    )
                    .await
            }
            OrchestratorRequest::AppUpdate {
                slug,
                name,
                visibility,
                run_command,
                build_command,
                health_path,
                env_vars,
            } => {
                self.apps_ctx()
                    .update(
                        slug,
                        name,
                        visibility,
                        run_command,
                        build_command,
                        health_path,
                        env_vars,
                    )
                    .await
            }
            OrchestratorRequest::AppDelete { slug, keep_data } => {
                self.apps_ctx().delete(slug, keep_data).await
            }
            OrchestratorRequest::AppControl { slug, action } => {
                self.apps_ctx().control(slug, action).await
            }
            OrchestratorRequest::AppStatus { slug } => self.apps_ctx().status(&slug).await,
            OrchestratorRequest::AppLogs { slug, limit, level } => {
                self.apps_ctx().logs(slug, limit, level).await
            }
            OrchestratorRequest::AppExec {
                slug,
                command,
                timeout_secs,
            } => self.apps_ctx().exec(slug, command, timeout_secs).await,
            OrchestratorRequest::AppRegenerateContext { slug } => {
                self.apps_ctx().regenerate_context(slug).await
            }
            OrchestratorRequest::AppDbListTables { slug } => {
                self.apps_ctx().db_list_tables(slug).await
            }
            OrchestratorRequest::AppDbDescribeTable { slug, table } => {
                self.apps_ctx().db_describe_table(slug, table).await
            }
            OrchestratorRequest::AppDbQuery { slug, sql, params } => {
                self.apps_ctx().db_query(slug, sql, params).await
            }
            OrchestratorRequest::AppDbExecute { slug, sql, params } => {
                self.apps_ctx().db_execute(slug, sql, params).await
            }
            OrchestratorRequest::AppDbSnapshot { slug } => self.apps_ctx().db_snapshot(slug).await,
        }
    }
}
