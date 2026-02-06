use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::ddns::ForceDdnsUpdate;
use crate::types::DdnsPageData;

fn ddns_icon() -> AnyView {
    view! { <IconGlobe class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn DdnsPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_ddns_data());

    view! {
        <PageHeader title="Dynamic DNS" icon=ddns_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <DdnsContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_ddns_data() -> Result<DdnsPageData, ServerFnError> {
    crate::server_fns::ddns::get_ddns_data().await
}

#[component]
fn DdnsContent(data: DdnsPageData) -> impl IntoView {
    let interface_display = data.interface.clone();
    let force_update_action = ServerAction::<ForceDdnsUpdate>::new();

    view! {
        // Configuration
        <Section title="Configuration">
            {if !data.configured {
                view! {
                    <div class="bg-yellow-500/10 border border-yellow-500/30 p-4">
                        <div class="flex items-center gap-2">
                            <IconAlertCircle class="w-5 h-5 text-yellow-400"/>
                            <p class="text-yellow-400">"DDNS non configuré — variables Cloudflare manquantes dans .env"</p>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
                        <ConfigCard label="Enregistrement" value=data.record_name.unwrap_or_else(|| "-".into())/>
                        <ConfigCard label="Interface" value=data.interface.clone()/>
                        <ConfigCard label="Zone ID" value=data.zone_id_masked.unwrap_or_else(|| "-".into())/>
                        <div class="bg-gray-800 border border-gray-700 p-4">
                            <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Proxy Cloudflare"</p>
                            {if data.proxied {
                                view! {
                                    <span class="px-2 py-0.5 text-xs bg-green-500/20 text-green-400 rounded">"Actif"</span>
                                }.into_any()
                            } else {
                                view! {
                                    <span class="px-2 py-0.5 text-xs bg-gray-500/20 text-gray-400 rounded">"Inactif"</span>
                                }.into_any()
                            }}
                        </div>
                    </div>
                }.into_any()
            }}
        </Section>

        // Force update button (SSR ActionForm)
        {data.configured.then(|| view! {
            <Section>
                <ActionForm action=force_update_action attr:class="inline">
                    <button
                        type="submit"
                        class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                    >
                        <IconRefreshCw class="w-4 h-4"/>
                        "Forcer la mise à jour DNS"
                    </button>
                </ActionForm>
            </Section>
        })}

        // Current state
        {data.configured.then(|| view! {
            <Section title="État actuel" contrast=true>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div class="bg-gray-800 border border-gray-700 p-4">
                        <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"IPv6 actuelle"</p>
                        <p class="font-mono text-sm text-blue-400">
                            {data.current_ipv6.clone().unwrap_or_else(|| "Aucune adresse IPv6 trouvée".into())}
                        </p>
                        <p class="text-xs text-gray-500 mt-1">{format!("Interface: {}", interface_display)}</p>
                    </div>
                    <div class="bg-gray-800 border border-gray-700 p-4">
                        <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Cloudflare"</p>
                        {match &data.cloudflare_ip {
                            Some(ip) => view! {
                                <p class="font-mono text-sm text-purple-400">{ip.clone()}</p>
                            }.into_any(),
                            None => view! {
                                <p class="text-sm text-gray-500">"Non disponible"</p>
                            }.into_any(),
                        }}
                    </div>
                </div>
            </Section>
        })}

        // Logs
        {(!data.logs.is_empty()).then(|| view! {
            <Section title="Journaux récents">
                <div class="bg-gray-900 border border-gray-700 p-4 max-h-80 overflow-y-auto">
                    <div class="space-y-1">
                        {data.logs.into_iter().map(|line| {
                            let class = if line.contains("ERREUR") || line.contains("ERROR") {
                                "text-red-400"
                            } else if line.contains("MAJ") || line.contains("CREE") || line.contains("UPDATE") {
                                "text-green-400"
                            } else {
                                "text-gray-400"
                            };
                            view! {
                                <p class=format!("font-mono text-xs {class}")>{line}</p>
                            }
                        }).collect_view()}
                    </div>
                </div>
            </Section>
        })}
    }
}

#[component]
fn ConfigCard(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="bg-gray-800 border border-gray-700 p-4">
            <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">{label}</p>
            <p class="text-sm text-white font-mono truncate">{value}</p>
        </div>
    }
}
