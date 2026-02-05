use leptos::prelude::*;
use crate::types::WebUserInfo;

/// Check the current user's session from the `auth_session` cookie.
/// Returns `None` if not authenticated.
#[server]
pub async fn get_current_user() -> Result<Option<WebUserInfo>, ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    // Request Parts are provided by leptos_axum during SSR
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
        });

    let session_id = match session_id {
        Some(id) => id,
        None => return Ok(None),
    };

    let session = match auth.sessions.validate(&session_id) {
        Ok(Some(s)) => s,
        _ => return Ok(None),
    };

    match auth.users.get(&session.user_id) {
        Some(user) => Ok(Some(WebUserInfo {
            username: user.username.clone(),
            display_name: user.displayname.clone(),
            is_admin: user.groups.contains(&"admins".to_string()),
        })),
        None => Ok(None),
    }
}

/// Authenticate a user. Sets the `auth_session` cookie on success.
#[server]
pub async fn login(
    username: String,
    password: String,
    remember_me: Option<String>,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();
    let response = expect_context::<leptos_axum::ResponseOptions>();
    let parts = use_context::<axum::http::request::Parts>();

    let username = username.to_lowercase();

    if username.is_empty() || password.is_empty() {
        return Err(ServerFnError::new("Nom d'utilisateur et mot de passe requis"));
    }

    let user = auth
        .users
        .get_with_password(&username)
        .ok_or_else(|| ServerFnError::new("Identifiants invalides"))?;

    if user.disabled {
        return Err(ServerFnError::new("Compte désactivé"));
    }

    if !hr_auth::users::verify_password(&password, &user.password_hash) {
        return Err(ServerFnError::new("Identifiants invalides"));
    }

    let ip = parts.as_ref().and_then(|p| {
        p.headers
            .get("x-real-ip")
            .or_else(|| p.headers.get("x-forwarded-for"))
            .and_then(|v| v.to_str().ok())
    });
    let ua = parts
        .as_ref()
        .and_then(|p| p.headers.get("user-agent").and_then(|v| v.to_str().ok()));

    let remember = remember_me.is_some();

    let (session_id, _) = auth
        .sessions
        .create(&username, ip, ua, remember)
        .map_err(|e| ServerFnError::new(format!("Erreur session: {e}")))?;

    auth.users.update_last_login(&username);

    // Build Set-Cookie header
    let mut cookie_parts = vec![
        format!("auth_session={session_id}"),
        "HttpOnly".into(),
        "SameSite=Lax".into(),
        "Path=/".into(),
    ];

    if let Some(ref p) = parts {
        let proto = p
            .headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        if proto == "https" {
            cookie_parts.push("Secure".into());
            cookie_parts.push(format!("Domain=.{}", auth.base_domain));
        }
    }

    if remember {
        cookie_parts.push(format!("Max-Age={}", 30 * 24 * 60 * 60));
    }

    response.insert_header(
        axum::http::header::SET_COOKIE,
        cookie_parts.join("; ").parse().unwrap(),
    );

    leptos_axum::redirect("/");
    Ok(())
}

/// Destroy the current session and clear the cookie.
#[server]
pub async fn logout() -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();
    let response = expect_context::<leptos_axum::ResponseOptions>();
    let parts = use_context::<axum::http::request::Parts>();

    // Delete server-side session
    if let Some(ref p) = parts {
        if let Some(session_id) = p
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                cookies
                    .split(';')
                    .find_map(|c| c.trim().strip_prefix("auth_session=").map(String::from))
            })
        {
            let _ = auth.sessions.delete(&session_id);
        }
    }

    // Clear cookie
    let mut clear = vec![
        "auth_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".into(),
    ];
    if let Some(ref p) = parts {
        let proto = p
            .headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        if proto == "https" {
            clear = vec![format!(
                "auth_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0; Secure; Domain=.{}",
                auth.base_domain
            )];
        }
    }

    response.insert_header(
        axum::http::header::SET_COOKIE,
        clear[0].parse().unwrap(),
    );

    leptos_axum::redirect("/login");
    Ok(())
}
