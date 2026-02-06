use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::wol::{
    BulkWake, CreateWolSchedule, DeleteWolSchedule, ExecuteWolSchedule, RebootServer,
    ShutdownServer, ToggleWolSchedule, WakeServer,
};
use crate::types::WolData;

fn wol_icon() -> AnyView {
    view! { <IconPower class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn WolPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_wol_data());

    view! {
        <PageHeader title="Wake-on-LAN" icon=wol_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <WolContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_wol_data() -> Result<WolData, ServerFnError> {
    crate::server_fns::wol::get_wol_data().await
}

#[component]
fn WolContent(data: WolData) -> impl IntoView {
    let action = get_query_param("action");
    let delete_id = get_query_param("delete");

    let bulk_wake_action = ServerAction::<BulkWake>::new();
    let create_schedule_action = ServerAction::<CreateWolSchedule>::new();
    let delete_schedule_action = ServerAction::<DeleteWolSchedule>::new();

    // Collect all server IDs for bulk wake
    let all_ids: String = data
        .servers
        .iter()
        .filter(|s| s.mac.is_some())
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let has_wakeable = !all_ids.is_empty();

    // Find schedule to delete for confirmation
    let delete_schedule_desc = delete_id.as_ref().and_then(|did| {
        data.schedules
            .iter()
            .find(|s| s.id == *did)
            .map(|s| format!("{} - {}", s.server_name, s.action))
    });

    // Clone servers for the schedule add modal select
    let servers_for_modal = data.servers.clone();

    view! {
        // Servers
        <Section title="Serveurs">
            {has_wakeable.then(|| view! {
                <div class="flex justify-end mb-4">
                    <ActionForm action=bulk_wake_action>
                        <input type="hidden" name="server_ids" value=all_ids/>
                        <button type="submit" class="px-4 py-2 text-sm bg-green-600 hover:bg-green-700 text-white">
                            "Réveiller tous"
                        </button>
                    </ActionForm>
                </div>
            })}
            {if data.servers.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconServer class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucun serveur configuré"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                        {data.servers.into_iter().map(|s| {
                            let name = s.name.clone();
                            let host = s.host.clone();
                            let groups = s.groups.clone();
                            let has_mac = s.mac.is_some();
                            let server_id_wake = s.id.clone();
                            let server_id_shutdown = s.id.clone();
                            let server_id_reboot = s.id.clone();
                            let wake_action = ServerAction::<WakeServer>::new();
                            let shutdown_action = ServerAction::<ShutdownServer>::new();
                            let reboot_action = ServerAction::<RebootServer>::new();
                            view! {
                                <div class="bg-gray-800 border border-gray-700 p-4">
                                    <div class="flex items-center gap-2 mb-2">
                                        <IconServer class="w-4 h-4 text-gray-400"/>
                                        <span class="text-white font-medium">{name}</span>
                                    </div>
                                    <div class="space-y-1 text-xs mb-3">
                                        <div class="flex justify-between">
                                            <span class="text-gray-500">"Hôte"</span>
                                            <span class="font-mono text-gray-300">{host}</span>
                                        </div>
                                        {s.mac.as_ref().map(|mac| view! {
                                            <div class="flex justify-between">
                                                <span class="text-gray-500">"MAC"</span>
                                                <span class="font-mono text-gray-300">{mac.clone()}</span>
                                            </div>
                                        })}
                                    </div>
                                    {(!groups.is_empty()).then(|| view! {
                                        <div class="flex flex-wrap gap-1 mb-3">
                                            {groups.into_iter().map(|g| view! {
                                                <span class="px-1.5 py-0.5 text-xs bg-blue-500/20 text-blue-400 rounded">{g}</span>
                                            }).collect_view()}
                                        </div>
                                    })}
                                    <div class="flex gap-2">
                                        {has_mac.then(|| view! {
                                            <ActionForm action=wake_action>
                                                <input type="hidden" name="id" value=server_id_wake/>
                                                <button type="submit" class="px-3 py-1 text-xs bg-green-600 hover:bg-green-700 text-white">
                                                    "Wake"
                                                </button>
                                            </ActionForm>
                                        })}
                                        <ActionForm action=shutdown_action>
                                            <input type="hidden" name="id" value=server_id_shutdown/>
                                            <button type="submit" class="px-3 py-1 text-xs bg-red-600 hover:bg-red-700 text-white">
                                                "Shutdown"
                                            </button>
                                        </ActionForm>
                                        <ActionForm action=reboot_action>
                                            <input type="hidden" name="id" value=server_id_reboot/>
                                            <button type="submit" class="px-3 py-1 text-xs bg-yellow-600 hover:bg-yellow-700 text-white">
                                                "Reboot"
                                            </button>
                                        </ActionForm>
                                    </div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </Section>

        // Schedules
        <Section title="Planifications">
            <div class="flex justify-end mb-4">
                <a href="/wol?action=add-schedule" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">
                    "Ajouter"
                </a>
            </div>
            {if data.schedules.is_empty() {
                view! {
                    <div class="text-center py-8 text-gray-500">
                        <IconClock class="w-10 h-10 mx-auto mb-3 opacity-50"/>
                        <p>"Aucune planification"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Serveur"</th>
                                    <th class="pb-2 pr-4">"Action"</th>
                                    <th class="pb-2 pr-4">"Cron"</th>
                                    <th class="pb-2 pr-4">"Description"</th>
                                    <th class="pb-2 pr-4">"État"</th>
                                    <th class="pb-2">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.schedules.into_iter().map(|s| {
                                    let action_color = match s.action.as_str() {
                                        "wake" => "bg-green-500/20 text-green-400",
                                        "shutdown" => "bg-red-500/20 text-red-400",
                                        "reboot" => "bg-yellow-500/20 text-yellow-400",
                                        _ => "bg-gray-500/20 text-gray-400",
                                    };
                                    let toggle_label = if s.enabled { "Désactiver" } else { "Activer" };
                                    let state_class = if s.enabled {
                                        "bg-green-500/20 text-green-400"
                                    } else {
                                        "bg-gray-500/20 text-gray-500"
                                    };
                                    let toggle_action = ServerAction::<ToggleWolSchedule>::new();
                                    let exec_action = ServerAction::<ExecuteWolSchedule>::new();
                                    let schedule_id_toggle = s.id.clone();
                                    let schedule_id_exec = s.id.clone();
                                    let delete_href = format!("/wol?delete={}", s.id);
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-white">{s.server_name}</td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {action_color}")>
                                                    {s.action}
                                                </span>
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-300 text-xs">{s.cron}</td>
                                            <td class="py-2 pr-4 text-gray-400 text-xs">{s.description}</td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {state_class}")>
                                                    {if s.enabled { "Actif" } else { "Inactif" }}
                                                </span>
                                            </td>
                                            <td class="py-2">
                                                <div class="flex items-center gap-2">
                                                    <ActionForm action=toggle_action>
                                                        <input type="hidden" name="id" value=schedule_id_toggle/>
                                                        <button type="submit" class="text-xs text-blue-400 hover:text-blue-300">
                                                            {toggle_label}
                                                        </button>
                                                    </ActionForm>
                                                    <ActionForm action=exec_action>
                                                        <input type="hidden" name="id" value=schedule_id_exec/>
                                                        <button type="submit" class="text-xs text-green-400 hover:text-green-300">
                                                            "Exécuter"
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

        // Add schedule modal
        {(action.as_deref() == Some("add-schedule")).then(|| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                    <h3 class="text-lg font-medium text-white mb-4">"Ajouter une planification"</h3>
                    <ActionForm action=create_schedule_action>
                        <div class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Serveur"</label>
                                <select name="server_id" required
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500">
                                    {servers_for_modal.into_iter().map(|s| {
                                        view! { <option value=s.id>{s.name}</option> }
                                    }).collect_view()}
                                </select>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Action"</label>
                                <select name="action" required
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500">
                                    <option value="wake">"Wake"</option>
                                    <option value="shutdown">"Shutdown"</option>
                                    <option value="reboot">"Reboot"</option>
                                </select>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Expression Cron"</label>
                                <input type="text" name="cron" required
                                    placeholder="0 8 * * 1-5"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Description"</label>
                                <input type="text" name="description" value=""
                                    placeholder="Réveil matinal en semaine"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                        </div>
                        <div class="flex justify-end gap-3 mt-6">
                            <a href="/wol" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">"Créer"</button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        })}

        // Delete schedule confirmation modal
        {delete_id.map(|did| {
            let desc = delete_schedule_desc.unwrap_or_default();
            view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                    <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-md mx-4">
                        <h3 class="text-lg font-medium text-white mb-2">"Supprimer la planification"</h3>
                        <p class="text-sm text-gray-400 mb-6">
                            "Voulez-vous vraiment supprimer la planification "
                            <span class="text-white font-medium">{desc}</span>
                            " ?"
                        </p>
                        <ActionForm action=delete_schedule_action>
                            <input type="hidden" name="id" value=did/>
                            <div class="flex justify-end gap-3">
                                <a href="/wol" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                                <button type="submit" class="px-4 py-2 text-sm bg-red-600 hover:bg-red-700 text-white">"Supprimer"</button>
                            </div>
                        </ActionForm>
                    </div>
                </div>
            }
        })}
    }
}
