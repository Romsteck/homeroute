use leptos::prelude::*;

use crate::components::icons::{IconEye, IconEyeOff, IconLoader, IconLock};
use crate::server_fns::auth::Login;

/// Interactive login form (island — hydrated on client).
#[island]
pub fn LoginForm() -> impl IntoView {
    let login_action = ServerAction::<Login>::new();

    let (show_password, set_show_password) = signal(false);

    let error_msg = move || {
        login_action.value().get().and_then(|r| {
            r.err().map(|e| {
                let s = e.to_string();
                // Strip the "error running server function: " prefix
                s.strip_prefix("error running server function: ")
                    .unwrap_or(&s)
                    .to_string()
            })
        })
    };

    let is_pending = login_action.pending();

    view! {
        <ActionForm action=login_action attr:class="space-y-5">
            // Error banner
            {move || error_msg().map(|msg| view! {
                <div class="bg-red-500/20 border border-red-500/50 text-red-300 px-4 py-3 text-sm">
                    {msg}
                </div>
            })}

            // Username
            <div>
                <label for="username" class="block text-sm font-medium text-gray-300 mb-1">
                    "Nom d'utilisateur"
                </label>
                <input
                    id="username"
                    name="username"
                    type="text"
                    autocomplete="username"
                    required
                    autofocus
                    class="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                    placeholder="admin"
                />
            </div>

            // Password
            <div>
                <label for="password" class="block text-sm font-medium text-gray-300 mb-1">
                    "Mot de passe"
                </label>
                <div class="relative">
                    <input
                        id="password"
                        name="password"
                        type=move || if show_password.get() { "text" } else { "password" }
                        autocomplete="current-password"
                        required
                        class="w-full px-3 py-2 pr-10 bg-gray-700 border border-gray-600 text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                    />
                    <button
                        type="button"
                        class="absolute inset-y-0 right-0 flex items-center pr-3 text-gray-400 hover:text-white"
                        on:click=move |_| set_show_password.update(|v| *v = !*v)
                    >
                        {move || if show_password.get() {
                            view! { <IconEyeOff class="w-4 h-4"/> }.into_any()
                        } else {
                            view! { <IconEye class="w-4 h-4"/> }.into_any()
                        }}
                    </button>
                </div>
            </div>

            // Remember me
            <div class="flex items-center gap-2">
                <input
                    id="remember_me"
                    name="remember_me"
                    type="checkbox"
                    value="on"
                    class="w-4 h-4 bg-gray-700 border-gray-600 text-blue-500 focus:ring-blue-500"
                />
                <label for="remember_me" class="text-sm text-gray-300">
                    "Se souvenir de moi"
                </label>
            </div>

            // Submit
            <button
                type="submit"
                disabled=move || is_pending.get()
                class="w-full px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
                {move || if is_pending.get() {
                    view! { <IconLoader class="w-4 h-4"/> }.into_any()
                } else {
                    view! { <IconLock class="w-4 h-4"/> }.into_any()
                }}
                "Connexion"
            </button>
        </ActionForm>
    }
}
