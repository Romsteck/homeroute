use leptos::prelude::*;

use crate::types::ProfileData;

#[server]
pub async fn get_profile_data() -> Result<ProfileData, ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    let parts = use_context::<axum::http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("No request context"))?;

    let session_id = parts
        .headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|c| c.trim().strip_prefix("auth_session=").map(String::from))
        })
        .ok_or_else(|| ServerFnError::new("Non authentifié"))?;

    let session = auth
        .sessions
        .validate(&session_id)
        .map_err(|e| ServerFnError::new(format!("{e}")))?
        .ok_or_else(|| ServerFnError::new("Session invalide"))?;

    let user = auth
        .users
        .get(&session.user_id)
        .ok_or_else(|| ServerFnError::new("Utilisateur introuvable"))?;

    Ok(ProfileData {
        username: user.username.clone(),
        display_name: user.displayname.clone(),
        email: user.email.clone(),
        groups: user.groups.clone(),
        is_admin: user.groups.contains(&"admins".to_string()),
    })
}

#[server]
pub async fn change_password(
    current_password: String,
    new_password: String,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    // Get current user from session
    let parts = use_context::<axum::http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("No request context"))?;

    let session_id = parts
        .headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|c| c.trim().strip_prefix("auth_session=").map(String::from))
        })
        .ok_or_else(|| ServerFnError::new("Non authentifié"))?;

    let session = auth
        .sessions
        .validate(&session_id)
        .map_err(|e| ServerFnError::new(format!("{e}")))?
        .ok_or_else(|| ServerFnError::new("Session invalide"))?;

    let user_with_pw = auth
        .users
        .get_with_password(&session.user_id)
        .ok_or_else(|| ServerFnError::new("Utilisateur introuvable"))?;

    // Verify current password
    if !hr_auth::users::verify_password(&current_password, &user_with_pw.password_hash) {
        return Err(ServerFnError::new("Mot de passe actuel incorrect"));
    }

    // Change password
    let result = auth.users.change_password(&user_with_pw.username, &new_password);
    if result.success {
        leptos_axum::redirect("/profile?msg=Mot+de+passe+modifi%C3%A9");
        Ok(())
    } else {
        Err(ServerFnError::new(
            result.error.unwrap_or_else(|| "Erreur inconnue".to_string()),
        ))
    }
}
