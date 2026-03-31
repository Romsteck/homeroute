use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::types::*;

/// Persisted state for pipelines.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PipelineState {
    definitions: Vec<PipelineDefinition>,
    runs: Vec<PipelineRun>,
    #[serde(default)]
    configs: Vec<PipelineConfig>,
    #[serde(default)]
    gates: Vec<GateApproval>,
}

/// Manages pipeline definitions and run history with JSON persistence.
pub struct PipelineStore {
    state: RwLock<PipelineState>,
    state_path: PathBuf,
}

impl PipelineStore {
    /// Load or create a pipeline store at the given path.
    pub fn new(state_path: PathBuf) -> Self {
        let state = if state_path.exists() {
            match std::fs::read_to_string(&state_path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_else(|e| {
                    warn!("Failed to parse pipeline state: {e}, starting fresh");
                    PipelineState::default()
                }),
                Err(e) => {
                    warn!("Failed to read pipeline state: {e}, starting fresh");
                    PipelineState::default()
                }
            }
        } else {
            PipelineState::default()
        };
        info!(
            "Pipeline store loaded: {} definitions, {} runs, {} configs, {} gates",
            state.definitions.len(),
            state.runs.len(),
            state.configs.len(),
            state.gates.len(),
        );
        Self {
            state: RwLock::new(state),
            state_path,
        }
    }

    /// Create a new pipeline run from a definition.
    pub async fn create_run(
        &self,
        pipeline_id: &str,
        app_slug: &str,
        version: &str,
        source_env: &str,
        target_env: &str,
        triggered_by: &str,
        steps: &[PipelineStepDef],
    ) -> PipelineRun {
        let run = PipelineRun {
            id: format!("run-{}", uuid::Uuid::new_v4()),
            pipeline_id: pipeline_id.to_string(),
            app_slug: app_slug.to_string(),
            version: version.to_string(),
            source_env: source_env.to_string(),
            target_env: target_env.to_string(),
            status: PipelineStatus::Pending,
            steps: steps
                .iter()
                .map(|s| PipelineStepRun {
                    name: s.name.clone(),
                    step_type: s.step_type,
                    status: StepStatus::Pending,
                    output: String::new(),
                    started_at: None,
                    finished_at: None,
                })
                .collect(),
            triggered_by: triggered_by.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            chain_id: None,
            artifact_url: None,
            artifact_sha256: None,
        };
        let mut state = self.state.write().await;
        state.runs.push(run.clone());
        drop(state);
        self.persist().await;
        run
    }

    /// Mark a run as started (Running).
    pub async fn start_run(&self, run_id: &str) {
        let mut state = self.state.write().await;
        if let Some(run) = state.runs.iter_mut().find(|r| r.id == run_id) {
            run.status = PipelineStatus::Running;
            run.started_at = Utc::now();
        }
        drop(state);
        self.persist().await;
    }

    /// Update a step's status and output.
    pub async fn update_step(
        &self,
        run_id: &str,
        step_name: &str,
        status: StepStatus,
        output: &str,
    ) {
        let mut state = self.state.write().await;
        if let Some(run) = state.runs.iter_mut().find(|r| r.id == run_id) {
            if let Some(step) = run.steps.iter_mut().find(|s| s.name == step_name) {
                if step.started_at.is_none() && status == StepStatus::Running {
                    step.started_at = Some(Utc::now());
                }
                step.status = status;
                if !output.is_empty() {
                    if !step.output.is_empty() {
                        step.output.push('\n');
                    }
                    step.output.push_str(output);
                }
                if matches!(status, StepStatus::Success | StepStatus::Failed | StepStatus::Skipped)
                {
                    step.finished_at = Some(Utc::now());
                }
            }
        }
        drop(state);
        self.persist().await;
    }

    /// Complete a pipeline run with a final status.
    pub async fn complete_run(&self, run_id: &str, status: PipelineStatus) {
        let mut state = self.state.write().await;
        if let Some(run) = state.runs.iter_mut().find(|r| r.id == run_id) {
            run.status = status;
            run.finished_at = Some(Utc::now());
        }
        drop(state);
        self.persist().await;
    }

    /// Get a run by ID.
    pub async fn get_run(&self, id: &str) -> Option<PipelineRun> {
        let state = self.state.read().await;
        state.runs.iter().find(|r| r.id == id).cloned()
    }

    /// List runs, optionally filtered by app_slug, newest first.
    pub async fn list_runs(&self, app_slug: Option<&str>, limit: usize) -> Vec<PipelineRun> {
        let state = self.state.read().await;
        let mut runs: Vec<_> = state
            .runs
            .iter()
            .filter(|r| app_slug.map_or(true, |a| r.app_slug == a))
            .cloned()
            .collect();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        runs
    }

    /// Get a pipeline definition for an app.
    pub async fn get_definition(&self, app_slug: &str) -> Option<PipelineDefinition> {
        let state = self.state.read().await;
        state
            .definitions
            .iter()
            .find(|d| d.app_slug == app_slug)
            .cloned()
    }

    /// Save (upsert) a pipeline definition.
    pub async fn save_definition(&self, def: PipelineDefinition) {
        let mut state = self.state.write().await;
        if let Some(existing) = state
            .definitions
            .iter_mut()
            .find(|d| d.app_slug == def.app_slug)
        {
            *existing = def;
        } else {
            state.definitions.push(def);
        }
        drop(state);
        self.persist().await;
    }

    /// List all definitions.
    pub async fn list_definitions(&self) -> Vec<PipelineDefinition> {
        let state = self.state.read().await;
        state.definitions.clone()
    }

    // ── Pipeline Config CRUD ──

    /// Get a pipeline config by app slug.
    pub async fn get_config(&self, app_slug: &str) -> Option<PipelineConfig> {
        let state = self.state.read().await;
        state
            .configs
            .iter()
            .find(|c| c.app_slug == app_slug)
            .cloned()
    }

    /// Save (upsert) a pipeline config by app slug.
    pub async fn save_config(&self, config: PipelineConfig) {
        let mut state = self.state.write().await;
        if let Some(existing) = state
            .configs
            .iter_mut()
            .find(|c| c.app_slug == config.app_slug)
        {
            *existing = config;
        } else {
            state.configs.push(config);
        }
        drop(state);
        self.persist().await;
    }

    /// List all pipeline configs.
    pub async fn list_configs(&self) -> Vec<PipelineConfig> {
        let state = self.state.read().await;
        state.configs.clone()
    }

    // ── Gate lifecycle ──

    /// Create a new pending gate approval.
    pub async fn create_gate(&self, gate: GateApproval) {
        let mut state = self.state.write().await;
        state.gates.push(gate);
        drop(state);
        self.persist().await;
    }

    /// Get a gate by ID.
    pub async fn get_gate(&self, gate_id: &str) -> Option<GateApproval> {
        let state = self.state.read().await;
        state.gates.iter().find(|g| g.id == gate_id).cloned()
    }

    /// Approve a gate, recording who approved and when.
    pub async fn approve_gate(&self, gate_id: &str, approved_by: &str) -> Option<GateApproval> {
        let mut state = self.state.write().await;
        let gate = state.gates.iter_mut().find(|g| g.id == gate_id)?;
        gate.status = GateStatus::Approved;
        gate.resolved_at = Some(Utc::now());
        gate.resolved_by = Some(approved_by.to_string());
        let result = gate.clone();
        drop(state);
        self.persist().await;
        Some(result)
    }

    /// Reject a gate, recording who rejected and when.
    pub async fn reject_gate(&self, gate_id: &str, rejected_by: &str) -> Option<GateApproval> {
        let mut state = self.state.write().await;
        let gate = state.gates.iter_mut().find(|g| g.id == gate_id)?;
        gate.status = GateStatus::Rejected;
        gate.resolved_at = Some(Utc::now());
        gate.resolved_by = Some(rejected_by.to_string());
        let result = gate.clone();
        drop(state);
        self.persist().await;
        Some(result)
    }

    /// List all pending gate approvals, optionally filtered by app slug.
    pub async fn list_pending_gates(&self, app_slug: Option<&str>) -> Vec<GateApproval> {
        let state = self.state.read().await;
        state
            .gates
            .iter()
            .filter(|g| g.status == GateStatus::Pending)
            .filter(|g| app_slug.map_or(true, |a| g.app_slug == a))
            .cloned()
            .collect()
    }

    /// Write state to disk.
    async fn persist(&self) {
        let state = self.state.read().await;
        let json = match serde_json::to_string_pretty(&*state) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize pipeline state: {e}");
                return;
            }
        };
        drop(state);
        if let Some(parent) = self.state_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&self.state_path, json) {
            warn!("Failed to persist pipeline state: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn sample_steps() -> Vec<PipelineStepDef> {
        vec![
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
        ]
    }

    #[tokio::test]
    async fn test_create_and_get_run() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());
        let steps = sample_steps();

        let run = store
            .create_run("pipe-1", "trader", "1.0.0", "dev", "prod", "test-user", &steps)
            .await;
        assert_eq!(run.status, PipelineStatus::Pending);
        assert_eq!(run.steps.len(), 3);

        let fetched = store.get_run(&run.id).await.unwrap();
        assert_eq!(fetched.app_slug, "trader");
        assert_eq!(fetched.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_update_step_and_complete() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());
        let steps = sample_steps();

        let run = store
            .create_run("pipe-1", "trader", "1.0.0", "dev", "prod", "user", &steps)
            .await;

        store.start_run(&run.id).await;
        store
            .update_step(&run.id, "test", StepStatus::Running, "Running tests...")
            .await;
        store
            .update_step(&run.id, "test", StepStatus::Success, "All 10 tests passed")
            .await;
        store
            .complete_run(&run.id, PipelineStatus::Success)
            .await;

        let fetched = store.get_run(&run.id).await.unwrap();
        assert_eq!(fetched.status, PipelineStatus::Success);
        assert!(fetched.finished_at.is_some());
        let test_step = &fetched.steps[0];
        assert_eq!(test_step.status, StepStatus::Success);
        assert!(test_step.output.contains("All 10 tests passed"));
        assert!(test_step.started_at.is_some());
        assert!(test_step.finished_at.is_some());
    }

    #[tokio::test]
    async fn test_list_runs_filtered() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());
        let steps = sample_steps();

        store
            .create_run("p1", "trader", "1.0", "dev", "prod", "u", &steps)
            .await;
        store
            .create_run("p2", "wallet", "2.0", "dev", "prod", "u", &steps)
            .await;
        store
            .create_run("p3", "trader", "1.1", "dev", "prod", "u", &steps)
            .await;

        let all = store.list_runs(None, 100).await;
        assert_eq!(all.len(), 3);

        let trader_runs = store.list_runs(Some("trader"), 100).await;
        assert_eq!(trader_runs.len(), 2);

        let limited = store.list_runs(None, 2).await;
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn test_definition_upsert() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());

        let def = PipelineDefinition {
            id: "pipe-trader".into(),
            app_slug: "trader".into(),
            steps: sample_steps(),
        };
        store.save_definition(def).await;

        let fetched = store.get_definition("trader").await.unwrap();
        assert_eq!(fetched.steps.len(), 3);

        // Upsert with different steps
        let def2 = PipelineDefinition {
            id: "pipe-trader-v2".into(),
            app_slug: "trader".into(),
            steps: vec![sample_steps()[0].clone()],
        };
        store.save_definition(def2).await;

        let fetched2 = store.get_definition("trader").await.unwrap();
        assert_eq!(fetched2.steps.len(), 1);
        assert_eq!(fetched2.id, "pipe-trader-v2");

        let defs = store.list_definitions().await;
        assert_eq!(defs.len(), 1);
    }

    #[tokio::test]
    async fn test_config_crud() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());

        // No config initially
        assert!(store.get_config("trader").await.is_none());
        assert!(store.list_configs().await.is_empty());

        // Save a config
        let config = PipelineConfig {
            app_slug: "trader".into(),
            env_chain: vec!["dev".into(), "acc".into(), "prod".into()],
            skip_steps: std::collections::HashSet::from(["test".into()]),
            auto_promote: std::collections::HashSet::from(["dev".into()]),
            gates: vec![GateDef {
                from_env: "acc".into(),
                to_env: "prod".into(),
            }],
        };
        store.save_config(config).await;

        let fetched = store.get_config("trader").await.unwrap();
        assert_eq!(fetched.env_chain.len(), 3);
        assert!(fetched.skip_steps.contains("test"));

        // Upsert: change the chain
        let config2 = PipelineConfig {
            app_slug: "trader".into(),
            env_chain: vec!["dev".into(), "prod".into()],
            skip_steps: std::collections::HashSet::new(),
            auto_promote: std::collections::HashSet::new(),
            gates: vec![],
        };
        store.save_config(config2).await;

        let fetched2 = store.get_config("trader").await.unwrap();
        assert_eq!(fetched2.env_chain.len(), 2);
        assert!(fetched2.skip_steps.is_empty());

        // Still only 1 config
        assert_eq!(store.list_configs().await.len(), 1);

        // Add another app config
        let config3 = PipelineConfig {
            app_slug: "wallet".into(),
            env_chain: vec!["dev".into()],
            skip_steps: std::collections::HashSet::new(),
            auto_promote: std::collections::HashSet::new(),
            gates: vec![],
        };
        store.save_config(config3).await;
        assert_eq!(store.list_configs().await.len(), 2);
    }

    #[tokio::test]
    async fn test_gate_lifecycle() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());

        // Create a pending gate
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
        store.create_gate(gate).await;

        // List pending
        let pending = store.list_pending_gates(None).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "gate-001");

        // Get by ID
        let fetched = store.get_gate("gate-001").await.unwrap();
        assert_eq!(fetched.status, GateStatus::Pending);

        // Approve
        let approved = store.approve_gate("gate-001", "romain").await.unwrap();
        assert_eq!(approved.status, GateStatus::Approved);
        assert!(approved.resolved_at.is_some());
        assert_eq!(approved.resolved_by.as_deref(), Some("romain"));

        // No longer pending
        assert!(store.list_pending_gates(None).await.is_empty());
    }

    #[tokio::test]
    async fn test_gate_reject() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());

        let gate = GateApproval {
            id: "gate-002".into(),
            chain_id: "chain-002".into(),
            app_slug: "wallet".into(),
            from_env: "dev".into(),
            to_env: "prod".into(),
            version: "2.0".into(),
            status: GateStatus::Pending,
            created_at: Utc::now(),
            resolved_at: None,
            resolved_by: None,
        };
        store.create_gate(gate).await;

        let rejected = store.reject_gate("gate-002", "admin").await.unwrap();
        assert_eq!(rejected.status, GateStatus::Rejected);
        assert_eq!(rejected.resolved_by.as_deref(), Some("admin"));
    }

    #[tokio::test]
    async fn test_pending_gates_filtered() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PipelineStore::new(tmp.path().to_path_buf());

        for (id, app) in [("g1", "trader"), ("g2", "wallet"), ("g3", "trader")] {
            store
                .create_gate(GateApproval {
                    id: id.into(),
                    chain_id: "c1".into(),
                    app_slug: app.into(),
                    from_env: "dev".into(),
                    to_env: "prod".into(),
                    version: "1.0".into(),
                    status: GateStatus::Pending,
                    created_at: Utc::now(),
                    resolved_at: None,
                    resolved_by: None,
                })
                .await;
        }

        assert_eq!(store.list_pending_gates(None).await.len(), 3);
        assert_eq!(store.list_pending_gates(Some("trader")).await.len(), 2);
        assert_eq!(store.list_pending_gates(Some("wallet")).await.len(), 1);
    }

    #[tokio::test]
    async fn test_persistence_reload() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        {
            let store = PipelineStore::new(path.clone());
            let steps = sample_steps();
            store
                .create_run("p1", "trader", "1.0", "dev", "prod", "u", &steps)
                .await;
            store
                .save_definition(PipelineDefinition {
                    id: "pipe-1".into(),
                    app_slug: "trader".into(),
                    steps,
                })
                .await;
        }

        // Reload from disk
        let store2 = PipelineStore::new(path);
        let runs = store2.list_runs(None, 100).await;
        assert_eq!(runs.len(), 1);
        let defs = store2.list_definitions().await;
        assert_eq!(defs.len(), 1);
    }
}
