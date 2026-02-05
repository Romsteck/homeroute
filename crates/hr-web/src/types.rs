/// Lightweight user info for the web UI (shared between SSR and WASM).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WebUserInfo {
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
}
