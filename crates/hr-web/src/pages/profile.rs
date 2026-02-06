use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::profile::{ChangePassword, get_profile_data};
use crate::types::ProfileData;

fn profile_icon() -> AnyView {
    view! { <IconUser class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn ProfilePage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_profile_data());

    view! {
        <PageHeader title="Mon compte" icon=profile_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <ProfileContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

#[component]
fn ProfileContent(data: ProfileData) -> impl IntoView {
    let change_pwd_action = ServerAction::<ChangePassword>::new();

    view! {
        <Section title="Informations">
            <div class="bg-gray-800 border border-gray-700 p-6 max-w-2xl">
                <div class="flex items-center gap-4 mb-6">
                    <div class="w-14 h-14 rounded-full bg-blue-600 flex items-center justify-center text-xl font-bold text-white">
                        {data.display_name.chars().next().unwrap_or('?').to_uppercase().to_string()}
                    </div>
                    <div>
                        <h2 class="text-lg font-semibold text-white">{data.display_name.clone()}</h2>
                        <p class="text-sm text-gray-400">{format!("@{}", data.username)}</p>
                    </div>
                    {data.is_admin.then(|| view! {
                        <span class="ml-auto px-2.5 py-0.5 text-xs font-medium bg-amber-500/20 text-amber-400 rounded">
                            "Admin"
                        </span>
                    })}
                </div>

                <div class="space-y-3">
                    <InfoRow label="Nom d'utilisateur" value=data.username.clone()/>
                    <InfoRow label="Nom d'affichage" value=data.display_name.clone()/>
                    <InfoRow
                        label="Email"
                        value=if data.email.is_empty() { "Non renseigné".to_string() } else { data.email.clone() }
                    />
                </div>
            </div>
        </Section>

        // Password change (SSR ActionForm)
        <Section title="Sécurité">
            <div class="bg-gray-800 border border-gray-700 p-6 max-w-md">
                <h3 class="text-sm font-medium text-gray-300 mb-4">"Changer le mot de passe"</h3>
                <ActionForm action=change_pwd_action>
                    <div class="space-y-3">
                        <div>
                            <label class="block text-sm text-gray-400 mb-1">"Mot de passe actuel"</label>
                            <input
                                type="password"
                                name="current_password"
                                required
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                        </div>
                        <div>
                            <label class="block text-sm text-gray-400 mb-1">"Nouveau mot de passe"</label>
                            <input
                                type="password"
                                name="new_password"
                                required
                                minlength="6"
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                        </div>
                        <div>
                            <label class="block text-sm text-gray-400 mb-1">"Confirmer le mot de passe"</label>
                            <input
                                type="password"
                                name="confirm_password"
                                required
                                minlength="6"
                                class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                        </div>
                        <button
                            type="submit"
                            class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors"
                        >
                            "Changer le mot de passe"
                        </button>
                    </div>
                </ActionForm>
            </div>
        </Section>

        <Section title="Groupes">
            <div class="flex flex-wrap gap-2">
                {data.groups.into_iter().map(|g| {
                    let color = if g == "admins" {
                        "bg-amber-500/20 text-amber-400"
                    } else {
                        "bg-blue-500/20 text-blue-400"
                    };
                    view! {
                        <span class=format!("px-3 py-1 text-sm font-medium rounded {color}")>
                            {g}
                        </span>
                    }
                }).collect_view()}
            </div>
        </Section>
    }
}

#[component]
fn InfoRow(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between py-2 border-b border-gray-700/50">
            <span class="text-sm text-gray-400">{label}</span>
            <span class="text-sm text-white">{value}</span>
        </div>
    }
}
