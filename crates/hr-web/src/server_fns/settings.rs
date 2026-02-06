use leptos::prelude::*;

use crate::types::SettingsPageData;

#[server]
pub async fn get_settings_data() -> Result<SettingsPageData, ServerFnError> {
    use std::sync::Arc;
    use hr_common::config::EnvConfig;

    let env: Arc<EnvConfig> = expect_context();

    Ok(SettingsPageData {
        base_domain: env.base_domain.clone(),
        api_port: env.api_port,
        data_dir: env.data_dir.display().to_string(),
        acme_email: env.acme_email.clone(),
        acme_staging: env.acme_staging,
        ddns_cron: env.ddns_cron.clone(),
    })
}
