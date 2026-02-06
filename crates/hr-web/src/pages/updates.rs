use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::updates::{CheckForUpdates, RunAptUpgrade};
use crate::types::UpdatesPageData;

fn updates_icon() -> AnyView {
    view! { <IconHardDrive class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn UpdatesPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_updates_data());

    view! {
        <PageHeader title="Mises à jour système" icon=updates_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <UpdatesContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_updates_data() -> Result<UpdatesPageData, ServerFnError> {
    crate::server_fns::updates::get_updates_data().await
}

#[component]
fn UpdatesContent(data: UpdatesPageData) -> impl IntoView {
    let total = data.apt_packages.len() + data.snap_packages.len();
    let has_apt = !data.apt_packages.is_empty();
    let has_services_to_restart = !data.services_to_restart.is_empty();
    let services_count = data.services_to_restart.len();

    let check_action = ServerAction::<CheckForUpdates>::new();
    let upgrade_action = ServerAction::<RunAptUpgrade>::new();

    view! {
        // Action buttons
        <div class="flex gap-3 mb-6">
            <ActionForm action=check_action attr:class="inline">
                <button
                    type="submit"
                    class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                >
                    <IconRefreshCw class="w-4 h-4"/>
                    "Vérifier les mises à jour"
                </button>
            </ActionForm>
            {has_apt.then(|| view! {
                <ActionForm action=upgrade_action attr:class="inline">
                    <button
                        type="submit"
                        class="px-4 py-2 text-sm bg-green-600 hover:bg-green-700 text-white transition-colors flex items-center gap-2"
                    >
                        <IconHardDrive class="w-4 h-4"/>
                        "Mettre à jour (apt)"
                    </button>
                </ActionForm>
            })}
        </div>

        // Summary cards
        <Section title="Résumé">
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Mises à jour"</p>
                    <p class="text-2xl font-bold text-blue-400">{total}</p>
                    <p class="text-xs text-gray-500">"paquets disponibles"</p>
                    {data.last_check.as_ref().map(|d| view! {
                        <p class="text-xs text-gray-600 mt-1">{format!("Dernière vérification: {d}")}</p>
                    })}
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Sécurité"</p>
                    <p class=format!("text-2xl font-bold {}", if data.security_count > 0 { "text-red-400" } else { "text-green-400" })>
                        {data.security_count}
                    </p>
                    <p class="text-xs text-gray-500">
                        {if data.security_count > 0 { "mises à jour critiques" } else { "système à jour" }}
                    </p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Services"</p>
                    {if data.kernel_reboot_needed {
                        view! {
                            <div class="flex items-center gap-2 text-red-400">
                                <IconAlertTriangle class="w-6 h-6"/>
                                <span class="text-sm font-medium">"Redémarrage requis"</span>
                            </div>
                        }.into_any()
                    } else if has_services_to_restart {
                        view! {
                            <p class="text-2xl font-bold text-yellow-400">{services_count}</p>
                            <p class="text-xs text-gray-500">"services à redémarrer"</p>
                        }.into_any()
                    } else {
                        view! {
                            <div class="flex items-center gap-2 text-green-400">
                                <IconCheckCircle class="w-6 h-6"/>
                                <span class="text-sm">"Aucun redémarrage requis"</span>
                            </div>
                        }.into_any()
                    }}
                </div>
            </div>
        </Section>

        // APT packages
        {(!data.apt_packages.is_empty()).then(|| view! {
            <Section title=format!("Paquets APT ({})", data.apt_packages.len())>
                <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                <th class="pb-2 pr-4">"Paquet"</th>
                                <th class="pb-2 pr-4">"Version actuelle"</th>
                                <th class="pb-2 pr-4">"Nouvelle version"</th>
                                <th class="pb-2">"Type"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {data.apt_packages.into_iter().map(|p| {
                                let type_class = if p.is_security {
                                    "bg-red-500/20 text-red-400"
                                } else {
                                    "bg-green-500/20 text-green-400"
                                };
                                view! {
                                    <tr class="border-t border-gray-700/50">
                                        <td class="py-2 pr-4 font-mono text-white">{p.name}</td>
                                        <td class="py-2 pr-4 font-mono text-gray-400 text-xs">{p.current_version}</td>
                                        <td class="py-2 pr-4 font-mono text-blue-400 text-xs">{p.new_version}</td>
                                        <td class="py-2">
                                            <span class=format!("px-1.5 py-0.5 text-xs rounded {type_class}")>
                                                {if p.is_security { "Sécurité" } else { "Normal" }}
                                            </span>
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                </div>
            </Section>
        })}

        // Snap packages
        {(!data.snap_packages.is_empty()).then(|| view! {
            <Section title=format!("Snaps ({})", data.snap_packages.len())>
                <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                <th class="pb-2 pr-4">"Snap"</th>
                                <th class="pb-2 pr-4">"Nouvelle version"</th>
                                <th class="pb-2 pr-4">"Révision"</th>
                                <th class="pb-2">"Éditeur"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {data.snap_packages.into_iter().map(|s| {
                                view! {
                                    <tr class="border-t border-gray-700/50">
                                        <td class="py-2 pr-4 font-mono text-white">{s.name}</td>
                                        <td class="py-2 pr-4 font-mono text-blue-400 text-xs">{s.new_version}</td>
                                        <td class="py-2 pr-4 text-gray-400">{s.revision}</td>
                                        <td class="py-2 text-gray-400">{s.publisher}</td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                </div>
            </Section>
        })}

        // Services to restart
        {has_services_to_restart.then(|| view! {
            <Section title="Services à redémarrer">
                <div class="space-y-2">
                    {data.services_to_restart.into_iter().map(|s| {
                        view! {
                            <div class="flex items-center gap-2 p-2 bg-gray-800 border border-gray-700">
                                <IconServer class="w-4 h-4 text-yellow-400"/>
                                <span class="font-mono text-sm text-gray-300">{s}</span>
                            </div>
                        }
                    }).collect_view()}
                </div>
            </Section>
        })}

        // Empty state
        {(total == 0 && !data.kernel_reboot_needed && !has_services_to_restart).then(|| view! {
            <Section>
                <div class="text-center py-12 text-gray-500">
                    <IconRefreshCw class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                    <p>"Aucune donnée disponible"</p>
                    <p class="text-sm mt-2">"Cliquez sur \"Vérifier les mises à jour\" pour lancer une vérification"</p>
                </div>
            </Section>
        })}
    }
}
