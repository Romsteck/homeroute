use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::{FlashMessage, get_query_param};
use crate::server_fns::users::{CreateUser, DeleteUser};
use crate::types::UsersPageData;

fn users_icon() -> AnyView {
    view! { <IconUsers class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn UsersPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_users_data());

    view! {
        <PageHeader title="Utilisateurs" icon=users_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <UsersContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_users_data() -> Result<UsersPageData, ServerFnError> {
    crate::server_fns::users::get_users_data().await
}

#[component]
fn UsersContent(data: UsersPageData) -> impl IntoView {
    let action = get_query_param("action");
    let delete_username = get_query_param("delete");

    let create_action = ServerAction::<CreateUser>::new();
    let delete_action = ServerAction::<DeleteUser>::new();

    view! {
        // Users table
        <Section title="Utilisateurs">
            <div class="flex justify-end mb-4">
                <a
                    href="/users?action=add"
                    class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                >
                    <IconPlus class="w-4 h-4"/>
                    "Ajouter un utilisateur"
                </a>
            </div>
            {if data.users.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconUsers class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucun utilisateur configuré"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Utilisateur"</th>
                                    <th class="pb-2 pr-4">"Nom affiché"</th>
                                    <th class="pb-2 pr-4">"Email"</th>
                                    <th class="pb-2 pr-4">"Groupes"</th>
                                    <th class="pb-2 pr-4">"État"</th>
                                    <th class="pb-2">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.users.into_iter().map(|u| {
                                    let state_class = if u.disabled {
                                        "bg-red-500/20 text-red-400"
                                    } else {
                                        "bg-green-500/20 text-green-400"
                                    };
                                    let is_admin = u.groups.contains(&"admins".to_string());
                                    let username = u.username.clone();
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-white font-medium">{u.username}</td>
                                            <td class="py-2 pr-4 text-gray-300">{u.displayname}</td>
                                            <td class="py-2 pr-4 text-gray-400 text-xs">{u.email}</td>
                                            <td class="py-2 pr-4">
                                                <div class="flex flex-wrap gap-1">
                                                    {u.groups.into_iter().map(|g| {
                                                        let color = if g == "admins" {
                                                            "bg-amber-500/20 text-amber-400"
                                                        } else {
                                                            "bg-blue-500/20 text-blue-400"
                                                        };
                                                        view! {
                                                            <span class=format!("px-1.5 py-0.5 text-xs rounded {color}")>{g}</span>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </td>
                                            <td class="py-2 pr-4">
                                                <span class=format!("px-1.5 py-0.5 text-xs rounded {state_class}")>
                                                    {if u.disabled { "Désactivé" } else { "Actif" }}
                                                </span>
                                            </td>
                                            <td class="py-2">
                                                {(!is_admin).then(|| view! {
                                                    <a
                                                        href=format!("/users?delete={username}")
                                                        class="text-xs text-red-400 hover:text-red-300"
                                                    >
                                                        "Supprimer"
                                                    </a>
                                                })}
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

        // Groups
        <Section title="Groupes">
            {if data.groups.is_empty() {
                view! { <p class="text-gray-500">"Aucun groupe"</p> }.into_any()
            } else {
                view! {
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                        {data.groups.into_iter().map(|g| {
                            view! {
                                <div class="bg-gray-800 border border-gray-700 p-4">
                                    <div class="flex items-center justify-between mb-2">
                                        <span class="font-medium text-white">{g.name}</span>
                                        {g.built_in.then(|| view! {
                                            <span class="text-xs text-gray-500 bg-gray-600/50 px-2 py-0.5">"Système"</span>
                                        })}
                                    </div>
                                    <p class="text-sm text-gray-400">{format!("{} membre(s)", g.member_count)}</p>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </Section>

        // Add user modal
        {(action.as_deref() == Some("add")).then(|| view! {
            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-lg mx-4">
                    <div class="flex items-center justify-between mb-4">
                        <h3 class="text-lg font-medium text-white">"Ajouter un utilisateur"</h3>
                        <a href="/users" class="text-gray-400 hover:text-white">"X"</a>
                    </div>
                    <ActionForm action=create_action attr:class="space-y-4">
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Nom d'utilisateur"</label>
                            <input type="text" name="username" required
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="jean"/>
                        </div>
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Nom affiché"</label>
                            <input type="text" name="displayname" required
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="Jean Dupont"/>
                        </div>
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Mot de passe"</label>
                            <input type="password" name="password" required minlength="8"
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="Minimum 8 caractères"/>
                        </div>
                        <div>
                            <label class="block text-sm text-gray-300 mb-1">"Email"</label>
                            <input type="email" name="email"
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-white text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="jean@example.com"/>
                        </div>
                        <div class="flex justify-end gap-3 pt-2">
                            <a href="/users" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <button type="submit" class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors">
                                "Créer"
                            </button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        })}

        // Delete confirmation modal
        {delete_username.map(|uname| {
            let uname_display = uname.clone();
            view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
                    <div class="bg-gray-800 border border-gray-700 p-6 w-full max-w-sm mx-4">
                        <h3 class="text-lg font-medium text-white mb-4">"Confirmer la suppression"</h3>
                        <p class="text-sm text-gray-400 mb-6">
                            {format!("Supprimer l'utilisateur \"{}\" ?", uname_display)}
                        </p>
                        <div class="flex justify-end gap-3">
                            <a href="/users" class="px-4 py-2 text-sm text-gray-400 hover:text-white">"Annuler"</a>
                            <ActionForm action=delete_action attr:class="inline">
                                <input type="hidden" name="username" value=uname/>
                                <button type="submit" class="px-4 py-2 text-sm bg-red-600 hover:bg-red-700 text-white transition-colors">
                                    "Supprimer"
                                </button>
                            </ActionForm>
                        </div>
                    </div>
                </div>
            }
        })}
    }
}
