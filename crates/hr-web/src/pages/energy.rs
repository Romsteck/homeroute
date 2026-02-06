use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::energy::SetEnergyMode;
use crate::types::EnergyPageData;

fn energy_icon() -> AnyView {
    view! { <IconZap class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn EnergyPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_energy_data());

    view! {
        <PageHeader title="Énergie" icon=energy_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <EnergyContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_energy_data() -> Result<EnergyPageData, ServerFnError> {
    crate::server_fns::energy::get_energy_data().await
}

#[component]
fn EnergyContent(data: EnergyPageData) -> impl IntoView {
    let temp_color = data.temperature.map(|t| {
        if t < 50.0 { "text-green-400" }
        else if t < 70.0 { "text-yellow-400" }
        else if t < 85.0 { "text-orange-400" }
        else { "text-red-400" }
    }).unwrap_or("text-gray-500");

    let current_mode = data.current_mode.clone();

    let mode_action = ServerAction::<SetEnergyMode>::new();

    view! {
        // CPU Info
        <Section title=format!("CPU ({})", data.cpu_model)>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                // Temperature
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Température"</p>
                    <p class=format!("text-3xl font-bold {temp_color}")>
                        {data.temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "--".to_string())}
                    </p>
                </div>
                // Frequency
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Fréquence"</p>
                    <p class="text-3xl font-bold text-blue-400">
                        {data.frequency_current.map(|f| format!("{:.1} GHz", f)).unwrap_or_else(|| "--".to_string())}
                    </p>
                    <p class="text-xs text-gray-500 mt-1">
                        {match (data.frequency_min, data.frequency_max) {
                            (Some(min), Some(max)) => format!("{:.1} - {:.1} GHz", min, max),
                            _ => "--".to_string(),
                        }}
                    </p>
                </div>
                // Usage
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Usage CPU"</p>
                    <p class="text-3xl font-bold text-purple-400">
                        {data.cpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "--".to_string())}
                    </p>
                </div>
            </div>
        </Section>

        // Energy mode selector
        <Section title="Mode énergie">
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
                {[("economy", "Économie", "CPU limité à 60% — économie maximale", "bg-indigo-500/20 text-indigo-400 border-indigo-500/50"),
                  ("auto", "Auto", "CPU limité à 85% — équilibré", "bg-blue-500/20 text-blue-400 border-blue-500/50"),
                  ("performance", "Performance", "CPU pleine puissance", "bg-orange-500/20 text-orange-400 border-orange-500/50")]
                    .into_iter().map(|(mode_id, label, desc, style)| {
                        let is_active = current_mode == mode_id;
                        let border = if is_active { "border-2" } else { "border" };
                        let bg = if is_active { style } else { "bg-gray-800 text-gray-300 border-gray-700" };
                        view! {
                            <ActionForm action=mode_action attr:class="block">
                                <input type="hidden" name="mode" value=mode_id/>
                                <button type="submit" class=format!("w-full text-left p-4 {border} {bg} hover:brightness-110 transition-all")>
                                    <div class="flex items-center justify-between mb-1">
                                        <span class="font-medium">{label}</span>
                                        {is_active.then(|| view! {
                                            <span class="text-xs bg-white/10 px-2 py-0.5 rounded">"Actif"</span>
                                        })}
                                    </div>
                                    <p class="text-xs opacity-75">{desc}</p>
                                </button>
                            </ActionForm>
                        }
                    }).collect_view()}
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Programmation"</p>
                    {if data.schedule_enabled {
                        view! {
                            <div class="flex items-center gap-2">
                                <span class="bg-green-500/20 text-green-400 px-1.5 py-0.5 text-xs rounded">"Actif"</span>
                                <span class="text-sm text-gray-300">
                                    {format!("Économie de {} à {}", data.schedule_night_start, data.schedule_night_end)}
                                </span>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <span class="bg-gray-500/20 text-gray-500 px-1.5 py-0.5 text-xs rounded">"Inactif"</span>
                        }.into_any()
                    }}
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-2">"Auto-select"</p>
                    {if data.auto_select_enabled {
                        view! {
                            <div class="flex items-center gap-2">
                                <span class="bg-green-500/20 text-green-400 px-1.5 py-0.5 text-xs rounded">"Actif"</span>
                                {data.auto_select_interface.map(|iface| view! {
                                    <span class="text-sm text-gray-300 font-mono">{iface}</span>
                                })}
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <span class="bg-gray-500/20 text-gray-500 px-1.5 py-0.5 text-xs rounded">"Inactif"</span>
                        }.into_any()
                    }}
                </div>
            </div>
        </Section>
    }
}
