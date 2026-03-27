use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tracing::{error, info, warn};

use crate::store::PipelineStore;
use crate::types::*;

/// Context passed to the step handler for each step execution.
#[derive(Debug, Clone)]
pub struct StepContext {
    pub run_id: String,
    pub app_slug: String,
    pub version: String,
    pub source_env: String,
    pub target_env: String,
    pub step_config: serde_json::Value,
}

/// Trait for executing pipeline steps.
/// Implemented by the orchestrator to dispatch to env-agents via WebSocket.
pub trait PipelineStepHandler: Send + Sync {
    /// Execute a single step. Returns Ok(output_message) or Err(error_message).
    fn execute_step(
        &self,
        step_type: PipelineStepType,
        context: &StepContext,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Rollback after a failure. Called when a step fails and prior steps need undoing.
    fn rollback(
        &self,
        context: &StepContext,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// Pipeline runner: orchestrates sequential step execution with rollback support.
pub struct PipelineRunner {
    store: Arc<PipelineStore>,
}

impl PipelineRunner {
    pub fn new(store: Arc<PipelineStore>) -> Self {
        Self { store }
    }

    /// Execute a pipeline run to completion.
    ///
    /// Creates a run record, executes steps sequentially, updates state in the store,
    /// and performs rollback if any step fails.
    pub async fn execute(
        &self,
        def: &PipelineDefinition,
        source_env: &str,
        target_env: &str,
        version: &str,
        triggered_by: &str,
        handler: &dyn PipelineStepHandler,
    ) -> PipelineRun {
        // Create the run record
        let run = self
            .store
            .create_run(
                &def.id,
                &def.app_slug,
                version,
                source_env,
                target_env,
                triggered_by,
                &def.steps,
            )
            .await;
        let run_id = run.id.clone();

        info!(
            run_id = %run_id,
            app = %def.app_slug,
            version = %version,
            "{source_env} -> {target_env}: pipeline started ({} steps)",
            def.steps.len()
        );

        self.store.start_run(&run_id).await;

        let context = StepContext {
            run_id: run_id.clone(),
            app_slug: def.app_slug.clone(),
            version: version.to_string(),
            source_env: source_env.to_string(),
            target_env: target_env.to_string(),
            step_config: serde_json::Value::Null,
        };

        let mut failed = false;
        let mut failed_step_idx: Option<usize> = None;

        for (idx, step_def) in def.steps.iter().enumerate() {
            let step_context = StepContext {
                step_config: step_def.config.clone(),
                ..context.clone()
            };

            info!(run_id = %run_id, step = %step_def.name, "Step {}/{} starting", idx + 1, def.steps.len());

            // Mark step as running
            self.store
                .update_step(&run_id, &step_def.name, StepStatus::Running, "")
                .await;

            // Execute with timeout
            let timeout = tokio::time::Duration::from_secs(step_def.timeout_secs);
            let result =
                tokio::time::timeout(timeout, handler.execute_step(step_def.step_type, &step_context))
                    .await;

            match result {
                Ok(Ok(output)) => {
                    info!(run_id = %run_id, step = %step_def.name, "Step succeeded");
                    self.store
                        .update_step(&run_id, &step_def.name, StepStatus::Success, &output)
                        .await;
                }
                Ok(Err(err)) => {
                    error!(run_id = %run_id, step = %step_def.name, error = %err, "Step failed");
                    self.store
                        .update_step(&run_id, &step_def.name, StepStatus::Failed, &err)
                        .await;
                    failed = true;
                    failed_step_idx = Some(idx);
                    break;
                }
                Err(_elapsed) => {
                    let msg = format!("Step timed out after {}s", step_def.timeout_secs);
                    error!(run_id = %run_id, step = %step_def.name, "{msg}");
                    self.store
                        .update_step(&run_id, &step_def.name, StepStatus::Failed, &msg)
                        .await;
                    failed = true;
                    failed_step_idx = Some(idx);
                    break;
                }
            }
        }

        // If failed, skip remaining steps and attempt rollback
        if failed {
            let fail_idx = failed_step_idx.unwrap();

            // Mark remaining steps as skipped
            for step_def in &def.steps[(fail_idx + 1)..] {
                self.store
                    .update_step(&run_id, &step_def.name, StepStatus::Skipped, "")
                    .await;
            }

            // Attempt rollback if any deploy-type step had succeeded
            let has_deployed = def.steps[..fail_idx]
                .iter()
                .any(|s| s.step_type == PipelineStepType::Deploy);

            if has_deployed {
                warn!(run_id = %run_id, "Attempting rollback after failure");
                match handler.rollback(&context).await {
                    Ok(msg) => {
                        info!(run_id = %run_id, "Rollback succeeded: {msg}");
                        self.store
                            .complete_run(&run_id, PipelineStatus::RolledBack)
                            .await;
                    }
                    Err(err) => {
                        error!(run_id = %run_id, "Rollback failed: {err}");
                        self.store
                            .complete_run(&run_id, PipelineStatus::Failed)
                            .await;
                    }
                }
            } else {
                self.store
                    .complete_run(&run_id, PipelineStatus::Failed)
                    .await;
            }
        } else {
            info!(run_id = %run_id, "Pipeline completed successfully");
            self.store
                .complete_run(&run_id, PipelineStatus::Success)
                .await;
        }

        self.store.get_run(&run_id).await.unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// A test handler that succeeds on all steps.
    struct SuccessHandler;

    impl PipelineStepHandler for SuccessHandler {
        fn execute_step(
            &self,
            step_type: PipelineStepType,
            _context: &StepContext,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            Box::pin(async move { Ok(format!("{step_type:?} completed")) })
        }

        fn rollback(
            &self,
            _context: &StepContext,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            Box::pin(async { Ok("rolled back".into()) })
        }
    }

    /// A handler that fails on a specific step type.
    struct FailOnStep {
        fail_on: PipelineStepType,
        executed: Mutex<Vec<PipelineStepType>>,
    }

    impl PipelineStepHandler for FailOnStep {
        fn execute_step(
            &self,
            step_type: PipelineStepType,
            _context: &StepContext,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            self.executed.lock().unwrap().push(step_type);
            let fail_on = self.fail_on;
            Box::pin(async move {
                if step_type == fail_on {
                    Err(format!("{step_type:?} failed intentionally"))
                } else {
                    Ok(format!("{step_type:?} ok"))
                }
            })
        }

        fn rollback(
            &self,
            _context: &StepContext,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            Box::pin(async { Ok("rolled back".into()) })
        }
    }

    fn make_def() -> PipelineDefinition {
        PipelineDefinition {
            id: "pipe-test".into(),
            app_slug: "testapp".into(),
            steps: vec![
                PipelineStepDef {
                    name: "test".into(),
                    step_type: PipelineStepType::Test,
                    timeout_secs: 60,
                    config: serde_json::Value::Null,
                },
                PipelineStepDef {
                    name: "deploy".into(),
                    step_type: PipelineStepType::Deploy,
                    timeout_secs: 120,
                    config: serde_json::Value::Null,
                },
                PipelineStepDef {
                    name: "health-check".into(),
                    step_type: PipelineStepType::HealthCheck,
                    timeout_secs: 30,
                    config: serde_json::Value::Null,
                },
            ],
        }
    }

    #[tokio::test]
    async fn test_successful_pipeline() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Arc::new(PipelineStore::new(tmp.path().to_path_buf()));
        let runner = PipelineRunner::new(store.clone());
        let handler = SuccessHandler;

        let run = runner
            .execute(&make_def(), "dev", "prod", "1.0.0", "tester", &handler)
            .await;

        assert_eq!(run.status, PipelineStatus::Success);
        assert!(run.finished_at.is_some());
        assert!(run.steps.iter().all(|s| s.status == StepStatus::Success));
        assert!(run.steps[0].output.contains("Test completed"));
    }

    #[tokio::test]
    async fn test_failure_skips_remaining() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Arc::new(PipelineStore::new(tmp.path().to_path_buf()));
        let runner = PipelineRunner::new(store.clone());
        let handler = FailOnStep {
            fail_on: PipelineStepType::Deploy,
            executed: Mutex::new(vec![]),
        };

        let run = runner
            .execute(&make_def(), "dev", "prod", "1.0.0", "tester", &handler)
            .await;

        assert_eq!(run.status, PipelineStatus::Failed);
        assert_eq!(run.steps[0].status, StepStatus::Success); // test passed
        assert_eq!(run.steps[1].status, StepStatus::Failed); // deploy failed
        assert_eq!(run.steps[2].status, StepStatus::Skipped); // health-check skipped

        // Only test and deploy were executed
        let executed = handler.executed.lock().unwrap();
        assert_eq!(executed.len(), 2);
    }

    #[tokio::test]
    async fn test_rollback_after_deploy_failure() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Arc::new(PipelineStore::new(tmp.path().to_path_buf()));
        let runner = PipelineRunner::new(store.clone());

        // Pipeline: test -> deploy -> health-check (fail on health-check, after deploy succeeds)
        let handler = FailOnStep {
            fail_on: PipelineStepType::HealthCheck,
            executed: Mutex::new(vec![]),
        };

        let run = runner
            .execute(&make_def(), "dev", "prod", "1.0.0", "tester", &handler)
            .await;

        // Deploy succeeded but health-check failed -> rollback triggered
        assert_eq!(run.status, PipelineStatus::RolledBack);
        assert_eq!(run.steps[0].status, StepStatus::Success);
        assert_eq!(run.steps[1].status, StepStatus::Success);
        assert_eq!(run.steps[2].status, StepStatus::Failed);
    }
}
