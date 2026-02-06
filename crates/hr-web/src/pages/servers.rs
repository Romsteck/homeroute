use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::servers::{AddServer, DeleteServer, TestServerConnection, UpdateServer};
use crate::types::ServersData;

fn servers_icon() -> AnyView {
    view! { <IconServer class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn ServersPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_servers_data());

    view! {
        <PageHeader title="Serveurs" icon=servers_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <ServersContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_servers_data() -> Result<ServersData, ServerFnError> {
    crate::server_fns::servers::get_servers_data().await
}

#[component]
fn ServersContent(data: ServersData) -> impl IntoView {
    let action = get_query_param("action");
    let edit_id = get_query_param("edit");
    let delete_id = get_query_param("delete");

    let add_action = ServerAction::<AddServer>::new();
    let update_action = ServerAction::<UpdateServer>::new();
    let delete_action = ServerAction::<DeleteServer>::new();

    // Find server for edit modal
    let edit_server = edit_id.as_ref().and_then(|eid| {
        data.servers.iter().find(|s| s.id == *eid).cloned()
    });

    // Find server for delete modal
    let delete_server_name = delete_id.as_ref().and_then(|did| {
        data.servers.iter().find(|s| s.id == *did).map(|s| s.name.clone())
    });

    let count = data.servers.len();

    view! {
        <Section title="Serveurs">
            <div class="flex items-center justify-between mb-4">
                <p class="text-sm text-gray-400">{format!("{count} serveur(s) configuré(s)")}</p>
                <a href="/servers?action=add" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">
                    "Ajouter un serveur"
                </a>
            </div>
            {if data.servers.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconServer class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucun serveur configuré"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Nom"</th>
                                    <th class="pb-2 pr-4">"Hôte"</th>
                                    <th class="pb-2 pr-4">"Port"</th>
                                    <th class="pb-2 pr-4">"Utilisateur"</th>
                                    <th class="pb-2 pr-4">"MAC"</th>
                                    <th class="pb-2 pr-4">"Groupes"</th>
                                    <th class="pb-2">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.servers.into_iter().map(|s| {
                                    let test_action = ServerAction::<TestServerConnection>::new();
                                    let server_id = s.id.clone();
                                    let edit_href = format!("/servers?edit={}", s.id);
                                    let delete_href = format!("/servers?delete={}", s.id);
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-white font-medium">{s.name}</td>
                                            <td class="py-2 pr-4 font-mono text-blue-400 text-xs">{s.host}</td>
                                            <td class="py-2 pr-4 text-gray-300">{s.port}</td>
                                            <td class="py-2 pr-4 text-gray-300">{s.username}</td>
                                            <td class="py-2 pr-4 font-mono text-gray-500 text-xs">
                                                {s.mac.unwrap_or_else(|| "-".into())}
                                            </td>
                                            <td class="py-2 pr-4">
                                                <div class="flex flex-wrap gap-1">
                                                    {s.groups.into_iter().map(|g| view! {
                                                        <span class="px-1.5 py-0.5 text-xs bg-blue-500/20 text-blue-400 rounded">{g}</span>
                                                    }).collect_view()}
                                                </div>
                                            </td>
                                            <td class="py-2">
                                                <div class="flex items-center gap-2">
                                                    <a href=edit_href class="text-xs text-blue-400 hover:text-blue-300">"Modifier"</a>
                                                    <ActionForm action=test_action>
                                                        <input type="hidden" name="id" value=server_id/>
                                                        <button type="submit" class="text-xs text-green-400 hover:text-green-300">"Tester"</button>
                                                    </ActionForm>
                                                    <a href=delete_href class="text-xs text-red-400 hover:text-red-300">"Supprimer"</a>
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

        // Add server modal
        {(action.as_deref() == Some("add")).then(|| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                    <h3 class="text-lg font-medium text-white mb-4">"Ajouter un serveur"</h3>
                    <ActionForm action=add_action>
                        <div class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Nom"</label>
                                <input type="text" name="name" required
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div class="grid grid-cols-3 gap-4">
                                <div class="col-span-2">
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Hôte"</label>
                                    <input type="text" name="host" required
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Port"</label>
                                    <input type="number" name="port" value="22" min="1" max="65535"
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Utilisateur"</label>
                                <input type="text" name="username" value="root"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Adresse MAC (optionnel)"</label>
                                <input type="text" name="mac" value=""
                                    placeholder="AA:BB:CC:DD:EE:FF"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Groupes (séparés par des virgules)"</label>
                                <input type="text" name="groups" value=""
                                    placeholder="web, database"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                            </div>
                        </div>
                        <div class="flex justify-end gap-3 mt-6">
                            <a href="/servers" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">"Créer"</button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        })}

        // Edit server modal
        {edit_server.map(|s| {
            let groups_str = s.groups.join(", ");
            let mac_str = s.mac.unwrap_or_default();
            view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                    <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                        <h3 class="text-lg font-medium text-white mb-4">"Modifier le serveur"</h3>
                        <ActionForm action=update_action>
                            <input type="hidden" name="id" value=s.id/>
                            <div class="space-y-4">
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Nom"</label>
                                    <input type="text" name="name" required value=s.name
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                                <div class="grid grid-cols-3 gap-4">
                                    <div class="col-span-2">
                                        <label class="block text-sm font-medium text-gray-300 mb-1">"Hôte"</label>
                                        <input type="text" name="host" required value=s.host
                                            class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                    </div>
                                    <div>
                                        <label class="block text-sm font-medium text-gray-300 mb-1">"Port"</label>
                                        <input type="number" name="port" value=s.port min="1" max="65535"
                                            class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                    </div>
                                </div>
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Utilisateur"</label>
                                    <input type="text" name="username" value=s.username
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Adresse MAC (optionnel)"</label>
                                    <input type="text" name="mac" value=mac_str
                                        placeholder="AA:BB:CC:DD:EE:FF"
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                                <div>
                                    <label class="block text-sm font-medium text-gray-300 mb-1">"Groupes (séparés par des virgules)"</label>
                                    <input type="text" name="groups" value=groups_str
                                        placeholder="web, database"
                                        class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"/>
                                </div>
                            </div>
                            <div class="flex justify-end gap-3 mt-6">
                                <a href="/servers" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                                <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white">"Enregistrer"</button>
                            </div>
                        </ActionForm>
                    </div>
                </div>
            }
        })}

        // Delete confirmation modal
        {delete_id.map(|did| {
            let name = delete_server_name.unwrap_or_default();
            view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                    <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-md mx-4">
                        <h3 class="text-lg font-medium text-white mb-2">"Supprimer le serveur"</h3>
                        <p class="text-sm text-gray-400 mb-6">
                            "Voulez-vous vraiment supprimer le serveur "
                            <span class="text-white font-medium">{name}</span>
                            " ?"
                        </p>
                        <ActionForm action=delete_action>
                            <input type="hidden" name="id" value=did/>
                            <div class="flex justify-end gap-3">
                                <a href="/servers" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                                <button type="submit" class="px-4 py-2 text-sm bg-red-600 hover:bg-red-700 text-white">"Supprimer"</button>
                            </div>
                        </ActionForm>
                    </div>
                </div>
            }
        })}
    }
}
