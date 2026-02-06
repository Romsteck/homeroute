use leptos::prelude::*;
use leptos_meta::Title;

use crate::components::toast::get_query_param;
use crate::server_fns::auth::Login;

/// Login page (public, no sidebar).
#[component]
pub fn LoginPage() -> impl IntoView {
    let error_msg = get_query_param("error");
    let login_action = ServerAction::<Login>::new();

    view! {
        <Title text="Connexion — HomeRoute"/>
        <div class="min-h-screen flex items-center justify-center bg-gray-900 px-4">
            <div class="w-full max-w-sm">
                <div class="text-center mb-8">
                    <h1 class="text-3xl font-bold text-white">"HomeRoute"</h1>
                    <p class="mt-2 text-gray-400">"Connectez-vous pour continuer"</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-6">
                    {error_msg.map(|msg| view! {
                        <div class="mb-4 px-4 py-3 border text-sm bg-red-500/20 border-red-500/50 text-red-300">
                            {msg}
                        </div>
                    })}
                    <ActionForm action=login_action>
                        <div class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Nom d'utilisateur"</label>
                                <input
                                    type="text"
                                    name="username"
                                    required
                                    autocomplete="username"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                                    placeholder="admin"
                                />
                            </div>
                            <div>
                                <label class="block text-sm font-medium text-gray-300 mb-1">"Mot de passe"</label>
                                <input
                                    type="password"
                                    name="password"
                                    required
                                    autocomplete="current-password"
                                    class="w-full px-3 py-2 bg-gray-900 border border-gray-700 text-white placeholder-gray-500 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                                    placeholder="••••••••"
                                />
                            </div>
                            <div class="flex items-center gap-2">
                                <input
                                    type="checkbox"
                                    name="remember_me"
                                    value="on"
                                    id="remember"
                                    class="rounded border-gray-600 bg-gray-900 text-blue-600"
                                />
                                <label for="remember" class="text-sm text-gray-400">"Se souvenir de moi"</label>
                            </div>
                            <button
                                type="submit"
                                class="w-full py-2 px-4 bg-blue-600 hover:bg-blue-700 text-white font-medium text-sm transition-colors"
                            >
                                "Connexion"
                            </button>
                        </div>
                    </ActionForm>
                </div>
            </div>
        </div>
    }
}
