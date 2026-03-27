use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A deployment pipeline definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDefinition {
    /// Unique pipeline template ID.
    pub id: String,
    /// App slug this pipeline is for.
    pub app_slug: String,
    /// Ordered steps to execute.
    pub steps: Vec<PipelineStepDef>,
}

/// A step in a pipeline definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStepDef {
    /// Step name (e.g., "test", "backup-db", "migrate-db", "deploy", "health-check").
    pub name: String,
    /// Step type.
    pub step_type: PipelineStepType,
    /// Timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Custom configuration for this step.
    #[serde(default)]
    pub config: serde_json::Value,
}

/// Types of pipeline steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStepType {
    /// Run tests in the source environment.
    Test,
    /// Snapshot the target DB before migration.
    BackupDb,
    /// Diff schemas and apply migrations.
    MigrateDb,
    /// Copy binary + assets to target environment.
    Deploy,
    /// HTTP health check after deploy.
    HealthCheck,
    /// Custom shell command.
    Custom,
}

/// A running or completed pipeline execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    /// Unique run ID.
    pub id: String,
    /// Pipeline definition ID.
    pub pipeline_id: String,
    /// App being deployed.
    pub app_slug: String,
    /// Version being promoted.
    pub version: String,
    /// Source environment slug.
    pub source_env: String,
    /// Target environment slug.
    pub target_env: String,
    /// Current status.
    pub status: PipelineStatus,
    /// Steps with their execution status.
    pub steps: Vec<PipelineStepRun>,
    /// Who triggered this pipeline.
    pub triggered_by: String,
    /// When the pipeline started.
    pub started_at: DateTime<Utc>,
    /// When the pipeline finished (if done).
    pub finished_at: Option<DateTime<Utc>>,
}

/// Status of a pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStatus {
    /// Queued, waiting to start.
    Pending,
    /// Currently executing.
    Running,
    /// All steps completed successfully.
    Success,
    /// A step failed, pipeline stopped.
    Failed,
    /// Pipeline was rolled back after failure.
    RolledBack,
    /// Cancelled by user.
    Cancelled,
}

/// Execution state of a single pipeline step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStepRun {
    /// Step name.
    pub name: String,
    /// Step type.
    pub step_type: PipelineStepType,
    /// Execution status.
    pub status: StepStatus,
    /// Output/logs from this step.
    #[serde(default)]
    pub output: String,
    /// When this step started.
    pub started_at: Option<DateTime<Utc>>,
    /// When this step finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// Status of a pipeline step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
}

fn default_timeout() -> u64 {
    120
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_run_serde() {
        let run = PipelineRun {
            id: "run-001".into(),
            pipeline_id: "pipe-trader".into(),
            app_slug: "trader".into(),
            version: "2.3.1".into(),
            source_env: "dev".into(),
            target_env: "prod".into(),
            status: PipelineStatus::Running,
            steps: vec![
                PipelineStepRun {
                    name: "test".into(),
                    step_type: PipelineStepType::Test,
                    status: StepStatus::Success,
                    output: "All 42 tests passed".into(),
                    started_at: Some(Utc::now()),
                    finished_at: Some(Utc::now()),
                },
                PipelineStepRun {
                    name: "deploy".into(),
                    step_type: PipelineStepType::Deploy,
                    status: StepStatus::Running,
                    output: String::new(),
                    started_at: Some(Utc::now()),
                    finished_at: None,
                },
            ],
            triggered_by: "claude".into(),
            started_at: Utc::now(),
            finished_at: None,
        };
        let json = serde_json::to_string_pretty(&run).unwrap();
        let parsed: PipelineRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, PipelineStatus::Running);
        assert_eq!(parsed.steps.len(), 2);
        assert_eq!(parsed.steps[0].status, StepStatus::Success);
    }
}
