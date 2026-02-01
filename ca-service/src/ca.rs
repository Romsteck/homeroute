use crate::storage::CaStorage;
use crate::types::{CaConfig, CaError, CaResult, CertificateInfo};
use chrono::{Duration, Utc};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    IsCa, KeyPair, KeyUsagePurpose,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Autorité de certification pour générer et gérer des certificats auto-signés
pub struct CertificateAuthority {
    config: CaConfig,
    storage: CaStorage,
    root_cert: Arc<RwLock<Option<Certificate>>>,
    root_key_pair: Arc<RwLock<Option<KeyPair>>>,
}

impl CertificateAuthority {
    /// Crée une nouvelle instance de CA
    pub fn new(config: CaConfig) -> Self {
        let storage = CaStorage::new(&config.storage_path);
        Self {
            config,
            storage,
            root_cert: Arc::new(RwLock::new(None)),
            root_key_pair: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialise la CA en générant le certificat root si nécessaire
    pub async fn init(&self) -> CaResult<()> {
        self.storage.init()?;

        if self.storage.is_initialized() {
            // Charger le certificat root existant
            self.load_root_certificate().await?;
        } else {
            // Générer un nouveau certificat root
            self.generate_root_certificate().await?;
        }

        Ok(())
    }

    /// Vérifie si la CA est initialisée
    pub fn is_initialized(&self) -> bool {
        self.storage.is_initialized()
    }

    /// Génère un nouveau certificat root auto-signé
    async fn generate_root_certificate(&self) -> CaResult<()> {
        let mut params = CertificateParams::new(vec![]).map_err(|e| {
            CaError::GenerationFailed(format!("Failed to create params: {}", e))
        })?;

        // Distinguished Name
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &self.config.common_name);
        dn.push(DnType::OrganizationName, &self.config.organization);
        params.distinguished_name = dn;

        // Root CA parameters
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];

        // Validity period (rcgen uses time crate, not chrono)
        let now = time::OffsetDateTime::now_utc();
        let validity_duration = time::Duration::days(self.config.root_validity_days as i64);
        params.not_before = now;
        params.not_after = now + validity_duration;

        // Generate key pair
        let key_pair = KeyPair::generate().map_err(|e| {
            CaError::GenerationFailed(format!("Failed to generate key pair: {}", e))
        })?;

        // Generate self-signed certificate
        let cert = params.self_signed(&key_pair).map_err(|e| {
            CaError::GenerationFailed(format!("Failed to generate root certificate: {}", e))
        })?;

        // Save to disk
        self.storage.write_file(
            self.storage.root_cert_path(),
            &cert.pem(),
        )?;
        self.storage.write_file(
            self.storage.root_key_path(),
            &key_pair.serialize_pem(),
        )?;

        // Store in memory
        *self.root_cert.write().await = Some(cert);
        *self.root_key_pair.write().await = Some(key_pair);

        Ok(())
    }

    /// Charge le certificat root depuis le disque
    async fn load_root_certificate(&self) -> CaResult<()> {
        let _cert_pem = self.storage.read_file(self.storage.root_cert_path())?;
        let key_pem = self.storage.read_file(self.storage.root_key_path())?;

        let key_pair = KeyPair::from_pem(&key_pem).map_err(|e| {
            CaError::ParsingError(format!("Failed to parse root key: {}", e))
        })?;

        // Pour rcgen 0.13, on stocke uniquement la clé car on utilise Certificate::from_params
        // qui nécessite de recréer les params. Pour l'instant, on stocke juste le PEM.
        // L'alternative serait de régénérer le certificat avec les mêmes params.
        // Pour simplifier, on va juste stocker le key_pair et regénérer le cert au besoin.

        // Recréer le certificat avec les mêmes paramètres
        let mut params = CertificateParams::new(vec![]).map_err(|e| {
            CaError::ParsingError(format!("Failed to create params: {}", e))
        })?;

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &self.config.common_name);
        dn.push(DnType::OrganizationName, &self.config.organization);
        params.distinguished_name = dn;

        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];

        let cert = params.self_signed(&key_pair).map_err(|e| {
            CaError::ParsingError(format!("Failed to reconstruct root certificate: {}", e))
        })?;

        *self.root_cert.write().await = Some(cert);
        *self.root_key_pair.write().await = Some(key_pair);

        Ok(())
    }

    /// Récupère le certificat root au format PEM
    pub async fn get_root_cert_pem(&self) -> CaResult<String> {
        let cert = self.root_cert.read().await;
        cert.as_ref()
            .map(|c| c.pem())
            .ok_or(CaError::NotInitialized)
    }

    /// Récupère le certificat root au format DER
    pub async fn get_root_cert_der(&self) -> CaResult<Vec<u8>> {
        let cert = self.root_cert.read().await;
        cert.as_ref()
            .map(|c| c.der().to_vec())
            .ok_or(CaError::NotInitialized)
    }

    /// Émet un nouveau certificat serveur pour les domaines spécifiés
    pub async fn issue_certificate(&self, domains: Vec<String>) -> CaResult<CertificateInfo> {
        if domains.is_empty() {
            return Err(CaError::InvalidDomain("No domains provided".to_string()));
        }

        // Validate domains
        for domain in &domains {
            if domain.is_empty() || !is_valid_domain(domain) {
                return Err(CaError::InvalidDomain(domain.clone()));
            }
        }

        // Generate unique ID
        let id = uuid::Uuid::new_v4().to_string();

        // Certificate parameters
        let mut params = CertificateParams::new(domains.clone()).map_err(|e| {
            CaError::GenerationFailed(format!("Failed to create params: {}", e))
        })?;

        // Common Name (first domain)
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &domains[0]);
        dn.push(DnType::OrganizationName, &self.config.organization);
        params.distinguished_name = dn;

        // Server certificate parameters
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
        ];

        // Validity period (rcgen uses time crate)
        let not_before_time = time::OffsetDateTime::now_utc();
        let validity_duration = time::Duration::days(self.config.cert_validity_days as i64);
        let not_after_time = not_before_time + validity_duration;
        params.not_before = not_before_time;
        params.not_after = not_after_time;

        // For CertificateInfo, convert to chrono
        let not_before = Utc::now();
        let not_after = not_before + Duration::days(self.config.cert_validity_days as i64);

        // Generate key pair
        let key_pair = KeyPair::generate().map_err(|e| {
            CaError::GenerationFailed(format!("Failed to generate key pair: {}", e))
        })?;

        // Sign with root CA
        let root_cert = self.root_cert.read().await;
        let root_key = self.root_key_pair.read().await;

        let (cert, key) = match (root_cert.as_ref(), root_key.as_ref()) {
            (Some(ca_cert), Some(ca_key)) => {
                let cert = params.signed_by(&key_pair, ca_cert, ca_key).map_err(|e| {
                    CaError::GenerationFailed(format!("Failed to sign certificate: {}", e))
                })?;
                (cert, key_pair)
            }
            _ => return Err(CaError::NotInitialized),
        };

        // Save to disk
        let cert_path = self.storage.cert_path(&id);
        let key_path = self.storage.key_path(&id);

        self.storage.write_file(&cert_path, &cert.pem())?;
        self.storage.write_file(&key_path, &key.serialize_pem())?;

        // Create certificate info
        let cert_info = CertificateInfo {
            id: id.clone(),
            domains: domains.clone(),
            issued_at: not_before,
            expires_at: not_after,
            serial_number: hex::encode(cert.key_identifier()),
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
        };

        // Update index
        let mut index = self.storage.load_index()?;
        index.push(cert_info.clone());
        self.storage.save_index(&index)?;

        Ok(cert_info)
    }

    /// Liste tous les certificats émis
    pub fn list_certificates(&self) -> CaResult<Vec<CertificateInfo>> {
        self.storage.load_index()
    }

    /// Récupère un certificat par son ID
    pub fn get_certificate(&self, id: &str) -> CaResult<CertificateInfo> {
        let index = self.storage.load_index()?;
        index
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CaError::CertificateNotFound(id.to_string()))
    }

    /// Renouvelle un certificat existant
    pub async fn renew_certificate(&self, id: &str) -> CaResult<CertificateInfo> {
        let cert_info = self.get_certificate(id)?;

        // Supprimer l'ancien certificat de l'index
        let mut index = self.storage.load_index()?;
        index.retain(|c| c.id != id);
        self.storage.save_index(&index)?;

        // Supprimer les fichiers
        self.storage.delete_certificate(id)?;

        // Émettre un nouveau certificat avec les mêmes domaines
        self.issue_certificate(cert_info.domains).await
    }

    /// Révoque un certificat (supprime de l'index et du disque)
    pub fn revoke_certificate(&self, id: &str) -> CaResult<()> {
        // Supprimer de l'index
        let mut index = self.storage.load_index()?;
        let found = index.iter().any(|c| c.id == id);

        if !found {
            return Err(CaError::CertificateNotFound(id.to_string()));
        }

        index.retain(|c| c.id != id);
        self.storage.save_index(&index)?;

        // Supprimer les fichiers
        self.storage.delete_certificate(id)?;

        Ok(())
    }

    /// Identifie les certificats nécessitant un renouvellement
    pub fn certificates_needing_renewal(&self) -> CaResult<Vec<CertificateInfo>> {
        let index = self.storage.load_index()?;
        Ok(index
            .into_iter()
            .filter(|c| c.needs_renewal(self.config.renewal_threshold_days))
            .collect())
    }
}

/// Valide un nom de domaine basique (pas de validation IDNA complète)
fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }

    // Accepter les wildcards
    if domain.starts_with("*.") {
        let rest = &domain[2..];
        if rest.is_empty() {
            return false;
        }
        return is_valid_domain_part(rest);
    }

    is_valid_domain_part(domain)
}

fn is_valid_domain_part(domain: &str) -> bool {
    domain
        .split('.')
        .all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_domain() {
        assert!(is_valid_domain("example.com"));
        assert!(is_valid_domain("sub.example.com"));
        assert!(is_valid_domain("*.example.com"));
        assert!(is_valid_domain("test-123.example.com"));

        assert!(!is_valid_domain(""));
        assert!(!is_valid_domain("-example.com"));
        assert!(!is_valid_domain("example-.com"));
        assert!(!is_valid_domain("*."));
        assert!(!is_valid_domain("example..com"));
    }
}
