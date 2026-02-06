use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::firewall::{AddFirewallRule, DeleteFirewallRule, ToggleFirewallRule};
use crate::types::FirewallData;

fn firewall_icon() -> AnyView {
    view! { <IconShieldCheck class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn FirewallPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_firewall_data());

    view! {
        <PageHeader title="Firewall IPv6" icon=firewall_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <FirewallContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_firewall_data() -> Result<FirewallData, ServerFnError> {
    crate::server_fns::firewall::get_firewall_data().await
}

#[component]
fn FirewallContent(data: FirewallData) -> impl IntoView {
    if !data.available {
        return view! {
            <Section>
                <div class="text-center py-12 text-gray-500">
                    <IconShieldCheck class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                    <p>"Firewall non disponible"</p>
                </div>
            </Section>
        }
        .into_any();
    }

    let action = get_query_param("action");
    let delete_id = get_query_param("delete");

    let add_action = ServerAction::<AddFirewallRule>::new();
    let delete_action = ServerAction::<DeleteFirewallRule>::new();

    // Find rule to delete for confirmation modal
    let delete_rule_desc = delete_id.as_ref().and_then(|did| {
        data.rules.iter().find(|r| r.id == *did).map(|r| r.description.clone())
    });

    let rule_count = data.rules.len();
    let enabled_class = if data.enabled {
        "bg-green-500/20 text-green-400"
    } else {
        "bg-red-500/20 text-red-400"
    };
    let enabled_label = if data.enabled { "Actif" } else { "Inactif" };

    view! {
        <Section title="État">
            <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Statut"</p>
                    <span class=format!("px-2 py-0.5 text-sm rounded {enabled_class}")>{enabled_label}</span>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Préfixe LAN"</p>
                    <p class="text-sm font-mono text-white">
                        {data.lan_prefix.unwrap_or_else(|| "-".into())}
                    </p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Politique par défaut"</p>
                    <span class="px-2 py-0.5 text-sm bg-orange-500/20 text-orange-400 rounded">
                        {data.default_policy.to_uppercase()}
                    </span>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Règles"</p>
                    <p class="text-2xl font-bold text-white">{rule_count}</p>
                </div>
            </div>
        </Section>

        <Section title="Règles">
            <div class="flex justify-end mb-4">
                <a href="/firewall?action=add" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">
                    "Ajouter une règle"
                </a>
            </div>
            {if data.rules.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconShield class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucune règle configurée"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Description"</th>
                                    <th class="pb-2 pr-4">"Protocole"</th>
                                    <th class="pb-2 pr-4">"Port"</th>
                                    <th class="pb-2 pr-4">"Destination"</th>
                                    <th class="pb-2 pr-4">"Source"</th>
                                    <th class="pb-2 pr-4">"État"</th>
                                    <th class="pb-2">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.rules.into_iter().map(|r| {
                                    let port_display = if r.dest_port == 0 {
                                        "Tous".to_string()
                                    } else if r.dest_port_end > 0 && r.dest_port_end != r.dest_port {
                                        format!("{}-{}", r.dest_port, r.dest_port_end)
                                    } else {
                                        r.dest_port.to_string()
                                    };
                                    let proto_color = match r.protocol.as_str() {
                                        "tcp" => "bg-blue-500/20 text-blue-400",
                                        "udp" => "bg-purple-500/20 text-purple-400",
                                        _ => "bg-gray-500/20 text-gray-400",
                                    };
                                    let toggle_label = if r.enabled { "Désactiver" } else { "Activer" };
                                    let state_class = if r.enabled {
                                        "bg-green-500/20 text-green-400"
                                    } else {
                                        "bg-gray-500/20 text-gray-500"
                                    };
                                    let toggle_action = ServerAction::<ToggleFirewallRule>::new();
                                    let rule_id = r.id.clone();
                                    let delete_href = format!("/firewall?delete={}", r.id);
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-gray-300">{r.description}</td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {proto_color}")>
                                                    {r.protocol.to_uppercase()}
                                                </span>
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-300 text-xs">{port_display}</td>
                                            <td class="py-2 pr-4 font-mono text-blue-400 text-xs">
                                                {if r.dest_address.is_empty() { "Tous".to_string() } else { r.dest_address }}
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-500 text-xs">
                                                {if r.source_address.is_empty() { "Tous".to_string() } else { r.source_address }}
                                            </td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {state_class}")>
                                                    {if r.enabled { "Actif" } else { "Inactif" }}
                                                </span>
                                            </td>
                                            <td class="py-2">
                                                <div class="flex items-center gap-2">
                                                    <ActionForm action=toggle_action>
                                                        <input type="hidden" name="id" value=rule_id/>
                                                        <button type="submit" class="text-xs text-blue-400 hover:text-blue-300">
                                                            {toggle_label}
                                                        </button>
                                                    </ActionForm>
                                                    <a href=delete_href class="text-xs text-red-400 hover:text-red-300">
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

        // Add rule modal
        {(action.as_deref() == Some("add")).then(|| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                    <h3 class="text-lg font-medium text-white mb-4">"Ajouter une règle"</h3>
                    <ActionForm action=add_action>
                        <div class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Description"</label>
                                <input type="text" name="description" required
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Protocole"</label>
                                <select name="protocol"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500">
                                    <option value="tcp">"TCP"</option>
                                    <option value="udp">"UDP"</option>
                                    <option value="icmpv6">"ICMPv6"</option>
                                </select>
                            </div>
                            <div class="grid grid-cols-2 gap-4">
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Port destination"</label>
                                    <input type="number" name="dest_port" value="0" min="0" max="65535"
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Port fin (plage)"</label>
                                    <input type="number" name="dest_port_end" value="0" min="0" max="65535"
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Adresse destination"</label>
                                <input type="text" name="dest_address" value=""
                                    placeholder="Laisser vide pour tous"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Adresse source"</label>
                                <input type="text" name="source_address" value=""
                                    placeholder="Laisser vide pour tous"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                        </div>
                        <div class="flex justify-end gap-3 mt-6">
                            <a href="/firewall" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">"Créer"</button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        })}

        // Delete confirmation modal
        {delete_id.map(|did| {
            let desc = delete_rule_desc.unwrap_or_default();
            view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                    <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-md mx-4">
                        <h3 class="text-lg font-medium text-white mb-2">"Supprimer la règle"</h3>
                        <p class="text-sm text-gray-400 mb-6">
                            "Voulez-vous vraiment supprimer la règle "
                            <span class="text-white font-medium">{desc}</span>
                            " ?"
                        </p>
                        <ActionForm action=delete_action>
                            <input type="hidden" name="id" value=did/>
                            <div class="flex justify-end gap-3">
                                <a href="/firewall" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                                <button type="submit" class="px-4 py-2 text-sm bg-red-600 hover:bg-red-700 text-white">"Supprimer"</button>
                            </div>
                        </ActionForm>
                    </div>
                </div>
            }
        })}
    }
    .into_any()
}
