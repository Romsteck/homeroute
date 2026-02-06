use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::types::SettingsPageData;

fn settings_icon() -> AnyView {
    view! { <IconSettings class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_settings_data());

    view! {
        <PageHeader title="Paramètres" icon=settings_icon/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <SettingsContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_settings_data() -> Result<SettingsPageData, ServerFnError> {
    crate::server_fns::settings::get_settings_data().await
}

#[component]
fn SettingsContent(data: SettingsPageData) -> impl IntoView {
    view! {
        <Section title="Configuration système">
            <div class="space-y-0 divide-y divide-gray-700/50">
                <ConfigRow label="Domaine de base" value=data.base_domain/>
                <ConfigRow label="Port API" value=data.api_port.to_string()/>
                <ConfigRow label="Répertoire données" value=data.data_dir/>
                <ConfigRow
                    label="Email ACME"
                    value=data.acme_email.unwrap_or_else(|| "Non configuré".to_string())
                />
                <ConfigRow
                    label="ACME Staging"
                    value=if data.acme_staging { "Oui".to_string() } else { "Non".to_string() }
                />
                <ConfigRow label="DDNS Cron" value=data.ddns_cron/>
            </div>
        </Section>
    }
}

#[component]
fn ConfigRow(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between py-3">
            <span class="text-sm text-gray-400">{label}</span>
            <span class="text-sm text-white font-mono">{value}</span>
        </div>
    }
}
