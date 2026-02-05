use leptos::prelude::*;

/// Minimal test page to validate SSR pipeline.
#[component]
pub fn TestPage() -> impl IntoView {
    view! {
        <main class="flex items-center justify-center min-h-screen">
            <div class="text-center">
                <h1 class="text-4xl font-bold text-white mb-4">"HomeRoute — Leptos SSR"</h1>
                <p class="text-gray-400">"Si vous voyez cette page, le SSR fonctionne."</p>
            </div>
        </main>
    }
}
