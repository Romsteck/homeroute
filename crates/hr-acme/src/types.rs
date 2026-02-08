use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Configuration for ACME certificate management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcmeConfig {
    /// Storage path for ACME data
    pub storage_path: String,
    /// Cloudflare API token for DNS-01 challenges
    pub cf_api_token: String,
    /// Cloudflare Zone ID
    pub cf_zone_id: String,
    /// Base domain (e.g., "mynetwk.biz")
    pub base_domain: String,
    /// Let's Encrypt directory URL (production or staging)
    pub directory_url: String,
    /// Account email for Let's Encrypt
    pub account_email: String,
    /// Days before expiry to trigger renewal
    pub renewal_threshold_days: u32,
}

impl Default for AcmeConfig {
    fn default() -> Self {
        Self {
            storage_path: "/var/lib/server-dashboard/acme".to_string(),
            cf_api_token: String::new(),
            cf_zone_id: String::new(),
            base_domain: String::new(),
            directory_url: "https://acme-v02.api.letsencrypt.org/directory".to_string(),
            account_email: String::new(),
            renewal_threshold_days: 30,
        }
    }
}

/// Type of wildcard certificate
///
/// Custom serde implementation for backward compatibility:
/// - `"main"` or `"global"` deserializes to `Global`
/// - `"code"` deserializes to `LegacyCode`
/// - `{"app": "slug_value"}` deserializes to `App { slug: "slug_value" }`
///
/// Serialization:
/// - `Global` -> `"global"`
/// - `LegacyCode` -> `"code"`
/// - `App { slug }` -> `{"app": "slug_value"}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WildcardType {
    /// *.mynetwk.biz - global wildcard (dashboard, redirections)
    Global,
    /// *.{slug}.mynetwk.biz - per-application wildcard
    App { slug: String },
    /// *.code.mynetwk.biz - legacy code server wildcard
    LegacyCode,
}

impl WildcardType {
    /// Get the domain pattern for this wildcard type
    pub fn domain_pattern(&self, base_domain: &str) -> String {
        match self {
            Self::Global => format!("*.{}", base_domain),
            Self::App { slug } => format!("*.{}.{}", slug, base_domain),
            Self::LegacyCode => format!("*.code.{}", base_domain),
        }
    }

    /// Get the unique ID for this wildcard type
    pub fn id(&self) -> String {
        match self {
            Self::Global => "wildcard-global".to_string(),
            Self::App { slug } => format!("app-{}", slug),
            Self::LegacyCode => "wildcard-code".to_string(),
        }
    }

    /// Get display name
    pub fn display_name(&self) -> String {
        match self {
            Self::Global => "Global (Dashboard)".to_string(),
            Self::App { slug } => format!("App: {}", slug),
            Self::LegacyCode => "Code Server (Legacy)".to_string(),
        }
    }

    /// Create an App wildcard type for a given application slug
    pub fn for_app(slug: &str) -> Self {
        Self::App {
            slug: slug.to_string(),
        }
    }
}

impl Serialize for WildcardType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Global => serializer.serialize_str("global"),
            Self::LegacyCode => serializer.serialize_str("code"),
            Self::App { slug } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("app", slug)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for WildcardType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct WildcardTypeVisitor;

        impl<'de> de::Visitor<'de> for WildcardTypeVisitor {
            type Value = WildcardType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(
                    r#"a string ("global", "main", "code") or a map ({"app": "slug"})"#,
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<WildcardType, E>
            where
                E: de::Error,
            {
                match value {
                    "global" | "main" => Ok(WildcardType::Global),
                    "code" => Ok(WildcardType::LegacyCode),
                    other => Err(de::Error::unknown_variant(
                        other,
                        &["global", "main", "code"],
                    )),
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<WildcardType, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let key: String = map
                    .next_key()?
                    .ok_or_else(|| de::Error::custom("expected a key in map"))?;

                match key.as_str() {
                    "app" => {
                        let slug: String = map.next_value()?;
                        Ok(WildcardType::App { slug })
                    }
                    other => Err(de::Error::unknown_field(other, &["app"])),
                }
            }
        }

        deserializer.deserialize_any(WildcardTypeVisitor)
    }
}

/// Information about an issued certificate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub id: String,
    pub wildcard_type: WildcardType,
    pub domains: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub cert_path: String,
    pub key_path: String,
}

impl CertificateInfo {
    /// Check if certificate needs renewal
    pub fn needs_renewal(&self, threshold_days: u32) -> bool {
        let now = Utc::now();
        let threshold = chrono::Duration::days(threshold_days as i64);
        self.expires_at - now < threshold
    }

    /// Check if certificate is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Get days until expiration
    pub fn days_until_expiry(&self) -> i64 {
        let now = Utc::now();
        (self.expires_at - now).num_days()
    }
}

#[derive(Error, Debug)]
pub enum AcmeError {
    #[error("ACME not initialized")]
    NotInitialized,

    #[error("ACME challenge failed: {0}")]
    ChallengeFailed(String),

    #[error("Certificate not found: {0}")]
    CertificateNotFound(String),

    #[error("Cloudflare API error: {0}")]
    CloudflareError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("ACME protocol error: {0}")]
    ProtocolError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type AcmeResult<T> = Result<T, AcmeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_type_serialize_global() {
        let wt = WildcardType::Global;
        let json = serde_json::to_string(&wt).unwrap();
        assert_eq!(json, r#""global""#);
    }

    #[test]
    fn test_wildcard_type_serialize_legacy_code() {
        let wt = WildcardType::LegacyCode;
        let json = serde_json::to_string(&wt).unwrap();
        assert_eq!(json, r#""code""#);
    }

    #[test]
    fn test_wildcard_type_serialize_app() {
        let wt = WildcardType::for_app("www");
        let json = serde_json::to_string(&wt).unwrap();
        assert_eq!(json, r#"{"app":"www"}"#);
    }

    #[test]
    fn test_wildcard_type_deserialize_global() {
        let wt: WildcardType = serde_json::from_str(r#""global""#).unwrap();
        assert_eq!(wt, WildcardType::Global);
    }

    #[test]
    fn test_wildcard_type_deserialize_main_alias() {
        let wt: WildcardType = serde_json::from_str(r#""main""#).unwrap();
        assert_eq!(wt, WildcardType::Global);
    }

    #[test]
    fn test_wildcard_type_deserialize_code() {
        let wt: WildcardType = serde_json::from_str(r#""code""#).unwrap();
        assert_eq!(wt, WildcardType::LegacyCode);
    }

    #[test]
    fn test_wildcard_type_deserialize_app() {
        let wt: WildcardType = serde_json::from_str(r#"{"app":"www"}"#).unwrap();
        assert_eq!(wt, WildcardType::App { slug: "www".to_string() });
    }

    #[test]
    fn test_wildcard_type_id() {
        assert_eq!(WildcardType::Global.id(), "wildcard-global");
        assert_eq!(WildcardType::LegacyCode.id(), "wildcard-code");
        assert_eq!(WildcardType::for_app("www").id(), "app-www");
    }

    #[test]
    fn test_wildcard_type_domain_pattern() {
        let base = "mynetwk.biz";
        assert_eq!(WildcardType::Global.domain_pattern(base), "*.mynetwk.biz");
        assert_eq!(WildcardType::LegacyCode.domain_pattern(base), "*.code.mynetwk.biz");
        assert_eq!(WildcardType::for_app("www").domain_pattern(base), "*.www.mynetwk.biz");
    }

    #[test]
    fn test_wildcard_type_display_name() {
        assert_eq!(WildcardType::Global.display_name(), "Global (Dashboard)");
        assert_eq!(WildcardType::LegacyCode.display_name(), "Code Server (Legacy)");
        assert_eq!(WildcardType::for_app("www").display_name(), "App: www");
    }

    #[test]
    fn test_backward_compat_certificate_info_with_main() {
        // Simulates an old index.json entry with "main" wildcard_type
        let json = r#"{
            "id": "wildcard-main",
            "wildcard_type": "main",
            "domains": ["*.mynetwk.biz"],
            "issued_at": "2025-01-01T00:00:00Z",
            "expires_at": "2025-04-01T00:00:00Z",
            "cert_path": "/path/to/cert.crt",
            "key_path": "/path/to/key.key"
        }"#;
        let info: CertificateInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.wildcard_type, WildcardType::Global);
    }
}
