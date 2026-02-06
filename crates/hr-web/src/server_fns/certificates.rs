use leptos::prelude::*;

use crate::types::CertificatesData;
#[cfg(feature = "ssr")]
use crate::types::CertEntry;

#[server]
pub async fn get_certificates_data() -> Result<CertificatesData, ServerFnError> {
    use std::sync::Arc;
    use hr_acme::AcmeManager;

    let acme: Arc<AcmeManager> = expect_context();

    let initialized = acme.is_initialized();
    let base_domain = acme.base_domain().to_string();
    let threshold = acme.renewal_threshold_days();

    let certificates = if initialized {
        acme.list_certificates()
            .map(|certs| {
                certs
                    .into_iter()
                    .map(|c| CertEntry {
                        id: c.id.clone(),
                        cert_type: format!("{:?}", c.wildcard_type),
                        domains: c.domains.clone(),
                        issued_at: c.issued_at.to_rfc3339(),
                        expires_at: c.expires_at.to_rfc3339(),
                        days_until_expiry: c.days_until_expiry(),
                        needs_renewal: c.needs_renewal(threshold),
                        expired: c.is_expired(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    Ok(CertificatesData {
        initialized,
        provider: "Let's Encrypt".to_string(),
        base_domain,
        certificates,
    })
}

#[server]
pub async fn renew_certificates() -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_acme::AcmeManager;

    let acme: Arc<AcmeManager> = expect_context();

    let needing_renewal = acme
        .certificates_needing_renewal()
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    if needing_renewal.is_empty() {
        leptos_axum::redirect("/certificates?msg=Tous+les+certificats+sont+%C3%A0+jour");
        return Ok(());
    }

    let mut renewed = 0u32;
    let mut errors = Vec::new();

    for cert in &needing_renewal {
        match acme.request_wildcard(cert.wildcard_type).await {
            Ok(_) => renewed += 1,
            Err(e) => errors.push(format!("{}: {e}", cert.id)),
        }
    }

    if errors.is_empty() {
        leptos_axum::redirect("/certificates?msg=Renouvellement+lanc%C3%A9");
    } else {
        leptos_axum::redirect(&format!(
            "/certificates?msg=error&detail={}+renouvel%C3%A9(s),+{}+erreur(s)",
            renewed,
            errors.len()
        ));
    }
    Ok(())
}
