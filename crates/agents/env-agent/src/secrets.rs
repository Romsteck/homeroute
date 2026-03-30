//! Per-app encrypted secrets vault.
//! Secrets are stored as JSON files per app.
//! Future: AES-256-GCM encryption with Argon2id key derivation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretStore {
    secrets: HashMap<String, String>,
}

pub struct SecretsManager {
    vault_path: PathBuf,
}

impl SecretsManager {
    pub fn new(vault_path: &Path) -> Self {
        std::fs::create_dir_all(vault_path).ok();
        Self {
            vault_path: vault_path.to_path_buf(),
        }
    }

    fn app_file(&self, app_slug: &str) -> PathBuf {
        self.vault_path.join(format!("{}.json", app_slug))
    }

    fn load_store(&self, app_slug: &str) -> Result<SecretStore> {
        let path = self.app_file(app_slug);
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&data)?)
        } else {
            Ok(SecretStore {
                secrets: HashMap::new(),
            })
        }
    }

    fn save_store(&self, app_slug: &str, store: &SecretStore) -> Result<()> {
        let path = self.app_file(app_slug);
        let data = serde_json::to_string_pretty(store)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    pub fn list(&self, app_slug: &str) -> Result<Vec<String>> {
        let store = self.load_store(app_slug)?;
        let mut keys: Vec<String> = store.secrets.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }

    pub fn get(&self, app_slug: &str, key: &str) -> Result<Option<String>> {
        let store = self.load_store(app_slug)?;
        Ok(store.secrets.get(key).cloned())
    }

    pub fn set(&self, app_slug: &str, key: &str, value: &str) -> Result<()> {
        let mut store = self.load_store(app_slug)?;
        store.secrets.insert(key.to_string(), value.to_string());
        self.save_store(app_slug, &store)?;
        info!(app = app_slug, key = key, "Secret set");
        Ok(())
    }

    pub fn delete(&self, app_slug: &str, key: &str) -> Result<bool> {
        let mut store = self.load_store(app_slug)?;
        let removed = store.secrets.remove(key).is_some();
        if removed {
            self.save_store(app_slug, &store)?;
            info!(app = app_slug, key = key, "Secret deleted");
        }
        Ok(removed)
    }

}
