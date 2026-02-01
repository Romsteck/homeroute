use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Vérifie l'authentification via forward-auth
pub async fn check_auth(
    auth_service_url: &str,
    headers: &HeaderMap,
    host: &str,
    uri: &str,
) -> Result<AuthResponse, AuthError> {
    let client = Client::new();

    // Extraire le cookie auth_session
    let cookie_header = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Forward auth request
    let url = format!("{}/api/authz/forward-auth", auth_service_url);

    let response = client
        .get(&url)
        .header("Cookie", cookie_header)
        .header("X-Forwarded-Host", host)
        .header("X-Forwarded-Uri", uri)
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .map_err(|e| AuthError::RequestFailed(e.to_string()))?;

    match response.status() {
        StatusCode::OK => {
            let auth_data: AuthResponse = response
                .json::<AuthResponse>()
                .await
                .map_err(|e: reqwest::Error| AuthError::ParseError(e.to_string()))?;
            Ok(auth_data)
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            // Extract redirect URL from X-Auth-Redirect header
            let redirect = response
                .headers()
                .get("x-auth-redirect")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            Err(AuthError::Unauthorized(redirect))
        }
        status => Err(AuthError::UnexpectedStatus(status.as_u16())),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub success: bool,
    pub user: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unauthorized")]
    Unauthorized(Option<String>),

    #[error("Unexpected status: {0}")]
    UnexpectedStatus(u16),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}
