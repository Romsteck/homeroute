use crate::types::*;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{error, info, warn};

/// Result of a pipeline step, sent via oneshot channel.
pub struct StepResult {
    pub success: bool,
    pub message: String,
}

/// Callback trait for the pipeline engine to communicate with the orchestrator.
/// The orchestrator implements this to send messages to env-agents.
pub trait PipelineTransport: Send + Sync + 'static {
    /// Send a command to an env-agent (by env slug).
    fn send_to_env(
        &self,
        env_slug: &str,
        msg: hr_environment::EnvOrchestratorMessage,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Check if an env-agent is connected.
    fn is_env_connected(&self, env_slug: &str) -> impl std::future::Future<Output = bool> + Send;

    /// Get the app version in a specific environment.
    fn get_app_version(
        &self,
        env_slug: &str,
        app_slug: &str,
    ) -> impl std::future::Future<Output = Option<String>> + Send;
}

/// Maximum number of pipeline runs to keep in memory.
const MAX_RUNS: usize = 100;

/// Pipeline execution engine.
///
/// Orchestrates app promotion from one environment to another,
/// with DB migration, health checks, and rollback.
pub struct PipelineEngine {
    /// Active pipeline runs.
    runs: Arc<RwLock<Vec<PipelineRun>>>,
    /// Pipeline step completion signals: "{pipeline_id}:{step_name}" → oneshot sender.
    step_signals: Arc<RwLock<HashMap<String, oneshot::Sender<StepResult>>>>,
}

impl PipelineEngine {
    pub fn new() -> Self {
        Self {
            runs: Arc::new(RwLock::new(Vec::new())),
            step_signals: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Promote an app from source_env to target_env.
    ///
    /// Creates a pipeline run and spawns its execution in a background task.
    /// Returns the PipelineRun immediately so the caller can poll status.
    pub async fn promote<T: PipelineTransport>(
        self: &Arc<Self>,
        transport: &Arc<T>,
        app_slug: String,
        version: String,
        source_env: String,
        target_env: String,
        triggered_by: String,
        custom_steps: Option<Vec<PipelineStepDef>>,
    ) -> anyhow::Result<PipelineRun> {
        // Check that both envs are connected.
        if !transport.is_env_connected(&source_env).await {
            anyhow::bail!(
                "Source environment '{}' is not connected",
                source_env
            );
        }
        if !transport.is_env_connected(&target_env).await {
            anyhow::bail!(
                "Target environment '{}' is not connected",
                target_env
            );
        }

        let steps_def = custom_steps.unwrap_or_else(Self::default_steps);
        let run_id = uuid::Uuid::new_v4().to_string();

        let step_runs: Vec<PipelineStepRun> = steps_def
            .iter()
            .map(|s| PipelineStepRun {
                name: s.name.clone(),
                step_type: s.step_type,
                status: StepStatus::Pending,
                output: String::new(),
                started_at: None,
                finished_at: None,
            })
            .collect();

        let run = PipelineRun {
            id: run_id.clone(),
            pipeline_id: format!("promote-{}-{}-to-{}", app_slug, source_env, target_env),
            app_slug: app_slug.clone(),
            version: version.clone(),
            source_env: source_env.clone(),
            target_env: target_env.clone(),
            status: PipelineStatus::Pending,
            steps: step_runs,
            triggered_by,
            started_at: Utc::now(),
            finished_at: None,
        };

        // Store the run.
        {
            let mut runs = self.runs.write().await;
            runs.push(run.clone());
            // Keep bounded.
            if runs.len() > MAX_RUNS {
                let excess = runs.len() - MAX_RUNS;
                runs.drain(0..excess);
            }
        }

        // Spawn background execution.
        let engine = Arc::clone(self);
        let transport = Arc::clone(transport);
        let steps_def_owned = steps_def;
        tokio::spawn(async move {
            engine
                .execute_pipeline(transport, run_id, steps_def_owned)
                .await;
        });

        Ok(run)
    }

    /// Execute a pipeline run: iterate through steps sequentially.
    async fn execute_pipeline<T: PipelineTransport>(
        self: Arc<Self>,
        transport: Arc<T>,
        run_id: String,
        steps_def: Vec<PipelineStepDef>,
    ) {
        // Mark pipeline as Running.
        self.update_run_status(&run_id, PipelineStatus::Running)
            .await;

        let (app_slug, target_env) = {
            let runs = self.runs.read().await;
            let run = runs.iter().find(|r| r.id == run_id);
            match run {
                Some(r) => (r.app_slug.clone(), r.target_env.clone()),
                None => {
                    error!("Pipeline run {} not found", run_id);
                    return;
                }
            }
        };

        for step_def in &steps_def {
            // Check if pipeline was cancelled.
            {
                let runs = self.runs.read().await;
                if let Some(run) = runs.iter().find(|r| r.id == run_id) {
                    if run.status == PipelineStatus::Cancelled {
                        info!("Pipeline {} was cancelled, stopping execution", run_id);
                        return;
                    }
                }
            }

            // Mark step as Running.
            self.update_step_status(&run_id, &step_def.name, StepStatus::Running, None)
                .await;

            let result = self
                .execute_step(&transport, &run_id, &app_slug, &target_env, step_def)
                .await;

            match result {
                Ok(output) => {
                    self.update_step_status(
                        &run_id,
                        &step_def.name,
                        StepStatus::Success,
                        Some(output),
                    )
                    .await;
                    info!(
                        "Pipeline {} step '{}' completed successfully",
                        run_id, step_def.name
                    );
                }
                Err(err) => {
                    let error_msg = format!("{:#}", err);
                    error!(
                        "Pipeline {} step '{}' failed: {}",
                        run_id, step_def.name, error_msg
                    );
                    self.update_step_status(
                        &run_id,
                        &step_def.name,
                        StepStatus::Failed,
                        Some(error_msg),
                    )
                    .await;

                    // Mark remaining steps as Skipped.
                    self.skip_remaining_steps(&run_id, &step_def.name).await;

                    // Attempt rollback.
                    info!("Pipeline {} triggering rollback", run_id);
                    let rollback_ok = self
                        .execute_rollback(&transport, &run_id, &app_slug, &target_env)
                        .await;

                    if rollback_ok {
                        self.update_run_status(&run_id, PipelineStatus::RolledBack)
                            .await;
                    } else {
                        self.update_run_status(&run_id, PipelineStatus::Failed)
                            .await;
                    }

                    // Mark finished.
                    self.mark_run_finished(&run_id).await;
                    return;
                }
            }
        }

        // All steps succeeded.
        self.update_run_status(&run_id, PipelineStatus::Success)
            .await;
        self.mark_run_finished(&run_id).await;
        info!("Pipeline {} completed successfully", run_id);
    }

    /// Execute a single pipeline step.
    async fn execute_step<T: PipelineTransport>(
        &self,
        transport: &Arc<T>,
        run_id: &str,
        app_slug: &str,
        target_env: &str,
        step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        match step_def.step_type {
            PipelineStepType::Test => {
                self.execute_test(transport, run_id, app_slug, target_env, step_def)
                    .await
            }
            PipelineStepType::BackupDb => {
                self.execute_backup_db(transport, run_id, app_slug, target_env, step_def)
                    .await
            }
            PipelineStepType::MigrateDb => {
                self.execute_migrate_db(transport, run_id, app_slug, target_env, step_def)
                    .await
            }
            PipelineStepType::Deploy => {
                self.execute_deploy(transport, run_id, app_slug, target_env, step_def)
                    .await
            }
            PipelineStepType::HealthCheck => {
                self.execute_health_check(transport, run_id, app_slug, target_env, step_def)
                    .await
            }
            PipelineStepType::Custom => {
                // Custom steps are not yet implemented.
                warn!(
                    "Pipeline {} custom step '{}' skipped (not implemented)",
                    run_id, step_def.name
                );
                Ok("Custom step skipped (not implemented)".to_string())
            }
        }
    }

    /// Test step: currently just logs that tests are skipped (future feature).
    async fn execute_test<T: PipelineTransport>(
        &self,
        _transport: &Arc<T>,
        run_id: &str,
        _app_slug: &str,
        _target_env: &str,
        _step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        info!("Pipeline {} test step: tests skipped (future feature)", run_id);
        Ok("Tests skipped (future feature)".to_string())
    }

    /// Backup DB step: send SnapshotDb to target env, wait for PipelineProgress.
    async fn execute_backup_db<T: PipelineTransport>(
        &self,
        transport: &Arc<T>,
        run_id: &str,
        app_slug: &str,
        target_env: &str,
        step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        let msg = hr_environment::EnvOrchestratorMessage::SnapshotDb {
            pipeline_id: run_id.to_string(),
            app_slug: app_slug.to_string(),
        };
        transport.send_to_env(target_env, msg).await?;

        let result = self
            .wait_for_step_signal(run_id, &step_def.name, step_def.timeout_secs)
            .await?;

        if result.success {
            Ok(result.message)
        } else {
            anyhow::bail!("DB backup failed: {}", result.message)
        }
    }

    /// Migrate DB step: send MigrateDb to target env, wait for MigrationResult.
    async fn execute_migrate_db<T: PipelineTransport>(
        &self,
        transport: &Arc<T>,
        run_id: &str,
        app_slug: &str,
        target_env: &str,
        step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        // Extract migrations from step config if provided.
        let migrations: Vec<String> = step_def
            .config
            .get("migrations")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let msg = hr_environment::EnvOrchestratorMessage::MigrateDb {
            pipeline_id: run_id.to_string(),
            app_slug: app_slug.to_string(),
            migrations,
        };
        transport.send_to_env(target_env, msg).await?;

        // Wait on the "migrate-db" signal key (uses step name).
        let result = self
            .wait_for_step_signal(run_id, &step_def.name, step_def.timeout_secs)
            .await?;

        if result.success {
            Ok(result.message)
        } else {
            anyhow::bail!("DB migration failed: {}", result.message)
        }
    }

    /// Deploy step: send DeployApp to target env, wait for PipelineProgress.
    async fn execute_deploy<T: PipelineTransport>(
        &self,
        transport: &Arc<T>,
        run_id: &str,
        app_slug: &str,
        target_env: &str,
        step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        // Extract artifact_url and sha256 from step config, or use defaults.
        let artifact_url = step_def
            .config
            .get("artifact_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let sha256 = step_def
            .config
            .get("sha256")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Get the version from the run.
        let version = {
            let runs = self.runs.read().await;
            runs.iter()
                .find(|r| r.id == run_id)
                .map(|r| r.version.clone())
                .unwrap_or_default()
        };

        let msg = hr_environment::EnvOrchestratorMessage::DeployApp {
            pipeline_id: run_id.to_string(),
            app_slug: app_slug.to_string(),
            version,
            artifact_url,
            sha256,
        };
        transport.send_to_env(target_env, msg).await?;

        let result = self
            .wait_for_step_signal(run_id, &step_def.name, step_def.timeout_secs)
            .await?;

        if result.success {
            Ok(result.message)
        } else {
            anyhow::bail!("Deployment failed: {}", result.message)
        }
    }

    /// Health check step: wait a few seconds, then expect a PipelineProgress from the env-agent.
    async fn execute_health_check<T: PipelineTransport>(
        &self,
        _transport: &Arc<T>,
        run_id: &str,
        _app_slug: &str,
        _target_env: &str,
        step_def: &PipelineStepDef,
    ) -> anyhow::Result<String> {
        // Wait a few seconds for the app to start.
        tokio::time::sleep(Duration::from_secs(5)).await;

        let result = self
            .wait_for_step_signal(run_id, &step_def.name, step_def.timeout_secs)
            .await?;

        if result.success {
            Ok(result.message)
        } else {
            anyhow::bail!("Health check failed: {}", result.message)
        }
    }

    /// Attempt to rollback by sending RollbackApp to the target env.
    async fn execute_rollback<T: PipelineTransport>(
        &self,
        transport: &Arc<T>,
        run_id: &str,
        app_slug: &str,
        target_env: &str,
    ) -> bool {
        let msg = hr_environment::EnvOrchestratorMessage::RollbackApp {
            pipeline_id: run_id.to_string(),
            app_slug: app_slug.to_string(),
        };

        match transport.send_to_env(target_env, msg).await {
            Ok(()) => {
                info!("Pipeline {} rollback sent to '{}'", run_id, target_env);
                true
            }
            Err(e) => {
                error!(
                    "Pipeline {} rollback failed to send to '{}': {:#}",
                    run_id, target_env, e
                );
                false
            }
        }
    }

    /// Register a step signal and wait for it with a timeout.
    async fn wait_for_step_signal(
        &self,
        pipeline_id: &str,
        step_name: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<StepResult> {
        let key = format!("{}:{}", pipeline_id, step_name);
        let (tx, rx) = oneshot::channel();

        {
            let mut signals = self.step_signals.write().await;
            signals.insert(key.clone(), tx);
        }

        let timeout = Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => {
                // Sender was dropped without sending — treat as failure.
                anyhow::bail!("Step signal for '{}' was dropped", step_name)
            }
            Err(_) => {
                // Timeout — clean up the signal.
                let mut signals = self.step_signals.write().await;
                signals.remove(&key);
                anyhow::bail!(
                    "Step '{}' timed out after {}s",
                    step_name,
                    timeout_secs
                )
            }
        }
    }

    /// Called by the WS handler when receiving PipelineProgress from an env-agent.
    /// Resolves the corresponding step signal.
    pub async fn on_pipeline_progress(
        &self,
        pipeline_id: &str,
        step: &str,
        success: bool,
        message: Option<String>,
    ) {
        let key = format!("{}:{}", pipeline_id, step);
        let sender = {
            let mut signals = self.step_signals.write().await;
            signals.remove(&key)
        };

        if let Some(tx) = sender {
            let result = StepResult {
                success,
                message: message.unwrap_or_default(),
            };
            if tx.send(result).is_err() {
                warn!(
                    "Failed to deliver pipeline progress for {}:{} (receiver dropped)",
                    pipeline_id, step
                );
            }
        } else {
            warn!(
                "Received pipeline progress for unknown signal {}:{}",
                pipeline_id, step
            );
        }
    }

    /// Called when receiving MigrationResult from an env-agent.
    /// Resolves the migration step signal.
    pub async fn on_migration_result(
        &self,
        pipeline_id: &str,
        _app_slug: &str,
        success: bool,
        migrations_applied: u32,
        error: Option<String>,
    ) {
        // Migration results resolve the "migrate-db" step signal.
        let key = format!("{}:migrate-db", pipeline_id);
        let sender = {
            let mut signals = self.step_signals.write().await;
            signals.remove(&key)
        };

        if let Some(tx) = sender {
            let message = if success {
                format!("{} migrations applied successfully", migrations_applied)
            } else {
                error.unwrap_or_else(|| "Migration failed (no details)".to_string())
            };
            let result = StepResult { success, message };
            if tx.send(result).is_err() {
                warn!(
                    "Failed to deliver migration result for {} (receiver dropped)",
                    pipeline_id
                );
            }
        } else {
            warn!(
                "Received migration result for unknown signal {}",
                pipeline_id
            );
        }
    }

    /// Get all pipeline runs.
    pub async fn get_runs(&self) -> Vec<PipelineRun> {
        self.runs.read().await.clone()
    }

    /// Get a specific pipeline run by ID.
    pub async fn get_run(&self, id: &str) -> Option<PipelineRun> {
        self.runs.read().await.iter().find(|r| r.id == id).cloned()
    }

    /// Cancel a running pipeline.
    pub async fn cancel(&self, id: &str) -> anyhow::Result<()> {
        let mut runs = self.runs.write().await;
        let run = runs
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| anyhow::anyhow!("Pipeline run '{}' not found", id))?;

        match run.status {
            PipelineStatus::Pending | PipelineStatus::Running => {
                run.status = PipelineStatus::Cancelled;
                run.finished_at = Some(Utc::now());
                info!("Pipeline {} cancelled", id);
                Ok(())
            }
            _ => {
                anyhow::bail!(
                    "Pipeline '{}' is in state {:?} and cannot be cancelled",
                    id,
                    run.status
                )
            }
        }
    }

    /// Returns the default pipeline steps: [Test, BackupDb, MigrateDb, Deploy, HealthCheck].
    pub fn default_steps() -> Vec<PipelineStepDef> {
        vec![
            PipelineStepDef {
                name: "test".to_string(),
                step_type: PipelineStepType::Test,
                timeout_secs: 120,
                config: serde_json::Value::Null,
            },
            PipelineStepDef {
                name: "backup-db".to_string(),
                step_type: PipelineStepType::BackupDb,
                timeout_secs: 120,
                config: serde_json::Value::Null,
            },
            PipelineStepDef {
                name: "migrate-db".to_string(),
                step_type: PipelineStepType::MigrateDb,
                timeout_secs: 120,
                config: serde_json::Value::Null,
            },
            PipelineStepDef {
                name: "deploy".to_string(),
                step_type: PipelineStepType::Deploy,
                timeout_secs: 120,
                config: serde_json::Value::Null,
            },
            PipelineStepDef {
                name: "health-check".to_string(),
                step_type: PipelineStepType::HealthCheck,
                timeout_secs: 120,
                config: serde_json::Value::Null,
            },
        ]
    }

    // ── Internal helpers ─────────────────────────────────────────────

    async fn update_run_status(&self, run_id: &str, status: PipelineStatus) {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.iter_mut().find(|r| r.id == run_id) {
            run.status = status;
        }
    }

    async fn mark_run_finished(&self, run_id: &str) {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.iter_mut().find(|r| r.id == run_id) {
            run.finished_at = Some(Utc::now());
        }
    }

    async fn update_step_status(
        &self,
        run_id: &str,
        step_name: &str,
        status: StepStatus,
        output: Option<String>,
    ) {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.iter_mut().find(|r| r.id == run_id) {
            if let Some(step) = run.steps.iter_mut().find(|s| s.name == step_name) {
                step.status = status;
                if let Some(msg) = output {
                    step.output = msg;
                }
                match status {
                    StepStatus::Running => {
                        step.started_at = Some(Utc::now());
                    }
                    StepStatus::Success | StepStatus::Failed | StepStatus::Skipped => {
                        step.finished_at = Some(Utc::now());
                    }
                    StepStatus::Pending => {}
                }
            }
        }
    }

    async fn skip_remaining_steps(&self, run_id: &str, failed_step_name: &str) {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.iter_mut().find(|r| r.id == run_id) {
            let mut found_failed = false;
            for step in run.steps.iter_mut() {
                if step.name == failed_step_name {
                    found_failed = true;
                    continue;
                }
                if found_failed && step.status == StepStatus::Pending {
                    step.status = StepStatus::Skipped;
                    step.finished_at = Some(Utc::now());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Mock transport for testing.
    struct MockTransport {
        source_connected: AtomicBool,
        target_connected: AtomicBool,
    }

    impl MockTransport {
        fn new(source: bool, target: bool) -> Self {
            Self {
                source_connected: AtomicBool::new(source),
                target_connected: AtomicBool::new(target),
            }
        }
    }

    impl PipelineTransport for MockTransport {
        async fn send_to_env(
            &self,
            _env_slug: &str,
            _msg: hr_environment::EnvOrchestratorMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn is_env_connected(&self, env_slug: &str) -> bool {
            if env_slug == "dev" {
                self.source_connected.load(Ordering::Relaxed)
            } else {
                self.target_connected.load(Ordering::Relaxed)
            }
        }

        async fn get_app_version(&self, _env_slug: &str, _app_slug: &str) -> Option<String> {
            Some("1.0.0".to_string())
        }
    }

    #[test]
    fn test_default_steps() {
        let steps = PipelineEngine::default_steps();
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].step_type, PipelineStepType::Test);
        assert_eq!(steps[1].step_type, PipelineStepType::BackupDb);
        assert_eq!(steps[2].step_type, PipelineStepType::MigrateDb);
        assert_eq!(steps[3].step_type, PipelineStepType::Deploy);
        assert_eq!(steps[4].step_type, PipelineStepType::HealthCheck);
    }

    #[tokio::test]
    async fn test_promote_disconnected_source() {
        let engine = Arc::new(PipelineEngine::new());
        let transport = Arc::new(MockTransport::new(false, true));

        let result = engine
            .promote(
                &transport,
                "trader".into(),
                "2.0.0".into(),
                "dev".into(),
                "prod".into(),
                "test-user".into(),
                None,
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not connected"));
    }

    #[tokio::test]
    async fn test_promote_disconnected_target() {
        let engine = Arc::new(PipelineEngine::new());
        let transport = Arc::new(MockTransport::new(true, false));

        let result = engine
            .promote(
                &transport,
                "trader".into(),
                "2.0.0".into(),
                "dev".into(),
                "prod".into(),
                "test-user".into(),
                None,
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not connected"));
    }

    #[tokio::test]
    async fn test_promote_creates_run() {
        let engine = Arc::new(PipelineEngine::new());
        let transport = Arc::new(MockTransport::new(true, true));

        let run = engine
            .promote(
                &transport,
                "trader".into(),
                "2.0.0".into(),
                "dev".into(),
                "prod".into(),
                "claude".into(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(run.app_slug, "trader");
        assert_eq!(run.version, "2.0.0");
        assert_eq!(run.source_env, "dev");
        assert_eq!(run.target_env, "prod");
        assert_eq!(run.status, PipelineStatus::Pending);
        assert_eq!(run.steps.len(), 5);
        assert_eq!(run.triggered_by, "claude");
    }

    #[tokio::test]
    async fn test_cancel_pipeline() {
        let engine = Arc::new(PipelineEngine::new());
        let transport = Arc::new(MockTransport::new(true, true));

        let run = engine
            .promote(
                &transport,
                "trader".into(),
                "2.0.0".into(),
                "dev".into(),
                "prod".into(),
                "claude".into(),
                None,
            )
            .await
            .unwrap();

        // Give a moment for the spawned task to start.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let result = engine.cancel(&run.id).await;
        assert!(result.is_ok());

        let updated = engine.get_run(&run.id).await.unwrap();
        assert_eq!(updated.status, PipelineStatus::Cancelled);
        assert!(updated.finished_at.is_some());
    }

    #[tokio::test]
    async fn test_on_pipeline_progress() {
        let engine = PipelineEngine::new();

        // Register a signal.
        let key = "run-001:deploy".to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut signals = engine.step_signals.write().await;
            signals.insert(key, tx);
        }

        // Fire the progress.
        engine
            .on_pipeline_progress("run-001", "deploy", true, Some("Deployed OK".into()))
            .await;

        let result = rx.await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Deployed OK");
    }

    #[tokio::test]
    async fn test_on_migration_result() {
        let engine = PipelineEngine::new();

        let key = "run-001:migrate-db".to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut signals = engine.step_signals.write().await;
            signals.insert(key, tx);
        }

        engine
            .on_migration_result("run-001", "trader", true, 3, None)
            .await;

        let result = rx.await.unwrap();
        assert!(result.success);
        assert!(result.message.contains("3 migrations"));
    }

    #[tokio::test]
    async fn test_get_runs_empty() {
        let engine = PipelineEngine::new();
        let runs = engine.get_runs().await;
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let engine = Arc::new(PipelineEngine::new());
        let result = engine.cancel("nonexistent").await;
        assert!(result.is_err());
    }
}
