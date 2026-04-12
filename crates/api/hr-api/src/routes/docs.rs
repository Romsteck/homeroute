//! Documentation: per-app docs with sections (meta, structure, features, backend, notes).

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::state::ApiState;

const DOCS_DIR: &str = "/opt/homeroute/data/docs";
const SECTIONS: &[&str] = &["structure", "features", "backend", "notes"];

fn validate_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains('/')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn validate_section(s: &str) -> bool {
    s == "meta" || SECTIONS.contains(&s)
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/", get(list_docs))
        .route("/search", get(search_docs))
        .route("/{app_id}", get(get_docs).post(create_docs))
        .route("/{app_id}/{section}", get(get_section).post(update_section))
}

/// GET /api/docs — list all documented apps
async fn list_docs() -> impl IntoResponse {
    let dir = std::path::Path::new(DOCS_DIR);
    if !dir.exists() {
        return Json(json!({ "success": true, "apps": [] })).into_response();
    }
    let mut apps = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": "Failed to read docs directory" })),
        )
            .into_response();
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let app_id = entry.file_name().to_string_lossy().to_string();
        let app_dir = entry.path();

        let meta: serde_json::Value = std::fs::read_to_string(app_dir.join("meta.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));

        let name = meta
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&app_id)
            .to_string();

        let mut filled = 0u32;
        if app_dir.join("meta.json").exists() {
            let content = std::fs::read_to_string(app_dir.join("meta.json")).unwrap_or_default();
            if content.trim().len() > 2 {
                filled += 1;
            }
        }
        for s in SECTIONS {
            if let Ok(content) = std::fs::read_to_string(app_dir.join(format!("{s}.md"))) {
                if !content.trim().is_empty() {
                    filled += 1;
                }
            }
        }

        apps.push(json!({
            "app_id": app_id,
            "name": name,
            "meta": meta,
            "filled": filled,
            "total": 5,
        }));
    }
    apps.sort_by(|a, b| {
        a["app_id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["app_id"].as_str().unwrap_or(""))
    });
    Json(json!({ "success": true, "apps": apps })).into_response()
}

/// GET /api/docs/:appId — get all sections
async fn get_docs(Path(app_id): Path<String>) -> impl IntoResponse {
    if !validate_id(&app_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Invalid app_id" })),
        )
            .into_response();
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(&app_id);
    if !app_dir.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Not found" })),
        )
            .into_response();
    }

    let meta = std::fs::read_to_string(app_dir.join("meta.json")).unwrap_or_default();
    let meta_parsed: serde_json::Value = serde_json::from_str(&meta).unwrap_or(json!({}));

    let mut sections = json!({});
    for s in SECTIONS {
        let content = std::fs::read_to_string(app_dir.join(format!("{s}.md"))).unwrap_or_default();
        sections[s] = json!(content);
    }

    Json(json!({
        "success": true,
        "app_id": app_id,
        "meta": meta_parsed,
        "sections": sections,
    }))
    .into_response()
}

/// POST /api/docs/:appId — create docs for an app
async fn create_docs(Path(app_id): Path<String>) -> impl IntoResponse {
    if !validate_id(&app_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Invalid app_id" })),
        )
            .into_response();
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(&app_id);
    if app_dir.exists() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "success": false, "error": "Already exists" })),
        )
            .into_response();
    }
    if let Err(e) = std::fs::create_dir_all(&app_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": format!("Failed: {e}") })),
        )
            .into_response();
    }
    let meta = json!({ "name": app_id, "stack": "", "description": "", "logo": "" });
    let _ = std::fs::write(
        app_dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
    for s in SECTIONS {
        let _ = std::fs::write(app_dir.join(format!("{s}.md")), "");
    }
    info!(app_id, "Created docs");
    Json(json!({ "success": true, "app_id": app_id })).into_response()
}

/// GET /api/docs/:appId/:section
async fn get_section(Path((app_id, section)): Path<(String, String)>) -> impl IntoResponse {
    if !validate_id(&app_id) || !validate_section(&section) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Invalid params" })),
        )
            .into_response();
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(&app_id);
    let filename = if section == "meta" {
        "meta.json".to_string()
    } else {
        format!("{section}.md")
    };
    let content = std::fs::read_to_string(app_dir.join(&filename)).unwrap_or_default();
    Json(json!({ "success": true, "app_id": app_id, "section": section, "content": content }))
        .into_response()
}

/// POST /api/docs/:appId/:section — update section content
#[derive(Deserialize)]
struct UpdateBody {
    content: String,
}

async fn update_section(
    Path((app_id, section)): Path<(String, String)>,
    Json(body): Json<UpdateBody>,
) -> impl IntoResponse {
    if !validate_id(&app_id) || !validate_section(&section) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Invalid params" })),
        )
            .into_response();
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(&app_id);
    if !app_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&app_dir) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Failed: {e}") })),
            )
                .into_response();
        }
    }
    let filename = if section == "meta" {
        "meta.json".to_string()
    } else {
        format!("{section}.md")
    };
    if let Err(e) = std::fs::write(app_dir.join(&filename), &body.content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": format!("Write failed: {e}") })),
        )
            .into_response();
    }
    info!(app_id, section, "Updated docs section");
    Json(json!({ "success": true })).into_response()
}

/// GET /api/docs/search?q=query
#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search_docs(Query(query): Query<SearchQuery>) -> impl IntoResponse {
    let q = query.q.to_lowercase();
    let dir = std::path::Path::new(DOCS_DIR);
    if !dir.exists() {
        return Json(json!({ "success": true, "results": [] })).into_response();
    }
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Json(json!({ "success": true, "results": [] })).into_response();
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let app_id = entry.file_name().to_string_lossy().to_string();
        let app_dir = entry.path();
        if let Ok(content) = std::fs::read_to_string(app_dir.join("meta.json")) {
            if content.to_lowercase().contains(&q) {
                results.push(json!({ "app_id": app_id, "section": "meta" }));
            }
        }
        for s in SECTIONS {
            if let Ok(content) = std::fs::read_to_string(app_dir.join(format!("{s}.md"))) {
                if content.to_lowercase().contains(&q) {
                    results.push(json!({ "app_id": app_id, "section": s }));
                }
            }
        }
    }
    Json(json!({ "success": true, "results": results, "count": results.len() })).into_response()
}
