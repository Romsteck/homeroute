use leptos::prelude::*;
use leptos_router::components::Outlet;

use crate::components::icons::*;
use crate::islands::logout_button::LogoutButton;
use crate::server_fns::auth::get_current_user;
use crate::types::WebUserInfo;

/// Protected layout that checks auth then renders Sidebar + content.
/// Used as the view for the root `ParentRoute`.
#[component]
pub fn ProtectedLayout() -> impl IntoView {
    let user = Resource::new(|| (), |_| get_current_user());

    view! {
        <Suspense>
            {move || Suspend::new(async move {
                match user.await {
                    Ok(Some(info)) => view! {
                        <div class="flex min-h-screen">
                            <Sidebar user_info=info/>
                            <main class="flex-1 overflow-auto">
                                <Outlet/>
                            </main>
                        </div>
                    }.into_any(),
                    _ => {
                        #[cfg(feature = "ssr")]
                        leptos_axum::redirect("/login");
                        view! { <p class="p-8">"Redirection..."</p> }.into_any()
                    }
                }
            })}
        </Suspense>
    }
}

/// Determine the current request path (SSR only).
fn current_path() -> String {
    #[cfg(feature = "ssr")]
    {
        use_context::<axum::http::request::Parts>()
            .map(|p| p.uri.path().to_string())
            .unwrap_or_default()
    }
    #[cfg(not(feature = "ssr"))]
    {
        String::new()
    }
}

/// Returns CSS class for a nav link based on active state.
fn nav_class(href: &str, current: &str) -> &'static str {
    let active = if href == "/" {
        current == "/"
    } else {
        current.starts_with(href)
    };
    if active {
        "flex items-center gap-3 px-4 py-2 text-sm border-l-[3px] border-blue-400 bg-gray-700/50 text-white"
    } else {
        "flex items-center gap-3 px-4 py-2 text-sm text-gray-400 hover:bg-gray-700/30 hover:text-white border-l-[3px] border-transparent"
    }
}

/// Sidebar navigation (SSR-only, except LogoutButton island).
#[component]
fn Sidebar(user_info: WebUserInfo) -> impl IntoView {
    let path = current_path();

    view! {
        <aside class="w-64 bg-gray-800 border-r border-gray-700 flex flex-col min-h-screen shrink-0">
            // Header
            <div class="flex items-center justify-between px-4 py-4 border-b border-gray-700">
                <div>
                    <h1 class="text-lg font-bold text-white">"HomeRoute"</h1>
                    <p class="text-xs text-gray-400">"cloudmaster"</p>
                </div>
                <a href="/settings" class="text-gray-400 hover:text-white">
                    <IconSettings class="w-5 h-5"/>
                </a>
            </div>

            // Navigation
            <nav class="flex-1 py-4 overflow-y-auto">
                // Main
                <div class="mb-2">
                    <a href="/" class=nav_class("/", &path)>
                        <IconDashboard class="w-5 h-5"/><span>"Dashboard"</span>
                    </a>
                </div>

                // Réseau
                <div class="mb-2">
                    <p class="px-4 py-1 text-xs font-semibold text-gray-500 uppercase tracking-wider">"Réseau"</p>
                    <a href="/dns" class=nav_class("/dns", &path)>
                        <IconServer class="w-5 h-5"/><span>"DNS / DHCP"</span>
                    </a>
                    <a href="/network" class=nav_class("/network", &path)>
                        <IconNetwork class="w-5 h-5"/><span>"Réseau"</span>
                    </a>
                    <a href="/firewall" class=nav_class("/firewall", &path)>
                        <IconShieldCheck class="w-5 h-5"/><span>"Firewall IPv6"</span>
                    </a>
                    <a href="/adblock" class=nav_class("/adblock", &path)>
                        <IconShield class="w-5 h-5"/><span>"AdBlock"</span>
                    </a>
                    <a href="/ddns" class=nav_class("/ddns", &path)>
                        <IconGlobe class="w-5 h-5"/><span>"Dynamic DNS"</span>
                    </a>
                    <a href="/reverseproxy" class=nav_class("/reverseproxy", &path)>
                        <IconArrowLeftRight class="w-5 h-5"/><span>"Reverse Proxy"</span>
                    </a>
                    <a href="/applications" class=nav_class("/applications", &path)>
                        <IconBoxes class="w-5 h-5"/><span>"Applications"</span>
                    </a>
                    <a href="/certificates" class=nav_class("/certificates", &path)>
                        <IconLock class="w-5 h-5"/><span>"Certificats"</span>
                    </a>
                </div>

                // Système
                <div class="mb-2">
                    <p class="px-4 py-1 text-xs font-semibold text-gray-500 uppercase tracking-wider">"Système"</p>
                    <a href="/servers" class=nav_class("/servers", &path)>
                        <IconHardDrive class="w-5 h-5"/><span>"Serveurs"</span>
                    </a>
                    <a href="/wol" class=nav_class("/wol", &path)>
                        <IconPower class="w-5 h-5"/><span>"Wake-on-LAN"</span>
                    </a>
                    <a href="/traffic" class=nav_class("/traffic", &path)>
                        <IconBarChart class="w-5 h-5"/><span>"Trafic"</span>
                    </a>
                    <a href="/users" class=nav_class("/users", &path)>
                        <IconUsers class="w-5 h-5"/><span>"Utilisateurs"</span>
                    </a>
                    <a href="/updates" class=nav_class("/updates", &path)>
                        <IconRefreshCw class="w-5 h-5"/><span>"Mises à jour"</span>
                    </a>
                    <a href="/energy" class=nav_class("/energy", &path)>
                        <IconZap class="w-5 h-5"/><span>"Énergie"</span>
                    </a>
                </div>
            </nav>

            // Footer: user info + logout
            <div class="border-t border-gray-700 px-4 py-3">
                <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2 min-w-0">
                        <IconUser class="w-5 h-5 text-gray-400 shrink-0"/>
                        <div class="min-w-0">
                            <p class="text-sm font-medium text-white truncate">
                                {user_info.display_name.clone()}
                            </p>
                            {if user_info.is_admin {
                                Some(view! { <span class="text-xs text-blue-400">"Admin"</span> })
                            } else {
                                None
                            }}
                        </div>
                    </div>
                    <LogoutButton/>
                </div>
            </div>
        </aside>
    }
}
