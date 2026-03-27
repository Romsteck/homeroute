#![allow(dead_code)]

use anyhow::Result;
use hr_environment::EnvAgentConfig;

/// Load and validate the env-agent configuration.
pub fn load(path: &str) -> Result<EnvAgentConfig> {
    EnvAgentConfig::load(path)
}
