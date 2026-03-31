use std::collections::HashSet;

use hr_environment::AppStackType;

use crate::types::{PipelineConfig, PipelineStepDef, PipelineStepType};

#[cfg(test)]
use crate::types::GateDef;

/// Build timeout is longer than other steps (300s vs 120s).
const BUILD_TIMEOUT: u64 = 300;
const DEFAULT_TIMEOUT: u64 = 120;

/// Resolve pipeline steps for a given stack type and config.
///
/// Steps are always: Build → (Test) → (BackupDb) → (MigrateDb) → Deploy → HealthCheck.
/// Steps in parentheses are optional: skipped if in `config.skip_steps` or if `has_db` is false.
pub fn steps_for_stack(
    _stack: AppStackType,
    config: &PipelineConfig,
    has_db: bool,
) -> Vec<PipelineStepDef> {
    let mut steps = Vec::new();

    // Build is always included
    steps.push(PipelineStepDef {
        name: "build".into(),
        step_type: PipelineStepType::Build,
        timeout_secs: BUILD_TIMEOUT,
        config: serde_json::Value::Null,
    });

    // Test (optional, skippable)
    if !config.skip_steps.contains("test") {
        steps.push(PipelineStepDef {
            name: "test".into(),
            step_type: PipelineStepType::Test,
            timeout_secs: DEFAULT_TIMEOUT,
            config: serde_json::Value::Null,
        });
    }

    // DB steps only if the app has a database
    if has_db {
        if !config.skip_steps.contains("backup-db") {
            steps.push(PipelineStepDef {
                name: "backup-db".into(),
                step_type: PipelineStepType::BackupDb,
                timeout_secs: DEFAULT_TIMEOUT,
                config: serde_json::Value::Null,
            });
        }
        if !config.skip_steps.contains("migrate-db") {
            steps.push(PipelineStepDef {
                name: "migrate-db".into(),
                step_type: PipelineStepType::MigrateDb,
                timeout_secs: DEFAULT_TIMEOUT,
                config: serde_json::Value::Null,
            });
        }
    }

    // Deploy is always included
    steps.push(PipelineStepDef {
        name: "deploy".into(),
        step_type: PipelineStepType::Deploy,
        timeout_secs: DEFAULT_TIMEOUT,
        config: serde_json::Value::Null,
    });

    // HealthCheck is always included
    steps.push(PipelineStepDef {
        name: "health-check".into(),
        step_type: PipelineStepType::HealthCheck,
        timeout_secs: DEFAULT_TIMEOUT,
        config: serde_json::Value::Null,
    });

    steps
}

/// Create a default pipeline config for an app: single env "dev", no gates, no auto-promote.
pub fn default_config(app_slug: &str) -> PipelineConfig {
    PipelineConfig {
        app_slug: app_slug.to_string(),
        env_chain: vec!["dev".into()],
        skip_steps: HashSet::new(),
        auto_promote: HashSet::new(),
        gates: Vec::new(),
    }
}

/// Validate that a transition from `source` to `target` is allowed by the config's env chain.
///
/// The source and target must be consecutive entries in `env_chain`.
pub fn validate_transition(
    config: &PipelineConfig,
    source: &str,
    target: &str,
) -> anyhow::Result<()> {
    let chain = &config.env_chain;
    let source_idx = chain
        .iter()
        .position(|e| e == source)
        .ok_or_else(|| anyhow::anyhow!("environment '{}' not found in chain {:?}", source, chain))?;
    let target_idx = chain
        .iter()
        .position(|e| e == target)
        .ok_or_else(|| anyhow::anyhow!("environment '{}' not found in chain {:?}", target, chain))?;

    if target_idx != source_idx + 1 {
        let chain_str = chain.join("→");
        anyhow::bail!(
            "transition {}→{} not allowed, chain is {}",
            source,
            target,
            chain_str,
        );
    }

    Ok(())
}

/// Return the next environment in the chain after `current`, if any.
pub fn next_env_in_chain(config: &PipelineConfig, current: &str) -> Option<String> {
    let chain = &config.env_chain;
    let idx = chain.iter().position(|e| e == current)?;
    chain.get(idx + 1).cloned()
}

/// Check if there is a gate defined between `from` and `to` environments.
pub fn has_gate(config: &PipelineConfig, from: &str, to: &str) -> bool {
    config
        .gates
        .iter()
        .any(|g| g.from_env == from && g.to_env == to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_env_config() -> PipelineConfig {
        PipelineConfig {
            app_slug: "trader".into(),
            env_chain: vec!["dev".into(), "acc".into(), "prod".into()],
            skip_steps: HashSet::new(),
            auto_promote: HashSet::from(["dev".into()]),
            gates: vec![GateDef {
                from_env: "acc".into(),
                to_env: "prod".into(),
            }],
        }
    }

    // ── steps_for_stack tests ──

    #[test]
    fn test_steps_all_included_with_db() {
        let config = default_config("myapp");
        let steps = steps_for_stack(AppStackType::AxumVite, &config, true);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["build", "test", "backup-db", "migrate-db", "deploy", "health-check"]
        );
    }

    #[test]
    fn test_steps_no_db() {
        let config = default_config("myapp");
        let steps = steps_for_stack(AppStackType::NextJs, &config, false);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "test", "deploy", "health-check"]);
    }

    #[test]
    fn test_steps_skip_test() {
        let mut config = default_config("myapp");
        config.skip_steps.insert("test".into());
        let steps = steps_for_stack(AppStackType::Axum, &config, false);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "deploy", "health-check"]);
    }

    #[test]
    fn test_steps_skip_db_steps() {
        let mut config = default_config("myapp");
        config.skip_steps.insert("backup-db".into());
        let steps = steps_for_stack(AppStackType::AxumVite, &config, true);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "test", "migrate-db", "deploy", "health-check"]);
    }

    #[test]
    fn test_build_step_has_longer_timeout() {
        let config = default_config("myapp");
        let steps = steps_for_stack(AppStackType::NextJs, &config, false);
        let build = &steps[0];
        assert_eq!(build.step_type, PipelineStepType::Build);
        assert_eq!(build.timeout_secs, 300);
        // Other steps have 120s
        for step in &steps[1..] {
            assert_eq!(step.timeout_secs, 120);
        }
    }

    // ── default_config tests ──

    #[test]
    fn test_default_config() {
        let config = default_config("wallet");
        assert_eq!(config.app_slug, "wallet");
        assert_eq!(config.env_chain, vec!["dev"]);
        assert!(config.skip_steps.is_empty());
        assert!(config.auto_promote.is_empty());
        assert!(config.gates.is_empty());
    }

    // ── validate_transition tests ──

    #[test]
    fn test_valid_transition_dev_to_acc() {
        let config = three_env_config();
        assert!(validate_transition(&config, "dev", "acc").is_ok());
    }

    #[test]
    fn test_valid_transition_acc_to_prod() {
        let config = three_env_config();
        assert!(validate_transition(&config, "acc", "prod").is_ok());
    }

    #[test]
    fn test_invalid_transition_skip() {
        let config = three_env_config();
        let err = validate_transition(&config, "dev", "prod").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("dev→prod not allowed"), "got: {msg}");
        assert!(msg.contains("dev→acc→prod"), "got: {msg}");
    }

    #[test]
    fn test_invalid_transition_reverse() {
        let config = three_env_config();
        let err = validate_transition(&config, "prod", "dev").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_invalid_transition_unknown_env() {
        let config = three_env_config();
        let err = validate_transition(&config, "dev", "staging").unwrap_err();
        assert!(err.to_string().contains("not found in chain"));
    }

    // ── next_env_in_chain tests ──

    #[test]
    fn test_next_env() {
        let config = three_env_config();
        assert_eq!(next_env_in_chain(&config, "dev"), Some("acc".into()));
        assert_eq!(next_env_in_chain(&config, "acc"), Some("prod".into()));
        assert_eq!(next_env_in_chain(&config, "prod"), None);
        assert_eq!(next_env_in_chain(&config, "unknown"), None);
    }

    // ── has_gate tests ──

    #[test]
    fn test_has_gate() {
        let config = three_env_config();
        assert!(!has_gate(&config, "dev", "acc"));
        assert!(has_gate(&config, "acc", "prod"));
        assert!(!has_gate(&config, "prod", "dev"));
    }

    #[test]
    fn test_has_gate_empty() {
        let config = default_config("myapp");
        assert!(!has_gate(&config, "dev", "prod"));
    }
}
