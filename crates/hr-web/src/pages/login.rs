use leptos::prelude::*;
use leptos_meta::Title;

use crate::islands::login_form::LoginForm;

/// Login page (public, no sidebar).
#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <Title text="Connexion — HomeRoute"/>
        <div class="min-h-screen flex items-center justify-center bg-gray-900 px-4">
            <div class="w-full max-w-sm">
                <div class="text-center mb-8">
                    <h1 class="text-3xl font-bold text-white">"HomeRoute"</h1>
                    <p class="mt-2 text-gray-400">"Connectez-vous pour continuer"</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-6">
                    <LoginForm/>
                </div>
            </div>
        </div>
    }
}
