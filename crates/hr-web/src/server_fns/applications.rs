use leptos::prelude::*;

use crate::types::ApplicationsPageData;
#[cfg(feature = "ssr")]
use crate::types::AppEntry;

#[server]
pub async fn get_applications_data() -> Result<ApplicationsPageData, ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;
    use hr_registry::AgentRegistry;
    use hr_registry::types::AgentStatus;

    let env: Arc<EnvConfig> = expect_context();
    let registry: Option<Arc<AgentRegistry>> = expect_context();

    let base_domain = env.base_domain.clone();

    let Some(registry) = registry else {
        return Ok(ApplicationsPageData {
            applications: vec![],
            base_domain,
            connected_count: 0,
        });
    };

    let apps = registry.list_applications().await;
    let connected_count = apps.iter().filter(|a| a.status == AgentStatus::Connected).count();

    let applications: Vec<AppEntry> = apps
        .into_iter()
        .map(|a| {
            let status = match a.status {
                AgentStatus::Pending => "pending",
                AgentStatus::Deploying => "deploying",
                AgentStatus::Connected => "connected",
                AgentStatus::Disconnected => "disconnected",
                AgentStatus::Error => "error",
            }
            .to_string();
            let (cpu_percent, memory_bytes, code_server_status, app_service_status, db_service_status) =
                if let Some(ref m) = a.metrics {
                    (
                        Some(m.cpu_percent),
                        Some(m.memory_bytes),
                        format!("{:?}", m.code_server_status).to_lowercase(),
                        format!("{:?}", m.app_status).to_lowercase(),
                        format!("{:?}", m.db_status).to_lowercase(),
                    )
                } else {
                    (None, None, "unknown".into(), "unknown".into(), "unknown".into())
                };
            AppEntry {
                id: a.id.clone(),
                name: a.name.clone(),
                slug: a.slug.clone(),
                container_name: a.container_name.clone(),
                enabled: a.enabled,
                status,
                ipv6_address: a.ipv6_address.map(|ip| ip.to_string()).unwrap_or_default(),
                code_server_enabled: a.code_server_enabled,
                frontend_port: a.frontend.target_port,
                frontend_auth_required: a.frontend.auth_required,
                frontend_local_only: a.frontend.local_only,
                api_count: a.apis.len(),
                cpu_percent,
                memory_bytes,
                code_server_status,
                app_service_status,
                db_service_status,
            }
        })
        .collect();

    Ok(ApplicationsPageData {
        applications,
        base_domain,
        connected_count,
    })
}

#[server]
pub async fn toggle_app_service(
    app_id: String,
    action: String,
    service_type: String,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_registry::AgentRegistry;
    use hr_registry::protocol::{ServiceAction, ServiceType};

    let registry: Option<Arc<AgentRegistry>> = expect_context();

    let Some(registry) = registry else {
        leptos_axum::redirect("/applications?msg=error&detail=Registry+non+disponible");
        return Ok(());
    };

    let service_action = match action.as_str() {
        "start" => ServiceAction::Start,
        "stop" => ServiceAction::Stop,
        _ => {
            leptos_axum::redirect("/applications?msg=error&detail=Action+invalide");
            return Ok(());
        }
    };

    let svc_type = match service_type.as_str() {
        "code-server" => ServiceType::CodeServer,
        "db" => ServiceType::Db,
        _ => ServiceType::App,
    };

    let svc_label = match service_type.as_str() {
        "code-server" => "Code-server",
        "db" => "Base+de+donn%C3%A9es",
        _ => "Application",
    };

    match registry.send_service_command(&app_id, svc_type, service_action).await {
        Ok(true) => {
            let verb = if action == "start" { "d%C3%A9marr%C3%A9" } else { "arr%C3%AAt%C3%A9" };
            leptos_axum::redirect(&format!("/applications?msg={svc_label}+{verb}"));
        }
        Ok(false) => {
            leptos_axum::redirect("/applications?msg=error&detail=Application+non+trouv%C3%A9e");
        }
        Err(e) => {
            leptos_axum::redirect(&format!("/applications?msg=error&detail={}", e.to_string().replace(' ', "+")));
        }
    }

    Ok(())
}
