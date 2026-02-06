use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::types::{DashboardData, LeaseInfo, ServiceInfo};
use crate::utils::{format_expiry, format_number};

fn dashboard_icon() -> AnyView {
    view! { <IconDashboard class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn DashboardPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_dashboard_data());

    view! {
        <PageHeader title="Dashboard" icon=dashboard_icon/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <DashboardContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

/// Use the server function type to call the function
async fn get_dashboard_data() -> Result<DashboardData, ServerFnError> {
    crate::server_fns::dashboard::get_dashboard_data().await
}

// ── Icon helpers (must be fn pointers, not closures, for view! macro) ──

fn icon_network() -> AnyView {
    view! { <IconNetwork class="w-5 h-5 text-blue-400"/> }.into_any()
}
fn icon_wifi() -> AnyView {
    view! { <IconWifi class="w-5 h-5 text-emerald-400"/> }.into_any()
}
fn icon_shield() -> AnyView {
    view! { <IconShield class="w-5 h-5 text-purple-400"/> }.into_any()
}
fn icon_globe() -> AnyView {
    view! { <IconGlobe class="w-5 h-5 text-cyan-400"/> }.into_any()
}

// ── Main dashboard content (rendered after data loads) ───────────────

#[component]
fn DashboardContent(data: DashboardData) -> impl IntoView {
    let DashboardData {
        interfaces,
        leases,
        adblock,
        ddns,
        services,
    } = data;

    // Derived data
    let lease_count = leases.len();
    let top_leases: Vec<LeaseInfo> = leases.iter().take(3).cloned().collect();
    let recent_leases: Vec<LeaseInfo> = leases.into_iter().take(10).collect();

    let iface_count = interfaces.len();
    let up_count = interfaces.iter().filter(|i| i.state == "UP").count();

    let critical: Vec<ServiceInfo> = services
        .iter()
        .filter(|s| s.priority == "critical")
        .cloned()
        .collect();
    let important: Vec<ServiceInfo> = services
        .iter()
        .filter(|s| s.priority == "important")
        .cloned()
        .collect();
    let background: Vec<ServiceInfo> = services
        .iter()
        .filter(|s| s.priority == "background")
        .cloned()
        .collect();

    view! {
        // ── Vue d'ensemble ───────────────────────────────────────────
        <Section title="Vue d'ensemble">
            <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
                // Network interfaces
                <OverviewCard
                    href="/network"
                    icon=icon_network
                    title="Interfaces Réseau"
                >
                    <div class="space-y-1.5">
                        {interfaces.into_iter().map(|iface| {
                            let (dot_class, label) = if iface.state == "UP" {
                                ("bg-green-400", "UP")
                            } else {
                                ("bg-red-400", "DOWN")
                            };
                            view! {
                                <div class="flex items-center justify-between text-sm">
                                    <span class="text-gray-300 font-mono text-xs">{iface.name}</span>
                                    <span class="flex items-center gap-1.5">
                                        <span class=format!("w-1.5 h-1.5 rounded-full {dot_class}")></span>
                                        <span class="text-xs text-gray-400">{label}</span>
                                    </span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                    <p class="text-xs text-gray-500 mt-2">
                        {format!("{up_count}/{iface_count} actives")}
                    </p>
                </OverviewCard>

                // DHCP Leases
                <OverviewCard
                    href="/dns"
                    icon=icon_wifi
                    title="Baux DHCP"
                >
                    <p class="text-2xl font-bold text-white">{lease_count}</p>
                    <p class="text-xs text-gray-400 mb-2">"appareils connectés"</p>
                    <div class="space-y-1">
                        {top_leases.into_iter().map(|l| {
                            let label = l.hostname.unwrap_or_else(|| l.ip.clone());
                            view! {
                                <p class="text-xs text-gray-500 truncate">{label}</p>
                            }
                        }).collect_view()}
                    </div>
                </OverviewCard>

                // AdBlock
                <OverviewCard
                    href="/adblock"
                    icon=icon_shield
                    title="AdBlock"
                >
                    <p class="text-2xl font-bold text-white">{format_number(adblock.domain_count)}</p>
                    <p class="text-xs text-gray-400">"domaines bloqués"</p>
                    <p class="text-xs text-gray-500 mt-1">
                        {format!("{} sources actives", adblock.source_count)}
                    </p>
                </OverviewCard>

                // Dynamic DNS
                <OverviewCard
                    href="/ddns"
                    icon=icon_globe
                    title="Dynamic DNS"
                >
                    <p class="text-sm font-medium text-white truncate">
                        {ddns.record_name.unwrap_or_else(|| "Non configuré".into())}
                    </p>
                    <p class="text-xs text-gray-400 font-mono mt-1 truncate">
                        {ddns.current_ipv6.unwrap_or_else(|| "Pas d'IPv6".into())}
                    </p>
                </OverviewCard>
            </div>
        </Section>

        // ── Services ─────────────────────────────────────────────────
        <Section title="Services" contrast=true>
            <div class="space-y-4">
                {(!critical.is_empty()).then(|| view! {
                    <ServiceGroup label="Critique" services=critical/>
                })}
                {(!important.is_empty()).then(|| view! {
                    <ServiceGroup label="Important" services=important/>
                })}
                {(!background.is_empty()).then(|| view! {
                    <ServiceGroup label="Arrière-plan" services=background/>
                })}
            </div>
        </Section>

        // ── Baux DHCP Récents ────────────────────────────────────────
        <Section title="Baux DHCP Récents">
            <div class="overflow-x-auto">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                            <th class="pb-2 pr-4">"Hostname"</th>
                            <th class="pb-2 pr-4">"IP"</th>
                            <th class="pb-2 pr-4">"MAC"</th>
                            <th class="pb-2">"Expiration"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {recent_leases.into_iter().map(|lease| {
                            let expiry = format_expiry(lease.expiry);
                            view! {
                                <tr class="border-t border-gray-700/50">
                                    <td class="py-2 pr-4 text-gray-300">
                                        {lease.hostname.unwrap_or_else(|| "-".into())}
                                    </td>
                                    <td class="py-2 pr-4 font-mono text-blue-400 text-xs">
                                        {lease.ip}
                                    </td>
                                    <td class="py-2 pr-4 font-mono text-gray-500 text-xs">
                                        {lease.mac}
                                    </td>
                                    <td class="py-2 text-gray-400 text-xs">{expiry}</td>
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            </div>
        </Section>
    }
}

// ── Helper components ────────────────────────────────────────────────

#[component]
fn OverviewCard(
    href: &'static str,
    icon: fn() -> AnyView,
    title: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <a href=href class="block bg-gray-800 border border-gray-700 hover:border-gray-600 transition-colors">
            <div class="p-4">
                <div class="flex items-center justify-between mb-3">
                    <div class="flex items-center gap-2">
                        {icon()}
                        <h3 class="text-sm font-medium text-gray-300">{title}</h3>
                    </div>
                    <IconChevronRight class="w-4 h-4 text-gray-600"/>
                </div>
                {children()}
            </div>
        </a>
    }
}

#[component]
fn ServiceGroup(label: &'static str, services: Vec<ServiceInfo>) -> impl IntoView {
    view! {
        <div>
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">{label}</p>
            <div class="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-4 gap-2">
                {services.into_iter().map(|s| {
                    let (dot_color, _status_label) = match s.state.as_str() {
                        "running" => ("bg-green-400", "Actif"),
                        "failed" => ("bg-red-400", "Erreur"),
                        "starting" => ("bg-yellow-400", "Démarrage"),
                        "stopped" => ("bg-gray-500", "Arrêté"),
                        "disabled" => ("bg-gray-600", "Désactivé"),
                        _ => ("bg-gray-500", "Inconnu"),
                    };
                    let has_error = s.error.is_some();
                    let restart_badge = (s.restart_count > 0).then(|| {
                        let count = s.restart_count;
                        view! {
                            <span class="ml-1 px-1 py-0.5 text-xs bg-yellow-500/20 text-yellow-400 rounded">
                                {count}
                            </span>
                        }
                    });
                    view! {
                        <div
                            class="flex items-center gap-2 px-2 py-1.5 bg-gray-900/50 border border-gray-700/50 text-sm"
                            title=s.error.unwrap_or_default()
                        >
                            <span class=format!("w-2 h-2 rounded-full shrink-0 {dot_color}")></span>
                            <span class="text-gray-300 font-mono text-xs truncate">{s.name}</span>
                            {restart_badge}
                            {has_error.then(|| view! {
                                <IconAlertCircle class="w-3 h-3 text-red-400 shrink-0 ml-auto"/>
                            })}
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

