//! App store: catalog management, APK upload/download, update checks.

use axum::extract::{DefaultBodyLimit, Path, Query};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::state::ApiState;

const STORE_DIR: &str = "/opt/homeroute/data/store";

// ── Data structures ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreCatalog {
    pub apps: Vec<StoreApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreApp {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: Option<String>,
    pub publisher_app_id: String,
    pub releases: Vec<StoreRelease>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreRelease {
    pub version: String,
    pub changelog: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}

// ── Persistence helpers ──────────────────────────────────────

fn catalog_path() -> std::path::PathBuf {
    std::path::PathBuf::from(STORE_DIR).join("catalog.json")
}

fn load_catalog() -> StoreCatalog {
    let path = catalog_path();
    match std::fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_default(),
        Err(_) => StoreCatalog::default(),
    }
}

fn save_catalog(catalog: &StoreCatalog) -> Result<(), String> {
    let path = catalog_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create store dir: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(catalog).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(&tmp, data).map_err(|e| format!("Write error: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Rename error: {e}"))?;
    Ok(())
}

// ── Version comparison ───────────────────────────────────────

/// Compare two dotted version strings segment-by-segment as u64.
/// Returns true if `a` is newer than `b`.
fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|seg| seg.parse::<u64>().ok()).collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let max_len = va.len().max(vb.len());
    for i in 0..max_len {
        let sa = va.get(i).copied().unwrap_or(0);
        let sb = vb.get(i).copied().unwrap_or(0);
        if sa > sb {
            return true;
        }
        if sa < sb {
            return false;
        }
    }
    false
}

// ── Router ───────────────────────────────────────────────────

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/apps", get(list_apps))
        .route("/apps/{slug}", get(get_app))
        .route(
            "/apps/{slug}/releases",
            post(publish_release).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
        )
        .route("/releases/{slug}/{version}/download", get(download_release))
        .route("/updates", get(check_updates))
}

// ── Handlers ─────────────────────────────────────────────────

/// GET /api/store/apps — list all apps with summary info.
async fn list_apps() -> impl IntoResponse {
    let catalog = load_catalog();
    let summary: Vec<serde_json::Value> = catalog
        .apps
        .iter()
        .map(|app| {
            let latest = app.releases.last();
            serde_json::json!({
                "slug": app.slug,
                "name": app.name,
                "description": app.description,
                "category": app.category,
                "icon": app.icon,
                "publisher_app_id": app.publisher_app_id,
                "latest_version": latest.map(|r| r.version.as_str()),
                "latest_size_bytes": latest.map(|r| r.size_bytes),
                "release_count": app.releases.len(),
                "created_at": app.created_at,
                "updated_at": app.updated_at,
            })
        })
        .collect();

    Json(serde_json::json!({
        "success": true,
        "apps": summary,
    }))
    .into_response()
}

/// GET /api/store/apps/{slug} — full app details with all releases.
async fn get_app(Path(slug): Path<String>) -> impl IntoResponse {
    let catalog = load_catalog();
    match catalog.apps.iter().find(|a| a.slug == slug) {
        Some(app) => Json(serde_json::json!({
            "success": true,
            "app": app,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "App not found"})),
        )
            .into_response(),
    }
}

/// POST /api/store/apps/{slug}/releases — upload an APK release.
///
/// Body: raw bytes (application/octet-stream).
/// Required headers: `X-Version`.
/// Optional headers: `X-App-Name` (required on first publish), `X-App-Description`,
///   `X-App-Category`, `X-Changelog`, `X-Publisher-App-Id`.
async fn publish_release(
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Validate version header
    let version = match headers.get("X-Version").and_then(|v| v.to_str().ok()) {
        Some(v) => v.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success": false, "error": "X-Version header required"})),
            )
                .into_response();
        }
    };

    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": "Empty body — send the APK as raw bytes"})),
        )
            .into_response();
    }

    // Optional headers
    let app_name = headers
        .get("X-App-Name")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let app_description = headers
        .get("X-App-Description")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let app_category = headers
        .get("X-App-Category")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("other")
        .to_string();
    let changelog = headers
        .get("X-Changelog")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let publisher_app_id = headers
        .get("X-Publisher-App-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Compute SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let sha256: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    let size_bytes = body.len() as u64;

    // Load catalog and check for duplicate version
    let mut catalog = load_catalog();

    if let Some(app) = catalog.apps.iter().find(|a| a.slug == slug) {
        if app.releases.iter().any(|r| r.version == version) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Version {} already exists for {}", version, slug),
                })),
            )
                .into_response();
        }
    }

    // Write APK file atomically
    let release_dir = format!("{}/releases/{}/{}", STORE_DIR, slug, version);
    if let Err(e) = std::fs::create_dir_all(&release_dir) {
        error!(slug, version, "Failed to create release dir: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("Failed to create release directory: {e}")})),
        )
            .into_response();
    }

    let apk_path = format!("{}/app.apk", release_dir);
    let tmp_path = format!("{}/app.apk.tmp", release_dir);
    if let Err(e) = std::fs::write(&tmp_path, &body) {
        error!(slug, version, "Failed to write APK: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("Failed to write APK file: {e}")})),
        )
            .into_response();
    }
    if let Err(e) = std::fs::rename(&tmp_path, &apk_path) {
        error!(slug, version, "Failed to rename APK: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("Failed to finalize APK file: {e}")})),
        )
            .into_response();
    }

    // Upsert into catalog
    let now = Utc::now();
    let release = StoreRelease {
        version: version.clone(),
        changelog,
        sha256: sha256.clone(),
        size_bytes,
        created_at: now,
    };

    if let Some(app) = catalog.apps.iter_mut().find(|a| a.slug == slug) {
        app.releases.push(release);
        app.updated_at = now;
        // Update metadata if provided
        if let Some(name) = &app_name {
            app.name = name.clone();
        }
        if !app_description.is_empty() {
            app.description = app_description;
        }
        if !app_category.is_empty() && app_category != "other" {
            app.category = app_category;
        }
    } else {
        // New app — name is required
        let name = match app_name {
            Some(n) => n,
            None => {
                // Clean up APK file since we can't register the app
                let _ = std::fs::remove_file(&apk_path);
                let _ = std::fs::remove_dir(&release_dir);
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "X-App-Name header required for first publish",
                    })),
                )
                    .into_response();
            }
        };
        catalog.apps.push(StoreApp {
            slug: slug.clone(),
            name,
            description: app_description,
            category: app_category,
            icon: None,
            publisher_app_id,
            releases: vec![release],
            created_at: now,
            updated_at: now,
        });
    }

    if let Err(e) = save_catalog(&catalog) {
        error!(slug, version, "Failed to save catalog: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("Failed to update catalog: {e}")})),
        )
            .into_response();
    }

    info!(slug, version, size_bytes, sha256, "Published new release");

    Json(serde_json::json!({
        "success": true,
        "slug": slug,
        "version": version,
        "sha256": sha256,
        "size_bytes": size_bytes,
    }))
    .into_response()
}

/// GET /api/store/releases/{slug}/{version}/download — download APK.
async fn download_release(Path((slug, version)): Path<(String, String)>) -> impl IntoResponse {
    let apk_path = format!("{}/releases/{}/{}/app.apk", STORE_DIR, slug, version);

    match tokio::fs::read(&apk_path).await {
        Ok(data) => {
            let catalog = load_catalog();
            let sha256 = catalog
                .apps
                .iter()
                .find(|a| a.slug == slug)
                .and_then(|a| a.releases.iter().find(|r| r.version == version))
                .map(|r| r.sha256.clone())
                .unwrap_or_default();

            let filename = format!("{}-{}.apk", slug, version);

            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                "application/vnd.android.package-archive".parse().unwrap(),
            );
            headers.insert(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
            );
            headers.insert("X-Sha256", sha256.parse().unwrap());

            (StatusCode::OK, headers, data).into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": "Release not found"})),
        )
            .into_response(),
    }
}

/// GET /api/store/updates?installed=slug1:v1,slug2:v2 — check for available updates.
#[derive(Deserialize)]
struct UpdateQuery {
    installed: Option<String>,
}

async fn check_updates(Query(query): Query<UpdateQuery>) -> impl IntoResponse {
    let installed_str = query.installed.unwrap_or_default();
    if installed_str.is_empty() {
        return Json(serde_json::json!({
            "success": true,
            "updates": [],
        }))
        .into_response();
    }

    // Parse "slug1:v1,slug2:v2"
    let installed: Vec<(&str, &str)> = installed_str
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, ':');
            let slug = parts.next()?;
            let version = parts.next()?;
            if slug.is_empty() || version.is_empty() {
                None
            } else {
                Some((slug, version))
            }
        })
        .collect();

    let catalog = load_catalog();
    let mut updates = Vec::new();

    for (slug, current_version) in &installed {
        if let Some(app) = catalog.apps.iter().find(|a| a.slug == *slug) {
            if let Some(latest) = app.releases.last() {
                if version_newer(&latest.version, current_version) {
                    updates.push(serde_json::json!({
                        "slug": slug,
                        "name": app.name,
                        "current_version": current_version,
                        "latest_version": latest.version,
                        "latest_changelog": latest.changelog,
                        "latest_sha256": latest.sha256,
                        "latest_size_bytes": latest.size_bytes,
                    }));
                }
            }
        }
    }

    Json(serde_json::json!({
        "success": true,
        "updates": updates,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_newer() {
        assert!(version_newer("1.1.0", "1.0.0"));
        assert!(version_newer("2.0.0", "1.9.9"));
        assert!(version_newer("1.0.1", "1.0.0"));
        assert!(!version_newer("1.0.0", "1.0.0"));
        assert!(!version_newer("1.0.0", "1.0.1"));
        assert!(version_newer("1.0.0.1", "1.0.0"));
        assert!(!version_newer("1.0.0", "1.0.0.1"));
    }
}
