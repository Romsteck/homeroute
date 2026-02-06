use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::reverseproxy::{AddProxyHost, DeleteProxyHost, ToggleProxyHost};
use crate::types::ReverseProxyPageData;

fn rp_icon() -> AnyView {
    view! { <IconGlobe class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn ReverseProxyPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_rp_data());

    view! {
        <PageHeader title="Reverse Proxy" icon=rp_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <RpContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_rp_data() -> Result<ReverseProxyPageData, ServerFnError> {
    crate::server_fns::reverseproxy::get_reverseproxy_data().await
}

#[component]
fn RpContent(data: ReverseProxyPageData) -> impl IntoView {
    let active_hosts = data.hosts.iter().filter(|h| h.enabled).count();
    let base_domain = data.base_domain.clone();
    let base_domain_display = if data.base_domain.is_empty() {
        "Non configuré".to_string()
    } else {
        data.base_domain
    };

    let action = get_query_param("action");
    let delete_id = get_query_param("delete");

    let add_action = ServerAction::<AddProxyHost>::new();
    let delete_action = ServerAction::<DeleteProxyHost>::new();
    let toggle_action = ServerAction::<ToggleProxyHost>::new();

    view! {
        // Status cards
        <Section title="État">
            <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Proxy"</p>
                    <span class=if data.proxy_running {
                        "text-green-400 font-medium"
                    } else {
                        "text-red-400 font-medium"
                    }>
                        {if data.proxy_running { "En ligne" } else { "Hors ligne" }}
                    </span>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Domaine"</p>
                    <p class="text-sm font-mono text-blue-400 truncate">
                        {base_domain_display}
                    </p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Routes actives"</p>
                    <p class="text-2xl font-bold text-white">{data.active_routes}</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Hôtes"</p>
                    <p class="text-2xl font-bold text-green-400">{active_hosts}</p>
                    <p class="text-xs text-gray-500">{format!("{} actif(s)", active_hosts)}</p>
                </div>
            </div>
        </Section>

        // Hosts table
        <Section title="Hôtes standalone">
            <div class="flex justify-end mb-4">
                <a
                    href="/reverseproxy?action=add"
                    class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                >
                    <IconPlus class="w-4 h-4"/>
                    "Ajouter un hôte"
                </a>
            </div>
            {if data.hosts.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconGlobe class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucun hôte configuré"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Domaine"</th>
                                    <th class="pb-2 pr-4">"Cible"</th>
                                    <th class="pb-2 pr-4">"Options"</th>
                                    <th class="pb-2 pr-4">"État"</th>
                                    <th class="pb-2">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.hosts.into_iter().map(|h| {
                                    let domain = h.custom_domain.clone().unwrap_or_else(|| {
                                        format!("{}.{}", h.subdomain.as_deref().unwrap_or("?"), &base_domain)
                                    });
                                    let target = format!("{}:{}", h.target_host, h.target_port);
                                    let state_class = if h.enabled {
                                        "bg-green-500/20 text-green-400"
                                    } else {
                                        "bg-gray-500/20 text-gray-500"
                                    };
                                    let host_id = h.id.clone();
                                    let host_id_del = h.id.clone();
                                    let toggle_label = if h.enabled { "Désactiver" } else { "Activer" };
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4">
                                                <div class="flex items-center gap-2 flex-wrap">
                                                    <span class="font-mono text-blue-400">{domain}</span>
                                                    {h.local_only.then(|| view! {
                                                        <span class="text-xs text-yellow-400 bg-yellow-900/30 px-1.5 py-0.5">"Local"</span>
                                                    })}
                                                    {h.require_auth.then(|| view! {
                                                        <span class="text-xs text-purple-400 bg-purple-900/30 px-1.5 py-0.5">"Auth"</span>
                                                    })}
                                                </div>
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-300 text-xs">{target}</td>
                                            <td class="py-2 pr-4 text-xs text-gray-400">
                                                {if h.local_only && h.require_auth {
                                                    "Local + Auth"
                                                } else if h.local_only {
                                                    "Local only"
                                                } else if h.require_auth {
                                                    "Auth requise"
                                                } else {
                                                    "Public"
                                                }}
                                            </td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {state_class}")>
                                                    {if h.enabled { "Actif" } else { "Inactif" }}
                                                </span>
                                            </td>
                                            <td class="py-2">
                                                <div class="flex items-center gap-2">
                                                    <ActionForm action=toggle_action attr:class="inline">
                                                        <input type="hidden" name="id" value=host_id/>
                                                        <button type="submit" class="text-xs text-blue-400 hover:text-blue-300">
                                                            {toggle_label}
                                                        </button>
                                                    </ActionForm>
                                                    <a
                                                        href=format!("/reverseproxy?delete={host_id_del}")
                                                        class="text-xs text-red-400 hover:text-red-300"
                                                    >
                                                        "Supprimer"
                                                    </a>
                                                </div>
                                            </td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </Section>

        // Local networks
        {(!data.local_networks.is_empty()).then(|| view! {
            <Section title="Réseaux locaux">
                <div class="flex flex-wrap gap-2">
                    {data.local_networks.into_iter().map(|n| view! {
                        <span class="px-2 py-1 text-xs bg-gray-800 border border-gray-700 font-mono text-gray-300 rounded">{n}</span>
                    }).collect_view()}
                </div>
            </Section>
        })}

        // Add host modal
        {(action.as_deref() == Some("add")).then(|| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                    <div class="flex items-center justify-between mb-4">
                        <h3 class="text-lg font-medium text-white">"Ajouter un hôte"</h3>
                        <a href="/reverseproxy" class="text-gray-400 hover:text-white">"X"</a>
                    </div>
                    <ActionForm action=add_action attr:class="space-y-4">
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Sous-domaine"</label>
                            <input type="text" name="subdomain" required
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="mon-service"/>
                        </div>
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Domaine personnalisé (optionnel)"</label>
                            <input type="text" name="custom_domain"
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="custom.example.com"/>
                        </div>
                        <div class="grid grid-cols-2 gap-4">
                            <div>
                                <label class="block text-sm text-gray-300 mb-1">"Hôte cible"</label>
                                <input type="text" name="target_host" required
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                    placeholder="10.0.0.1"/>
                            </div>
                            <div>
                                <label class="block text-sm text-gray-300 mb-1">"Port cible"</label>
                                <input type="number" name="target_port" required value="80"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"/>
                            </div>
                        </div>
                        <div class="flex items-center gap-6">
                            <label class="flex items-center gap-2 text-sm text-gray-300">
                                <input type="checkbox" name="local_only" value="on"
                                    class="rounded border-gray-600 bg-gray-900"/>
                                "Accès local uniquement"
                            </label>
                            <label class="flex items-center gap-2 text-sm text-gray-300">
                                <input type="checkbox" name="require_auth" value="on"
                                    class="rounded border-gray-600 bg-gray-900"/>
                                "Auth requise"
                            </label>
                        </div>
                        <div class="flex justify-end gap-3 pt-2">
                            <a href="/reverseproxy" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors">
                                "Ajouter"
                            </button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        })}

        // Delete confirmation modal
        {delete_id.map(|did| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-sm mx-4">
                    <h3 class="text-lg font-medium text-white mb-4">"Confirmer la suppression"</h3>
                    <p class="text-sm text-gray-400 mb-6">"Voulez-vous vraiment supprimer cet hôte ?"</p>
                    <div class="flex justify-end gap-3">
                        <a href="/reverseproxy" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                        <ActionForm action=delete_action attr:class="inline">
                            <input type="hidden" name="id" value=did/>
                            <button type="submit" class="px-4 py-2 text-sm bg-red-600 hover:bg-red-700 text-white transition-colors">
                                "Supprimer"
                            </button>
                        </ActionForm>
                    </div>
                </div>
            </div>
        })}
    }
}
