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
pub fn shell(_options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="fr" class="dark">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <link rel="icon" href="/favicon.svg" type="image/svg+xml"/>
                <link rel="stylesheet" href="/pkg/style.css"/>
                <MetaTags/>
            </head>
            <body class="bg-gray-900 text-gray-100 min-h-screen">
                <App/>
                <script src="/scripts/ws.js" defer></script>
                <script>
                    "document.querySelectorAll('form').forEach(f=>f.addEventListener('submit',function(){var b=f.querySelector('[type=submit]');if(b)b.disabled=true;}));"
                </script>
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
                    <Route path=path!("") view=pages::dashboard::DashboardPage/>
                    <Route path=path!("dns") view=pages::dns_dhcp::DnsDhcpPage/>
                    <Route path=path!("network") view=pages::network::NetworkPage/>
                    <Route path=path!("firewall") view=pages::firewall::FirewallPage/>
                    <Route path=path!("adblock") view=pages::adblock::AdblockPage/>
                    <Route path=path!("ddns") view=pages::ddns::DdnsPage/>
                    <Route path=path!("reverseproxy") view=pages::reverseproxy::ReverseProxyPage/>
                    <Route path=path!("applications") view=pages::applications::ApplicationsPage/>
                    <Route path=path!("certificates") view=pages::certificates::CertificatesPage/>
                    <Route path=path!("servers") view=pages::servers::ServersPage/>
                    <Route path=path!("wol") view=pages::wol::WolPage/>
                    <Route path=path!("traffic") view=pages::traffic::TrafficPage/>
                    <Route path=path!("users") view=pages::users::UsersPage/>
                    <Route path=path!("updates") view=pages::updates::UpdatesPage/>
                    <Route path=path!("energy") view=pages::energy::EnergyPage/>
                    <Route path=path!("settings") view=pages::settings::SettingsPage/>
                    <Route path=path!("profile") view=pages::profile::ProfilePage/>
                </ParentRoute>
            </Routes>
        </Router>
    }
}
