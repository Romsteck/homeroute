use leptos::prelude::*;

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <main class="flex items-center justify-center min-h-screen">
            <div class="text-center">
                <h1 class="text-6xl font-bold text-gray-500 mb-4">"404"</h1>
                <p class="text-gray-400">"Page introuvable"</p>
            </div>
        </main>
    }
}
