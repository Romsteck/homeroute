use std::collections::HashSet;

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
    /// Build the application from source.
    Build,
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
    /// Chain ID linking multiple runs in an env chain promotion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    /// Artifact URL produced by the build step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_url: Option<String>,
    /// SHA256 hash of the artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
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
    /// Waiting for a gate approval before continuing the chain.
    #[serde(rename = "waiting_gate")]
    WaitingGate,
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

/// Per-app pipeline configuration: env chain, skip steps, auto-promote, gates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// App slug this config belongs to.
    pub app_slug: String,
    /// Ordered list of environments for promotion (e.g. ["dev", "acc", "prod"]).
    pub env_chain: Vec<String>,
    /// Step names to skip (e.g. {"test"}).
    #[serde(default)]
    pub skip_steps: HashSet<String>,
    /// Envs where auto-promote is enabled after successful deploy
    /// (e.g. {"dev"} = auto promote after deploy in dev).
    #[serde(default)]
    pub auto_promote: HashSet<String>,
    /// Gate definitions requiring manual approval between envs.
    #[serde(default)]
    pub gates: Vec<GateDef>,
}

/// A gate definition between two environments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateDef {
    /// Source environment.
    pub from_env: String,
    /// Target environment.
    pub to_env: String,
}

/// Status of a gate approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateStatus {
    Pending,
    Approved,
    Rejected,
}

/// A gate approval record for a chain promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateApproval {
    /// Unique gate approval ID.
    pub id: String,
    /// Chain ID this gate belongs to.
    pub chain_id: String,
    /// App slug.
    pub app_slug: String,
    /// Source environment.
    pub from_env: String,
    /// Target environment.
    pub to_env: String,
    /// Version being promoted.
    pub version: String,
    /// Current gate status.
    pub status: GateStatus,
    /// When the gate was created.
    pub created_at: DateTime<Utc>,
    /// When the gate was resolved (approved or rejected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    /// Who resolved the gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
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
            chain_id: None,
            artifact_url: None,
            artifact_sha256: None,
        };
        let json = serde_json::to_string_pretty(&run).unwrap();
        let parsed: PipelineRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, PipelineStatus::Running);
        assert_eq!(parsed.steps.len(), 2);
        assert_eq!(parsed.steps[0].status, StepStatus::Success);
        // New optional fields should be None
        assert!(parsed.chain_id.is_none());
        assert!(parsed.artifact_url.is_none());
        assert!(parsed.artifact_sha256.is_none());
    }

    #[test]
    fn test_pipeline_run_serde_with_chain() {
        let run = PipelineRun {
            id: "run-002".into(),
            pipeline_id: "pipe-trader".into(),
            app_slug: "trader".into(),
            version: "abc1234".into(),
            source_env: "dev".into(),
            target_env: "acc".into(),
            status: PipelineStatus::WaitingGate,
            steps: vec![],
            triggered_by: "ci".into(),
            started_at: Utc::now(),
            finished_at: None,
            chain_id: Some("chain-001".into()),
            artifact_url: Some("http://10.0.0.254:4001/artifacts/trader/abc1234.tar.gz".into()),
            artifact_sha256: Some("abcdef1234567890".into()),
        };
        let json = serde_json::to_string_pretty(&run).unwrap();
        let parsed: PipelineRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, PipelineStatus::WaitingGate);
        assert_eq!(parsed.chain_id.as_deref(), Some("chain-001"));
        assert!(parsed.artifact_url.is_some());
        assert!(parsed.artifact_sha256.is_some());
    }

    #[test]
    fn test_pipeline_run_backward_compat() {
        // Simulate a JSON from before the new fields were added
        let old_json = r#"{
            "id": "run-old",
            "pipeline_id": "pipe-1",
            "app_slug": "wallet",
            "version": "1.0",
            "source_env": "dev",
            "target_env": "prod",
            "status": "running",
            "steps": [],
            "triggered_by": "user",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": null
        }"#;
        let parsed: PipelineRun = serde_json::from_str(old_json).unwrap();
        assert_eq!(parsed.app_slug, "wallet");
        assert!(parsed.chain_id.is_none());
        assert!(parsed.artifact_url.is_none());
        assert!(parsed.artifact_sha256.is_none());
    }

    #[test]
    fn test_pipeline_config_serde() {
        let config = PipelineConfig {
            app_slug: "trader".into(),
            env_chain: vec!["dev".into(), "acc".into(), "prod".into()],
            skip_steps: HashSet::from(["test".into()]),
            auto_promote: HashSet::from(["dev".into()]),
            gates: vec![GateDef {
                from_env: "acc".into(),
                to_env: "prod".into(),
            }],
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.env_chain.len(), 3);
        assert!(parsed.skip_steps.contains("test"));
        assert!(parsed.auto_promote.contains("dev"));
        assert_eq!(parsed.gates.len(), 1);
    }

    #[test]
    fn test_gate_approval_serde() {
        let gate = GateApproval {
            id: "gate-001".into(),
            chain_id: "chain-001".into(),
            app_slug: "trader".into(),
            from_env: "acc".into(),
            to_env: "prod".into(),
            version: "1.2.3".into(),
            status: GateStatus::Pending,
            created_at: Utc::now(),
            resolved_at: None,
            resolved_by: None,
        };
        let json = serde_json::to_string_pretty(&gate).unwrap();
        let parsed: GateApproval = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, GateStatus::Pending);
        assert!(parsed.resolved_at.is_none());
    }
}
