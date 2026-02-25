use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::debug;

const STUDIO_HOME: &str = "/home/studio";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCredentials {
    pub method: String,
    pub token: Option<String>,
    pub subscription_type: Option<String>,
    pub created_at: u64,
}

/// Sanitize username to prevent path traversal: keep only alphanumeric, dash, underscore, dot.
fn sanitize_username(username: &str) -> String {
    username
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Return the per-user Claude directory: /home/studio/.claude/users/{sanitized_name}
pub fn user_claude_dir(username: &str) -> PathBuf {
    let safe = sanitize_username(username);
    PathBuf::from(format!("{STUDIO_HOME}/.claude/users/{safe}"))
}

/// Return the path to the user's .credentials.json (for OAuth method).
pub fn user_credentials_json(username: &str) -> PathBuf {
    user_claude_dir(username).join(".credentials.json")
}

/// Return the path to the user's metadata file.
fn user_meta_path(username: &str) -> PathBuf {
    user_claude_dir(username).join("user-auth.json")
}

/// Check if a user has valid credentials stored.
pub async fn has_credentials(username: &str) -> bool {
    get_auth_status(username).await.is_some()
}

/// Read the user's credential metadata, returning None if not found or invalid.
pub async fn get_auth_status(username: &str) -> Option<UserCredentials> {
    let meta = user_meta_path(username);
    if let Ok(data) = tokio::fs::read_to_string(&meta).await {
        if let Ok(cred) = serde_json::from_str::<UserCredentials>(&data) {
            // Validate: for oauth, the .credentials.json must also exist
            if cred.method == "oauth" && !user_credentials_json(username).exists() {
                debug!("OAuth metadata exists for {username} but .credentials.json missing");
            } else if cred.method == "token" && cred.token.is_none() {
                // For token method, the token field must be present
                debug!("Token metadata exists for {username} but token field is empty");
            } else {
                return Some(cred);
            }
        }
    }

    // Fallback: detect native Claude OAuth credentials at /home/studio/.claude/.credentials.json
    // This covers users who authenticated via `claude` CLI directly (not via Studio token flow)
    let native_creds = PathBuf::from(format!("{STUDIO_HOME}/.claude/.credentials.json"));
    if native_creds.exists() {
        debug!("Native Claude OAuth credentials found for {username}");
        return Some(UserCredentials {
            method: "claudeAiOauth".to_string(),
            token: None,
            subscription_type: None,
            created_at: 0,
        });
    }

    None
}

/// Save a pasted API/OAuth token for a user.
pub async fn save_token(username: &str, token: &str) -> Result<(), String> {
    let dir = user_claude_dir(username);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create user dir: {e}"))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let cred = UserCredentials {
        method: "token".to_string(),
        token: Some(token.to_string()),
        subscription_type: None,
        created_at: now,
    };

    let json = serde_json::to_string_pretty(&cred)
        .map_err(|e| format!("Failed to serialize credentials: {e}"))?;

    tokio::fs::write(user_meta_path(username), json)
        .await
        .map_err(|e| format!("Failed to write credentials: {e}"))?;

    // chown the user dir to studio:studio
    let _ = tokio::process::Command::new("chown")
        .args(["-R", "studio:studio", &dir.to_string_lossy().to_string()])
        .output()
        .await;

    debug!("Saved token credentials for user {username}");
    Ok(())
}

/// Remove all credentials for a user.
pub async fn remove_credentials(username: &str) -> Result<(), String> {
    let dir = user_claude_dir(username);
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|e| format!("Failed to remove credentials: {e}"))?;
        debug!("Removed credentials for user {username}");
    }
    Ok(())
}
