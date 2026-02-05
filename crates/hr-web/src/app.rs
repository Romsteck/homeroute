use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{
    components::{ParentRoute, Route, Router, Routes},
    path,
};

use crate::components::layout::ProtectedLayout;
use crate::pages;

/// HTML shell wrapping all pages (rendered server-side).
/// This is a plain function, NOT a #[component].
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="fr" class="dark">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <link rel="icon" href="/favicon.svg" type="image/svg+xml"/>
                <link rel="stylesheet" href="/pkg/style.css"/>
                <HydrationScripts options islands=true/>
                <MetaTags/>
            </head>
            <body class="bg-gray-900 text-gray-100 min-h-screen">
                <App/>
            </body>
        </html>
    }
}

/// Main application component with router.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Router>
            <Routes fallback=|| view! { <pages::not_found::NotFound/> }>
                <Route path=path!("/login") view=pages::login::LoginPage/>
                <Route path=path!("/leptos-test") view=pages::test::TestPage/>
                <ParentRoute path=path!("/") view=ProtectedLayout>
                    <Route path=path!("") view=Placeholder/>
                    <Route path=path!("dns") view=Placeholder/>
                    <Route path=path!("network") view=Placeholder/>
                    <Route path=path!("firewall") view=Placeholder/>
                    <Route path=path!("adblock") view=Placeholder/>
                    <Route path=path!("ddns") view=Placeholder/>
                    <Route path=path!("reverseproxy") view=Placeholder/>
                    <Route path=path!("applications") view=Placeholder/>
                    <Route path=path!("certificates") view=Placeholder/>
                    <Route path=path!("servers") view=Placeholder/>
                    <Route path=path!("wol") view=Placeholder/>
                    <Route path=path!("traffic") view=Placeholder/>
                    <Route path=path!("users") view=Placeholder/>
                    <Route path=path!("updates") view=Placeholder/>
                    <Route path=path!("energy") view=Placeholder/>
                    <Route path=path!("settings") view=Placeholder/>
                    <Route path=path!("profile") view=Placeholder/>
                </ParentRoute>
            </Routes>
        </Router>
    }
}

/// Temporary placeholder for pages not yet migrated.
#[component]
fn Placeholder() -> impl IntoView {
    view! {
        <div class="p-8">
            <p class="text-gray-400">"Page en cours de migration..."</p>
        </div>
    }
}
