use crate::types::{AcmeResult, CertificateInfo, WildcardType};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// ACME certificate storage management
pub struct AcmeStorage {
    base_path: PathBuf,
}

impl AcmeStorage {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Initialize storage directories
    pub fn init(&self) -> AcmeResult<()> {
        fs::create_dir_all(&self.base_path)?;
        fs::create_dir_all(self.base_path.join("certs"))?;
        fs::create_dir_all(self.base_path.join("keys"))?;
        Ok(())
    }

    /// Path to account credentials file
    pub fn account_path(&self) -> PathBuf {
        self.base_path.join("account.json")
    }

    /// Path to certificate file by cert ID string
    pub fn cert_path_by_id(&self, cert_id: &str) -> PathBuf {
        self.base_path
            .join("certs")
            .join(format!("{}.crt", cert_id))
    }

    /// Path to private key file by cert ID string
    pub fn key_path_by_id(&self, cert_id: &str) -> PathBuf {
        self.base_path.join("keys").join(format!("{}.key", cert_id))
    }

    /// Path to full chain certificate by cert ID string
    pub fn chain_path_by_id(&self, cert_id: &str) -> PathBuf {
        self.base_path
            .join("certs")
            .join(format!("{}-chain.crt", cert_id))
    }

    /// Path to certificate file for a wildcard type
    pub fn cert_path(&self, wildcard_type: &WildcardType) -> PathBuf {
        self.cert_path_by_id(&wildcard_type.id())
    }

    /// Path to private key file for a wildcard type
    pub fn key_path(&self, wildcard_type: &WildcardType) -> PathBuf {
        self.key_path_by_id(&wildcard_type.id())
    }

    /// Path to full chain certificate
    pub fn chain_path(&self, wildcard_type: &WildcardType) -> PathBuf {
        self.chain_path_by_id(&wildcard_type.id())
    }

    /// Path to certificate index file
    pub fn index_path(&self) -> PathBuf {
        self.base_path.join("index.json")
    }

    /// Check if ACME account is initialized
    pub fn is_initialized(&self) -> bool {
        self.account_path().exists()
    }

    /// Load certificate index
    pub fn load_index(&self) -> AcmeResult<Vec<CertificateInfo>> {
        let index_path = self.index_path();
        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(index_path)?;
        let index: Vec<CertificateInfo> = serde_json::from_str(&content)?;
        Ok(index)
    }

    /// Save certificate index atomically
    pub fn save_index(&self, index: &[CertificateInfo]) -> AcmeResult<()> {
        let content = serde_json::to_string_pretty(index)?;
        let index_path = self.index_path();
        let temp_path = index_path.with_extension("json.tmp");

        // Write to temporary file first
        fs::write(&temp_path, &content)?;

        // Atomic rename
        fs::rename(&temp_path, &index_path)?;

        Ok(())
    }

    /// Write a file
    pub fn write_file<P: AsRef<Path>>(&self, path: P, content: &str) -> AcmeResult<()> {
        fs::write(path.as_ref(), content)?;
        Ok(())
    }

    /// Read a file
    pub fn read_file<P: AsRef<Path>>(&self, path: P) -> AcmeResult<String> {
        let content = fs::read_to_string(path.as_ref())?;
        Ok(content)
    }

    /// Check if certificate files exist
    pub fn cert_exists(&self, wildcard_type: &WildcardType) -> bool {
        self.cert_path(wildcard_type).exists() && self.key_path(wildcard_type).exists()
    }

    /// Remove legacy per-environment wildcard certificates from disk and the index.
    ///
    /// Targets the old `wildcard-env-*` IDs left over from the dismantled environments
    /// system. Files in `certs/` and `keys/` matching that prefix are deleted, and any
    /// matching entries are stripped from `index.json`. Returns the list of cert IDs
    /// that were removed.
    ///
    /// This method is intentionally not invoked at startup. It is exposed for an admin
    /// endpoint or one-shot script to call when cleanup is desired.
    pub async fn cleanup_legacy_env_certs(&self) -> AcmeResult<Vec<String>> {
        let mut removed: Vec<String> = Vec::new();

        // Walk certs/ and keys/ directories looking for legacy IDs.
        for sub in ["certs", "keys"] {
            let dir = self.base_path.join(sub);
            if !dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    warn!(dir = %dir.display(), error = %e, "Failed to read ACME storage dir");
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.starts_with("wildcard-env-") {
                    continue;
                }
                match fs::remove_file(&path) {
                    Ok(()) => {
                        info!(file = %path.display(), "Removed legacy env wildcard file");
                        // Track unique cert IDs (stem before first dot)
                        let id = name.split('.').next().unwrap_or(name).to_string();
                        let id = id.trim_end_matches("-chain").to_string();
                        if !removed.contains(&id) {
                            removed.push(id);
                        }
                    }
                    Err(e) => {
                        warn!(file = %path.display(), error = %e, "Failed to remove legacy env wildcard file");
                    }
                }
            }
        }

        // Strip matching entries from the index.
        let index = self.load_index().unwrap_or_default();
        let before = index.len();
        let filtered: Vec<CertificateInfo> = index
            .into_iter()
            .filter(|c| !c.id.starts_with("wildcard-env-"))
            .collect();
        if filtered.len() != before {
            self.save_index(&filtered)?;
        }

        info!(
            certs_removed = removed.len(),
            "Cleaned up legacy env-specific wildcard certs"
        );

        Ok(removed)
    }
}
