use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::dns_dhcp::ReloadDnsConfig;
use crate::types::DnsDhcpData;
use crate::utils::format_expiry;

fn dns_icon() -> AnyView {
    view! { <IconWifi class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn DnsDhcpPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_dns_dhcp_data());

    view! {
        <PageHeader title="DNS / DHCP" icon=dns_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <DnsDhcpContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_dns_dhcp_data() -> Result<DnsDhcpData, ServerFnError> {
    crate::server_fns::dns_dhcp::get_dns_dhcp_data().await
}

#[component]
fn DnsDhcpContent(data: DnsDhcpData) -> impl IntoView {
    let lease_count = data.leases.len();
    let reload_action = ServerAction::<ReloadDnsConfig>::new();

    view! {
        // Reload button (SSR ActionForm)
        <div class="mb-6">
            <ActionForm action=reload_action attr:class="inline">
                <button
                    type="submit"
                    class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-2"
                >
                    <IconRefreshCw class="w-4 h-4"/>
                    "Recharger la configuration"
                </button>
            </ActionForm>
        </div>

        // DNS Configuration
        <Section title="Configuration DNS">
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Serveurs upstream"</p>
                    <div class="space-y-1">
                        {data.dns.upstream_servers.into_iter().map(|s| view! {
                            <p class="font-mono text-sm text-blue-400">{s}</p>
                        }).collect_view()}
                    </div>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Cache DNS"</p>
                    <p class="text-2xl font-bold text-white">{data.dns.cache_size}</p>
                    <p class="text-xs text-gray-400">"entrées en cache"</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Wildcard DNS"</p>
                    {if !data.dns.wildcard_domain.is_empty() {
                        view! {
                            <p class="text-sm text-white font-mono">{data.dns.wildcard_domain.clone()}</p>
                            <p class="text-xs text-gray-400 mt-1">
                                {format!("IPv4: {} | IPv6: {}", data.dns.wildcard_ipv4, data.dns.wildcard_ipv6)}
                            </p>
                        }.into_any()
                    } else {
                        view! { <p class="text-sm text-gray-500">"Non configuré"</p> }.into_any()
                    }}
                </div>
            </div>
        </Section>

        // Static records
        {(!data.static_records.is_empty()).then(|| view! {
            <Section title="Enregistrements statiques">
                <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                <th class="pb-2 pr-4">"Nom"</th>
                                <th class="pb-2 pr-4">"Type"</th>
                                <th class="pb-2">"Valeur"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {data.static_records.into_iter().map(|r| {
                                let type_color = match r.record_type.as_str() {
                                    "A" => "bg-blue-500/20 text-blue-400",
                                    "AAAA" => "bg-purple-500/20 text-purple-400",
                                    "CNAME" => "bg-green-500/20 text-green-400",
                                    _ => "bg-gray-500/20 text-gray-400",
                                };
                                view! {
                                    <tr class="border-t border-gray-700/50">
                                        <td class="py-2 pr-4 font-mono text-blue-400 text-xs">{r.name}</td>
                                        <td class="py-2 pr-4">
                                            <span class=format!("px-1.5 py-0.5 text-xs rounded {type_color}")>
                                                {r.record_type}
                                            </span>
                                        </td>
                                        <td class="py-2 font-mono text-gray-300 text-xs">{r.value}</td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                </div>
            </Section>
        })}

        // DHCP Configuration
        <Section title="Configuration DHCP" contrast=true>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Interface"</p>
                    <p class="text-sm text-white font-mono">{data.dhcp.interface.clone()}</p>
                    <p class="text-xs text-gray-400 mt-1">{format!("Domaine: {}", data.dhcp.domain)}</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Plage DHCP"</p>
                    <p class="text-sm text-white font-mono">{format!("{} - {}", data.dhcp.range_start, data.dhcp.range_end)}</p>
                    <p class="text-xs text-gray-400 mt-1">{format!("Bail: {}h", data.dhcp.lease_time_secs / 3600)}</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Réseau"</p>
                    <p class="text-xs text-gray-300">{format!("Passerelle: {}", data.dhcp.gateway)}</p>
                    <p class="text-xs text-gray-300">{format!("DNS: {}", data.dhcp.dns_server)}</p>
                </div>
            </div>
        </Section>

        // IPv6
        {data.ipv6.ra_enabled.then(|| view! {
            <Section title="IPv6">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div class="bg-gray-800 border border-gray-700 p-4">
                        <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Router Advertisement"</p>
                        <span class="px-2 py-0.5 text-xs bg-green-500/20 text-green-400 rounded">"Actif"</span>
                    </div>
                    {(!data.ipv6.dhcpv6_range.is_empty()).then(|| view! {
                        <div class="bg-gray-800 border border-gray-700 p-4">
                            <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"DHCPv6"</p>
                            <p class="text-sm text-white font-mono">{data.ipv6.dhcpv6_range.clone()}</p>
                        </div>
                    })}
                </div>
            </Section>
        })}

        // DHCP Leases
        <Section title="Baux DHCP actifs">
            <p class="text-sm text-gray-400 mb-3">{format!("{lease_count} appareil(s) connecté(s)")}</p>
            {if data.leases.is_empty() {
                view! { <p class="text-gray-500">"Aucun bail actif"</p> }.into_any()
            } else {
                view! {
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
                                {data.leases.into_iter().map(|l| {
                                    let expiry = format_expiry(l.expiry);
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class="py-2 pr-4 text-gray-300">
                                                {l.hostname.unwrap_or_else(|| "-".into())}
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-blue-400 text-xs">{l.ip}</td>
                                            <td class="py-2 pr-4 font-mono text-gray-500 text-xs">{l.mac}</td>
                                            <td class="py-2 text-gray-400 text-xs">{expiry}</td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </Section>
    }
}
