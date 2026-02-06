use leptos::prelude::*;

use crate::types::UsersPageData;
#[cfg(feature = "ssr")]
use crate::types::{GroupEntry, UserEntry};

#[server]
pub async fn get_users_data() -> Result<UsersPageData, ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    // Get all users
    let raw_users = auth.users.get_all();
    let users: Vec<UserEntry> = raw_users
        .iter()
        .map(|u| UserEntry {
            username: u.username.clone(),
            displayname: u.displayname.clone(),
            email: u.email.clone(),
            groups: u.groups.clone(),
            disabled: u.disabled,
        })
        .collect();

    // Build groups from user group memberships + builtins
    let mut group_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    group_ids.insert("admins".to_string());
    group_ids.insert("users".to_string());
    for u in &raw_users {
        for g in &u.groups {
            group_ids.insert(g.clone());
        }
    }

    let groups: Vec<GroupEntry> = group_ids
        .into_iter()
        .map(|id| {
            let member_count = raw_users.iter().filter(|u| u.groups.contains(&id)).count();
            let built_in = id == "admins" || id == "users";
            GroupEntry {
                name: id.clone(),
                id,
                built_in,
                member_count,
            }
        })
        .collect();

    Ok(UsersPageData { users, groups })
}

#[server]
pub async fn create_user(
    username: String,
    displayname: String,
    password: String,
    email: String,
) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    let result = auth.users.create(
        &username.to_lowercase(),
        &password,
        Some(&displayname),
        Some(&email),
        vec!["users".to_string()],
    );

    if !result.success {
        let err = result.error.unwrap_or_else(|| "Erreur inconnue".to_string());
        leptos_axum::redirect(&format!("/users?msg=error&detail={}", err.replace(' ', "+")));
        return Ok(());
    }

    leptos_axum::redirect("/users?msg=Utilisateur+cr%C3%A9%C3%A9");
    Ok(())
}

#[server]
pub async fn delete_user(username: String) -> Result<(), ServerFnError> {
    use std::sync::Arc;
    use hr_auth::AuthService;

    let auth: Arc<AuthService> = expect_context();

    // Prevent deleting admin users
    if let Some(user) = auth.users.get(&username) {
        if user.groups.contains(&"admins".to_string()) {
            leptos_axum::redirect("/users?msg=error&detail=Impossible+de+supprimer+un+administrateur");
            return Ok(());
        }
    }

    let _ = auth.sessions.delete_by_user(&username);
    let result = auth.users.delete(&username);

    if !result.success {
        let err = result.error.unwrap_or_else(|| "Erreur inconnue".to_string());
        leptos_axum::redirect(&format!("/users?msg=error&detail={}", err.replace(' ', "+")));
        return Ok(());
    }

    leptos_axum::redirect("/users?msg=Utilisateur+supprim%C3%A9");
    Ok(())
}
