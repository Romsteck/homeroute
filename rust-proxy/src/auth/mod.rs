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
            Err(AuthError::Unauthorized)
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
    Unauthorized,

    #[error("Unexpected status: {0}")]
    UnexpectedStatus(u16),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}
