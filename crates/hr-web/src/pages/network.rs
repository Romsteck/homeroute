use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::types::NetworkData;

fn network_icon() -> AnyView {
    view! { <IconNetwork class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn NetworkPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_network_data());

    view! {
        <PageHeader title="Réseau" icon=network_icon/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <NetworkContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_network_data() -> Result<NetworkData, ServerFnError> {
    crate::server_fns::network::get_network_data().await
}

#[component]
fn NetworkContent(data: NetworkData) -> impl IntoView {
    let up_count = data.interfaces.iter().filter(|i| i.state == "UP").count();
    let total = data.interfaces.len();

    view! {
        // Interfaces
        <Section title="Interfaces">
            <p class="text-sm text-gray-400 mb-4">{format!("{up_count}/{total} actives")}</p>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                {data.interfaces.into_iter().map(|iface| {
                    let state_class = if iface.state == "UP" {
                        "bg-green-400"
                    } else {
                        "bg-red-400"
                    };
                    let name = iface.name.clone();
                    let state_display = iface.state.clone();
                    let mac = iface.mac.clone();
                    view! {
                        <div class="bg-gray-800 border border-gray-700 p-4">
                            <div class="flex items-center justify-between mb-3">
                                <div class="flex items-center gap-2">
                                    <span class=format!("w-2 h-2 rounded-full {state_class}")></span>
                                    <span class="text-white font-mono font-medium">{name}</span>
                                </div>
                                <span class="text-xs text-gray-400">{state_display}</span>
                            </div>
                            <div class="space-y-1 text-xs">
                                {(!iface.mac.is_empty()).then(move || view! {
                                    <div class="flex justify-between">
                                        <span class="text-gray-500">"MAC"</span>
                                        <span class="font-mono text-gray-300">{mac}</span>
                                    </div>
                                })}
                                {iface.mtu.map(|mtu| view! {
                                    <div class="flex justify-between">
                                        <span class="text-gray-500">"MTU"</span>
                                        <span class="text-gray-300">{mtu}</span>
                                    </div>
                                })}
                                {iface.addresses.into_iter().map(|addr| {
                                    let color = if addr.family == "inet" {
                                        "text-blue-400"
                                    } else {
                                        "text-purple-400"
                                    };
                                    let label = if addr.family == "inet" { "IPv4" } else { "IPv6" };
                                    let display = match addr.prefixlen {
                                        Some(p) => format!("{}/{p}", addr.address),
                                        None => addr.address,
                                    };
                                    view! {
                                        <div class="flex justify-between">
                                            <span class="text-gray-500">{label}</span>
                                            <span class=format!("font-mono {color}")>{display}</span>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        </div>
                    }
                }).collect_view()}
            </div>
        </Section>

        // IPv4 Routes
        <Section title="Routes IPv4" contrast=true>
            {if data.ipv4_routes.is_empty() {
                view! { <p class="text-gray-500">"Aucune route"</p> }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Destination"</th>
                                    <th class="pb-2 pr-4">"Passerelle"</th>
                                    <th class="pb-2 pr-4">"Interface"</th>
                                    <th class="pb-2">"Métrique"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.ipv4_routes.into_iter().map(|r| {
                                    let dst_class = if r.destination == "default" {
                                        "text-blue-400 font-bold"
                                    } else {
                                        "text-gray-300"
                                    };
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class=format!("py-2 pr-4 font-mono text-xs {dst_class}")>{r.destination}</td>
                                            <td class="py-2 pr-4 font-mono text-gray-400 text-xs">
                                                {r.gateway.unwrap_or_else(|| "-".into())}
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-300 text-xs">{r.device}</td>
                                            <td class="py-2 text-gray-400 text-xs">
                                                {r.metric.map(|m| m.to_string()).unwrap_or_else(|| "-".into())}
                                            </td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </Section>

        // IPv6 Routes
        <Section title="Routes IPv6">
            {if data.ipv6_routes.is_empty() {
                view! { <p class="text-gray-500">"Aucune route"</p> }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-gray-500 uppercase tracking-wider">
                                    <th class="pb-2 pr-4">"Destination"</th>
                                    <th class="pb-2 pr-4">"Passerelle"</th>
                                    <th class="pb-2 pr-4">"Interface"</th>
                                    <th class="pb-2">"Métrique"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.ipv6_routes.into_iter().map(|r| {
                                    let dst_class = if r.destination == "default" {
                                        "text-blue-400 font-bold"
                                    } else {
                                        "text-gray-300"
                                    };
                                    view! {
                                        <tr class="border-t border-gray-700/50">
                                            <td class=format!("py-2 pr-4 font-mono text-xs {dst_class}")>{r.destination}</td>
                                            <td class="py-2 pr-4 font-mono text-gray-400 text-xs">
                                                {r.gateway.unwrap_or_else(|| "-".into())}
                                            </td>
                                            <td class="py-2 pr-4 font-mono text-gray-300 text-xs">{r.device}</td>
                                            <td class="py-2 text-gray-400 text-xs">
                                                {r.metric.map(|m| m.to_string()).unwrap_or_else(|| "-".into())}
                                            </td>
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
