use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    base_port: u16,
    assignments: HashMap<String, u16>,
}

#[derive(Debug)]
pub struct PortRegistry {
    base_port: u16,
    registry_path: PathBuf,
    assignments: HashMap<String, u16>,
}

impl PortRegistry {
    pub fn new(base_port: u16, registry_path: PathBuf) -> Self {
        Self {
            base_port,
            registry_path,
            assignments: HashMap::new(),
        }
    }

    /// Load assignments from JSON file. Ignores if file not found.
    pub fn load(&mut self) -> Result<()> {
        let data = match std::fs::read_to_string(&self.registry_path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(path = %self.registry_path.display(), "port registry file not found, starting fresh");
                return Ok(());
            }
            Err(e) => return Err(e).context("reading port registry"),
        };

        let file: RegistryFile =
            serde_json::from_str(&data).context("parsing port registry JSON")?;

        info!(
            path = %self.registry_path.display(),
            count = file.assignments.len(),
            "loaded port registry"
        );

        self.base_port = file.base_port;
        self.assignments = file.assignments;
        Ok(())
    }

    /// Assign ports to all apps. Rules:
    /// 1. Slug already in registry -> keep existing port
    /// 2. configured_port != 3000 (explicit override) -> use that
    /// 3. Otherwise -> assign next available port from base_port
    /// Returns error if any two apps end up on the same port.
    pub fn assign_all(&mut self, apps: &[(String, u16)]) -> Result<()> {
        // Collect all ports that are already taken (existing + explicit overrides)
        let mut used_ports: HashMap<u16, String> = HashMap::new();

        // Phase 1: lock in existing registry entries and explicit overrides
        for (slug, configured_port) in apps {
            if let Some(&existing) = self.assignments.get(slug) {
                used_ports.insert(existing, slug.clone());
            } else if *configured_port != 3000 {
                self.assignments.insert(slug.clone(), *configured_port);
                used_ports.insert(*configured_port, slug.clone());
            }
        }

        // Phase 2: assign sequential ports to remaining apps
        let mut next_port = self.base_port;
        for (slug, _) in apps {
            if self.assignments.contains_key(slug) {
                continue;
            }
            // Find next available port
            while used_ports.contains_key(&next_port) {
                next_port = next_port.checked_add(1).context("port overflow")?;
            }
            self.assignments.insert(slug.clone(), next_port);
            used_ports.insert(next_port, slug.clone());
            next_port = next_port.checked_add(1).context("port overflow")?;
        }

        // Phase 3: check for conflicts (two slugs with the same port)
        let mut port_to_slug: HashMap<u16, &str> = HashMap::new();
        for (slug, &port) in &self.assignments {
            if let Some(existing) = port_to_slug.insert(port, slug) {
                bail!(
                    "port conflict: both '{}' and '{}' assigned to port {}",
                    existing,
                    slug,
                    port
                );
            }
        }

        Ok(())
    }

    /// Assign a single port to a new app slug.
    /// Uses the same logic as assign_all phase 2: find the next available port from base_port.
    pub fn assign_one(&mut self, slug: &str) -> Result<u16> {
        if let Some(&existing) = self.assignments.get(slug) {
            return Ok(existing);
        }

        let used_ports: std::collections::HashSet<u16> = self.assignments.values().copied().collect();
        let mut next_port = self.base_port;
        loop {
            if !used_ports.contains(&next_port) {
                self.assignments.insert(slug.to_string(), next_port);
                return Ok(next_port);
            }
            next_port = next_port.checked_add(1).context("port overflow")?;
        }
    }

    /// Get the assigned port for a slug.
    pub fn port_for(&self, slug: &str) -> Option<u16> {
        self.assignments.get(slug).copied()
    }

    /// Write JSON atomically (write to tmp file then rename).
    pub fn save(&self) -> Result<()> {
        let file = RegistryFile {
            base_port: self.base_port,
            assignments: self.assignments.clone(),
        };

        let json = serde_json::to_string_pretty(&file).context("serializing port registry")?;

        let tmp_path = self.registry_path.with_extension("tmp");
        std::fs::write(&tmp_path, &json).context("writing tmp port registry")?;
        std::fs::rename(&tmp_path, &self.registry_path).context("renaming port registry")?;

        info!(
            path = %self.registry_path.display(),
            count = self.assignments.len(),
            "saved port registry"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn registry_in(dir: &TempDir) -> PortRegistry {
        PortRegistry::new(3001, dir.path().join("ports.json"))
    }

    #[test]
    fn test_basic_assignment() {
        let dir = TempDir::new().unwrap();
        let mut reg = registry_in(&dir);

        let apps = vec![
            ("alpha".into(), 3000u16),
            ("beta".into(), 3000),
            ("gamma".into(), 3000),
        ];
        reg.assign_all(&apps).unwrap();

        assert_eq!(reg.port_for("alpha"), Some(3001));
        assert_eq!(reg.port_for("beta"), Some(3002));
        assert_eq!(reg.port_for("gamma"), Some(3003));
    }

    #[test]
    fn test_explicit_override() {
        let dir = TempDir::new().unwrap();
        let mut reg = registry_in(&dir);

        let apps = vec![
            ("alpha".into(), 3000u16),
            ("beta".into(), 3010),
            ("gamma".into(), 3000),
        ];
        reg.assign_all(&apps).unwrap();

        assert_eq!(reg.port_for("alpha"), Some(3001));
        assert_eq!(reg.port_for("beta"), Some(3010));
        assert_eq!(reg.port_for("gamma"), Some(3002));
    }

    #[test]
    fn test_stability_across_save_load() {
        let dir = TempDir::new().unwrap();
        let mut reg = registry_in(&dir);

        let apps = vec![
            ("trader".into(), 3000u16),
            ("wallet".into(), 3000),
            ("home".into(), 3000),
        ];
        reg.assign_all(&apps).unwrap();
        reg.save().unwrap();

        // Reload into a fresh registry
        let mut reg2 = registry_in(&dir);
        reg2.load().unwrap();
        reg2.assign_all(&apps).unwrap();

        assert_eq!(reg2.port_for("trader"), reg.port_for("trader"));
        assert_eq!(reg2.port_for("wallet"), reg.port_for("wallet"));
        assert_eq!(reg2.port_for("home"), reg.port_for("home"));
    }

    #[test]
    fn test_conflict_detection() {
        let dir = TempDir::new().unwrap();
        let mut reg = registry_in(&dir);

        let apps = vec![
            ("alpha".into(), 3010u16),
            ("beta".into(), 3010),
        ];
        let result = reg.assign_all(&apps);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("port conflict"), "unexpected error: {msg}");
    }
}
