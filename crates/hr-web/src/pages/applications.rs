use leptos::prelude::*;

use crate::components::icons::*;
use crate::components::page_header::PageHeader;
use crate::components::section::Section;
use crate::components::toast::FlashMessage;
use crate::server_fns::applications::ToggleAppService;
use crate::types::{AppEntry, ApplicationsPageData};

fn apps_icon() -> AnyView {
    view! { <IconBoxes class="w-6 h-6"/> }.into_any()
}

#[component]
pub fn ApplicationsPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_apps_data());

    view! {
        <PageHeader title="Applications" icon=apps_icon/>
        <FlashMessage/>
        <Suspense fallback=|| view! { <div class="p-8 text-gray-400">"Chargement..."</div> }>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => view! { <AppsContent data=d/> }.into_any(),
                    Err(e) => view! {
                        <div class="p-8 text-red-400">{e.to_string()}</div>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

async fn get_apps_data() -> Result<ApplicationsPageData, ServerFnError> {
    crate::server_fns::applications::get_applications_data().await
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn service_badge_class(status: &str) -> &'static str {
    match status {
        "running" => "bg-green-500/20 text-green-400",
        "starting" | "stopping" => "bg-blue-500/20 text-blue-400",
        "stopped" | "manuallyoff" => "bg-gray-500/20 text-gray-500",
        _ => "bg-gray-500/20 text-gray-600",
    }
}

fn service_label(status: &str) -> &'static str {
    match status {
        "running" => "ON",
        "starting" => "...",
        "stopping" => "...",
        "stopped" | "manuallyoff" => "OFF",
        _ => "-",
    }
}

fn is_service_running(status: &str) -> bool {
    status == "running"
}

#[component]
fn AppsContent(data: ApplicationsPageData) -> impl IntoView {
    let total = data.applications.len();
    let base_domain = data.base_domain.clone();
    let base_domain_display = if data.base_domain.is_empty() {
        "Non configuré".to_string()
    } else {
        data.base_domain
    };

    view! {
        // Stats
        <Section title="Vue d'ensemble">
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Applications"</p>
                    <p class="text-2xl font-bold text-white">{total}</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Agents connectés"</p>
                    <p class="text-2xl font-bold text-green-400">{data.connected_count}</p>
                </div>
                <div class="bg-gray-800 border border-gray-700 p-4">
                    <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">"Domaine"</p>
                    <p class="text-sm font-mono text-blue-400 truncate">
                        {base_domain_display}
                    </p>
                </div>
            </div>
        </Section>

        // Application cards
        <Section title="Applications">
            {if data.applications.is_empty() {
                view! {
                    <div class="text-center py-12 text-gray-500">
                        <IconBoxes class="w-12 h-12 mx-auto mb-3 opacity-50"/>
                        <p>"Aucune application"</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="space-y-4">
                        {data.applications.into_iter().map(|app| {
                            view! { <AppCard app=app base_domain=base_domain.clone()/> }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </Section>
    }
}

#[component]
fn AppCard(app: AppEntry, base_domain: String) -> impl IntoView {
    let status_class = match app.status.as_str() {
        "connected" => "bg-green-500/20 text-green-400",
        "deploying" => "bg-blue-500/20 text-blue-400",
        "pending" => "bg-yellow-500/20 text-yellow-400",
        _ => "bg-red-500/20 text-red-400",
    };
    let status_label = match app.status.as_str() {
        "connected" => "Connecté",
        "deploying" => "Déploiement",
        "pending" => "En attente",
        "disconnected" => "Déconnecté",
        _ => "Erreur",
    };
    let is_connected = app.status == "connected";

    // Domains
    let domain = format!("{}.{}", app.slug, &base_domain);
    let ide_url = format!("https://{}.code.{}", app.slug, &base_domain);

    // CPU display
    let cpu_text = app.cpu_percent
        .map(|c| format!("{:.0}%", c))
        .unwrap_or_else(|| "-".into());
    let cpu_class = match app.cpu_percent {
        Some(c) if c > 80.0 => "text-red-400",
        Some(c) if c > 50.0 => "text-yellow-400",
        Some(_) => "text-green-400",
        None => "text-gray-600",
    };

    // RAM display
    let ram_text = app.memory_bytes
        .map(|b| format_bytes(b))
        .unwrap_or_else(|| "-".into());

    // Service statuses
    let cs_class = service_badge_class(&app.code_server_status);
    let cs_label = service_label(&app.code_server_status);
    let app_svc_class = service_badge_class(&app.app_service_status);
    let app_svc_label = service_label(&app.app_service_status);
    let db_class = service_badge_class(&app.db_service_status);
    let db_label = service_label(&app.db_service_status);

    // For service toggle buttons
    let cs_running = is_service_running(&app.code_server_status);
    let app_running = is_service_running(&app.app_service_status);
    let db_running = is_service_running(&app.db_service_status);

    let app_id = app.id.clone();

    view! {
        <div class="bg-gray-800 border border-gray-700 p-4">
            // Row 1: Name + status + metrics
            <div class="flex items-center justify-between mb-3">
                <div class="flex items-center gap-3">
                    <div>
                        <div class="flex items-center gap-2">
                            <span class="text-white font-medium">{app.name.clone()}</span>
                            {(!app.enabled).then(|| view! {
                                <span class="text-xs text-gray-500 bg-gray-700 px-1.5 py-0.5">"off"</span>
                            })}
                        </div>
                        <span class="text-xs font-mono text-gray-500">{domain.clone()}</span>
                    </div>
                    <span
                        class=format!("px-1.5 py-0.5 text-xs rounded {status_class}")
                        data-ws-target=format!("agent:status:status:{}", app_id)
                    >
                        {status_label}
                    </span>
                </div>
                // Live metrics
                <div class="flex items-center gap-4 text-xs">
                    <div class="flex items-center gap-1" title="CPU">
                        <IconZap class="w-3 h-3 text-gray-500"/>
                        <span
                            class=format!("font-mono {cpu_class}")
                            data-ws-target=format!("agent:metrics:cpuPercent:{}", app_id)
                        >
                            {cpu_text}
                        </span>
                    </div>
                    <div class="flex items-center gap-1" title="RAM">
                        <IconHardDrive class="w-3 h-3 text-gray-500"/>
                        <span
                            class="font-mono text-gray-300"
                            data-ws-target=format!("agent:metrics:memoryBytes:{}", app_id)
                        >
                            {ram_text}
                        </span>
                    </div>
                </div>
            </div>

            // Row 2: Services + Controls
            <div class="flex items-center justify-between border-t border-gray-700/50 pt-3">
                // Service badges
                <div class="flex items-center gap-2">
                    // code-server
                    {app.code_server_enabled.then(|| view! {
                        <ServiceBadge
                            label="IDE"
                            status_class=cs_class
                            status_label=cs_label
                            ws_target=format!("agent:metrics:codeServerStatus:{}", app_id)
                        />
                    })}
                    // App service
                    <ServiceBadge
                        label="App"
                        status_class=app_svc_class
                        status_label=app_svc_label
                        ws_target=format!("agent:metrics:appStatus:{}", app_id)
                    />
                    // DB service
                    <ServiceBadge
                        label="DB"
                        status_class=db_class
                        status_label=db_label
                        ws_target=format!("agent:metrics:dbStatus:{}", app_id)
                    />
                    // Options tags
                    {app.frontend_auth_required.then(|| view! {
                        <span class="text-[10px] text-purple-400 bg-purple-900/30 px-1 py-0.5 rounded">"Auth"</span>
                    })}
                    {app.frontend_local_only.then(|| view! {
                        <span class="text-[10px] text-yellow-400 bg-yellow-900/30 px-1 py-0.5 rounded">"Local"</span>
                    })}
                    {(app.api_count > 0).then(|| view! {
                        <span class="text-[10px] text-blue-400 bg-blue-900/30 px-1 py-0.5 rounded">
                            {format!("{} API", app.api_count)}
                        </span>
                    })}
                </div>

                // Action buttons
                <div class="flex items-center gap-2">
                    // IDE link (code-server)
                    {app.code_server_enabled.then(|| view! {
                        <a
                            href=ide_url
                            target="_blank"
                            class="inline-flex items-center gap-1 px-2 py-1 text-xs text-cyan-400 hover:text-cyan-300 bg-cyan-900/20 hover:bg-cyan-900/40 rounded transition-colors"
                            title="Ouvrir l'IDE"
                        >
                            <IconServer class="w-3 h-3"/>
                            "IDE"
                        </a>
                    })}

                    // Frontend link
                    <a
                        href=format!("https://{domain}")
                        target="_blank"
                        class="inline-flex items-center gap-1 px-2 py-1 text-xs text-blue-400 hover:text-blue-300 bg-blue-900/20 hover:bg-blue-900/40 rounded transition-colors"
                        title="Ouvrir l'application"
                    >
                        <IconGlobe class="w-3 h-3"/>
                        "App"
                    </a>

                    // Service controls (only when connected)
                    {is_connected.then(|| view! {
                        <ServiceControls
                            app_id=app_id.clone()
                            code_server_enabled=app.code_server_enabled
                            cs_running=cs_running
                            app_running=app_running
                            db_running=db_running
                        />
                    })}
                </div>
            </div>
        </div>
    }
}

#[component]
fn ServiceBadge(
    label: &'static str,
    status_class: &'static str,
    status_label: &'static str,
    ws_target: String,
) -> impl IntoView {
    view! {
        <span
            class=format!("px-1.5 py-0.5 text-[10px] rounded {status_class}")
            data-ws-target=ws_target
        >
            {format!("{label} {status_label}")}
        </span>
    }
}

#[component]
fn ServiceControls(
    app_id: String,
    code_server_enabled: bool,
    cs_running: bool,
    app_running: bool,
    db_running: bool,
) -> impl IntoView {
    let cs_action = ServerAction::<ToggleAppService>::new();
    let app_action = ServerAction::<ToggleAppService>::new();
    let db_action = ServerAction::<ToggleAppService>::new();

    let app_id_cs = app_id.clone();
    let app_id_app = app_id.clone();
    let app_id_db = app_id.clone();

    view! {
        <div class="flex items-center gap-1 border-l border-gray-700 pl-2 ml-1">
            // code-server toggle
            {code_server_enabled.then(|| {
                let (action_val, btn_class, btn_label) = if cs_running {
                    ("stop", "text-red-400 hover:text-red-300 bg-red-900/20 hover:bg-red-900/40", "IDE ■")
                } else {
                    ("start", "text-green-400 hover:text-green-300 bg-green-900/20 hover:bg-green-900/40", "IDE ▶")
                };
                view! {
                    <ActionForm action=cs_action attr:class="inline">
                        <input type="hidden" name="app_id" value=app_id_cs.clone()/>
                        <input type="hidden" name="action" value=action_val/>
                        <input type="hidden" name="service_type" value="code-server"/>
                        <button
                            type="submit"
                            class=format!("px-1.5 py-0.5 text-[10px] rounded transition-colors {btn_class}")
                            title=format!("{} code-server", if cs_running { "Arrêter" } else { "Démarrer" })
                        >
                            {btn_label}
                        </button>
                    </ActionForm>
                }
            })}

            // App toggle
            {
                let (action_val, btn_class, btn_label) = if app_running {
                    ("stop", "text-red-400 hover:text-red-300 bg-red-900/20 hover:bg-red-900/40", "App ■")
                } else {
                    ("start", "text-green-400 hover:text-green-300 bg-green-900/20 hover:bg-green-900/40", "App ▶")
                };
                view! {
                    <ActionForm action=app_action attr:class="inline">
                        <input type="hidden" name="app_id" value=app_id_app.clone()/>
                        <input type="hidden" name="action" value=action_val/>
                        <input type="hidden" name="service_type" value="app"/>
                        <button
                            type="submit"
                            class=format!("px-1.5 py-0.5 text-[10px] rounded transition-colors {btn_class}")
                            title=format!("{} l'application", if app_running { "Arrêter" } else { "Démarrer" })
                        >
                            {btn_label}
                        </button>
                    </ActionForm>
                }
            }

            // DB toggle
            {
                let (action_val, btn_class, btn_label) = if db_running {
                    ("stop", "text-red-400 hover:text-red-300 bg-red-900/20 hover:bg-red-900/40", "DB ■")
                } else {
                    ("start", "text-green-400 hover:text-green-300 bg-green-900/20 hover:bg-green-900/40", "DB ▶")
                };
                view! {
                    <ActionForm action=db_action attr:class="inline">
                        <input type="hidden" name="app_id" value=app_id_db.clone()/>
                        <input type="hidden" name="action" value=action_val/>
                        <input type="hidden" name="service_type" value="db"/>
                        <button
                            type="submit"
                            class=format!("px-1.5 py-0.5 text-[10px] rounded transition-colors {btn_class}")
                            title=format!("{} la base de données", if db_running { "Arrêter" } else { "Démarrer" })
                        >
                            {btn_label}
                        </button>
                    </ActionForm>
                }
            }
        </div>
    }
}
