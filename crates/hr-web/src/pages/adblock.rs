use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::adblock::{
    AddWhitelistDomain, RemoveWhitelistDomain, UpdateAdblockSources,
};
use crate::types::AdblockPageData;
use crate::utils::format_number;

fn adblock_icon() -> AnyView {
    view! { <IconShield class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn AdblockPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_adblock_data());

    view! {
        <PageHeader title="AdBlock" icon=adblock_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <AdblockContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_adblock_data() -> Result<AdblockPageData, ServerFnError> {
    crate::server_fns::adblock::get_adblock_data().await
}

async fn do_search_adblock(query: String) -> Result<crate::types::AdblockSearchResult, ServerFnError> {
    crate::server_fns::adblock::search_adblock(query).await
}

#[component]
fn AdblockContent(data: AdblockPageData) -> impl IntoView {
    let search_query = get_query_param("search");

    let update_action = ServerAction::<UpdateAdblockSources>::new();
    let add_whitelist_action = ServerAction::<AddWhitelistDomain>::new();

    let status_class = if data.enabled {
        "bg-green-500/20 text-green-400"
    } else {
        "bg-red-500/20 text-red-400"
    };
    let status_label = if data.enabled { "Actif" } else { "Inactif" };

    // Load search results if a query is present
    let search_result = search_query.clone().map(|q| {
        Resource::new(move || (), {
            let q = q.clone();
            move |_| {
                let q = q.clone();
                do_search_adblock(q)
            }
        })
    });

    view! {
        <Section title="Statistiques">
            <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
                <StatCard label="Statut" value=status_label.to_string() value_class=status_class/>
                <StatCard label="Domaines bloqués" value=format_number(data.domain_count) value_class="text-white"/>
                <StatCard label="Sources" value=data.source_count.to_string() value_class="text-white"/>
                <StatCard label="Whitelist" value=data.whitelist_count.to_string() value_class="text-white"/>
            </div>
        </Section>

        <Section title="Sources de blocage">
            <div class="flex justify-end mb-4">
                <ActionForm action=update_action>
                    <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">
                        "Mettre à jour les sources"
                    </button>
                </ActionForm>
            </div>
            {if data.sources.is_empty() {
                view! { <p class="text-gray-500">"Aucune source configurée"</p> }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Nom"</th>
                                    <th class="pb-2">"URL"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.sources.into_iter().map(|s| {
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-white">{s.name}</td>
                                            <td class="py-2 font-mono text-xs text-blue-400 truncate max-w-md">
                                                {s.url}
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

        // Search section
        <Section title="Recherche">
            <form method="GET" action="/adblock" class="flex gap-3 mb-4">
                <input type="text" name="search"
                    value=search_query.clone().unwrap_or_default()
                    placeholder="Rechercher un domaine..."
                    class="flex-1 px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">
                    "Rechercher"
                </button>
            </form>
            {search_result.map(|res| {
                view! {
                    <Suspense fallback=|| view! { <p class="text-gray-400 text-sm">"Recherche en cours..."</p> }>
                        {move || Suspend::new(async move {
                            match res.await {
                                Ok(r) => {
                                    let blocked_class = if r.is_blocked {
                                        "bg-red-500/20 border-red-500/50 text-red-300"
                                    } else {
                                        "bg-green-500/20 border-green-500/50 text-green-300"
                                    };
                                    let blocked_label = if r.is_blocked { "Bloqué" } else { "Autorisé" };
                                    view! {
                                        <div class=format!("px-4 py-3 border text-sm mb-4 {blocked_class}")>
                                            <span class="font-mono">{r.query.clone()}</span>
                                            " — "
                                            <span class="font-medium">{blocked_label}</span>
                                        </div>
                                        {(!r.results.is_empty()).then(|| view! {
                                            <div class="space-y-1 max-h-64 overflow-y-auto">
                                                {r.results.into_iter().map(|d| view! {
                                                    <div class="text-xs font-mono text-gray-400 py-0.5">{d}</div>
                                                }).collect_view()}
                                            </div>
                                        })}
                                    }.into_any()
                                }
                                Err(e) => view! {
                                    <p class="text-red-400 text-sm">{e.to_string()}</p>
                                }.into_any(),
                            }
                        })}
                    </Suspense>
                }
            })}
        </Section>

        // Whitelist section
        <Section title="Whitelist">
            <div class="mb-4">
                <ActionForm action=add_whitelist_action>
                    <div class="flex gap-3">
                        <input type="text" name="domain" required
                            placeholder="example.com"
                            class="flex-1 px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                        <button type="submit" class="px-4 py-2 text-sm bg-green-600 hover:bg-green-700 text-white">
                            "Ajouter"
                        </button>
                    </div>
                </ActionForm>
            </div>
            {if data.whitelist.is_empty() {
                view! { <p class="text-gray-500">"Aucun domaine en whitelist"</p> }.into_any()
            } else {
                view! {
                    <div class="flex flex-wrap gap-2">
                        {data.whitelist.into_iter().map(|d| {
                            let remove_action = ServerAction::<RemoveWhitelistDomain>::new();
                            let domain_val = d.clone();
                            view! {
                                <div class="flex items-center gap-1 px-2 py-1 bg-green-500/20 rounded">
                                    <span class="text-xs text-green-400 font-mono">{d}</span>
                                    <ActionForm action=remove_action>
                                        <input type="hidden" name="domain" value=domain_val/>
                                        <button type="submit" class="text-red-400 hover:text-red-300 text-xs ml-1">"x"</button>
                                    </ActionForm>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </Section>
    }
}

#[component]
fn StatCard(label: &'static str, value: String, value_class: &'static str) -> impl IntoView {
    view! {
        <div class="bg-gray-800 border border-gray-700 p-4">
            <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">{label}</p>
            <p class=format!("text-2xl font-bold {value_class}")>{value}</p>
        </div>
    }
}
