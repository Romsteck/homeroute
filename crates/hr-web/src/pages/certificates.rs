use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::certificates::RenewCertificates;
use crate::types::{CertEntry, CertificatesData};

fn certs_icon() -> AnyView {
    view! { <IconLock class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn CertificatesPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_certificates_data());

    view! {
        <PageHeader title="Certificats TLS" icon=certs_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <CertificatesContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_certificates_data() -> Result<CertificatesData, ServerFnError> {
    crate::server_fns::certificates::get_certificates_data().await
}

#[component]
fn CertificatesContent(data: CertificatesData) -> impl IntoView {
    let renew_action = ServerAction::<RenewCertificates>::new();

    view! {
        // Provider status
        <Section title="Fournisseur">
            <div class="bg-gray-800 border border-gray-700 p-4 max-w-xl">
                <div class="flex items-center gap-3">
                    {if data.initialized {
                        view! { <IconCheck class="w-5 h-5 text-green-400"/> }.into_any()
                    } else {
                        view! { <IconAlertCircle class="w-5 h-5 text-yellow-400"/> }.into_any()
                    }}
                    <div>
                        <p class="text-white font-medium">{data.provider.clone()}</p>
                        <p class="text-xs text-gray-400">{format!("Domaine: {}", data.base_domain)}</p>
                    </div>
                    {data.initialized.then(|| view! {
                        <span class="ml-auto px-2 py-0.5 text-xs bg-green-500/20 text-green-400 rounded">"Actif"</span>
                    })}
                </div>
            </div>
        </Section>

        // Renew button (SSR ActionForm)
        <Section>
            <ActionForm action=renew_action attr:class="inline">
                <button
                    type="submit"
                    class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                >
                    <IconRefreshCw class="w-4 h-4"/>
                    "Renouveler les certificats"
                </button>
            </ActionForm>
        </Section>

        // Certificates list
        <Section title="Certificats">
            {if data.certificates.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconLock class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucun certificat"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="space-y-3">
                        {data.certificates.into_iter().map(|c| {
                            view! { <CertCard cert=c/> }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </Section>
    }
}

#[component]
fn CertCard(cert: CertEntry) -> impl IntoView {
    let (status_class, status_label) = if cert.expired {
        ("bg-red-500/20 text-red-400", format!("Expiré depuis {}j", -cert.days_until_expiry))
    } else if cert.needs_renewal {
        ("bg-yellow-500/20 text-yellow-400", format!("Renouvellement dans {}j", cert.days_until_expiry))
    } else {
        ("bg-green-500/20 text-green-400", format!("Valide — {}j restants", cert.days_until_expiry))
    };

    let issued = format_date(&cert.issued_at);
    let expires = format_date(&cert.expires_at);

    view! {
        <div class="bg-gray-800 border border-gray-700 p-4">
            <div class="flex items-center justify-between mb-2">
                <div class="flex items-center gap-2">
                    <IconGlobe class="w-4 h-4 text-blue-400"/>
                    <span class="text-white font-medium">
                        {cert.domains.first().cloned().unwrap_or_default()}
                    </span>
                    <span class="px-1.5 py-0.5 text-xs bg-gray-700 text-gray-300 rounded">
                        {cert.cert_type}
                    </span>
                </div>
                <span class=format!("px-2 py-0.5 text-xs rounded {status_class}")>
                    {status_label}
                </span>
            </div>
            <div class="flex gap-6 text-xs text-gray-400">
                <span>{format!("Émis: {issued}")}</span>
                <span>{format!("Expire: {expires}")}</span>
            </div>
        </div>
    }
}

fn format_date(rfc3339: &str) -> String {
    rfc3339.split('T').next().unwrap_or(rfc3339).to_string()
}
