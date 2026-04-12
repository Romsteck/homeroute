//! Git repository management: bare repos, commits, branches, mirrors, Smart HTTP.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use hr_ipc::orchestrator::OrchestratorRequest;
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        // Management API
        .route("/repos", get(list_repos))
        .route("/repos/{slug}", get(get_repo))
        .route("/repos/{slug}/commits", get(get_commits))
        .route("/repos/{slug}/branches", get(get_branches))
        // Mirror management (per-repo sync + sync-all)
        .route("/repos/{slug}/mirror/sync", post(trigger_sync))
        .route("/repos/sync-all", post(sync_all))
        // SSH key management
        .route("/ssh-key", get(get_ssh_key).post(generate_ssh_key))
        // Git global config
        .route("/config", get(get_config).put(update_config))
        // Smart HTTP protocol endpoints (large body limit for push)
        .route("/repos/{slug_git}/info/refs", get(git_info_refs))
        .route("/repos/{slug_git}/git-upload-pack", post(git_upload_pack))
        .route("/repos/{slug_git}/git-receive-pack", post(git_receive_pack))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2 GB for git push
}

// ── IPC Helpers ─────────────────────────────────────────────

fn err_ipc(e: impl std::fmt::Display) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": format!("IPC error: {e}")})),
    )
        .into_response()
}

fn err_from_ipc(r: hr_ipc::types::IpcResponse) -> axum::response::Response {
    let msg = r.error.unwrap_or_else(|| "Unknown error".to_string());
    let status = if msg.contains("not found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(json!({"error": msg}))).into_response()
}

// ── Management handlers (via IPC) ──────────────────────────

async fn list_repos(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::ListRepos)
        .await
    {
        Ok(r) if r.ok => {
            let repos = r.data.unwrap_or(json!([]));
            Json(json!({"repos": repos})).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

async fn get_repo(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetRepo { slug })
        .await
    {
        Ok(r) if r.ok => match r.data {
            Some(repo) => Json(json!({"repo": repo})).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Repository not found"})),
            )
                .into_response(),
        },
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

#[derive(Deserialize)]
struct CommitsQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    50
}

async fn get_commits(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Query(q): Query<CommitsQuery>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetCommits {
            slug,
            limit: q.limit,
        })
        .await
    {
        Ok(r) if r.ok => {
            let commits = r.data.unwrap_or(json!([]));
            Json(json!({"commits": commits})).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

async fn get_branches(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetBranches { slug })
        .await
    {
        Ok(r) if r.ok => {
            let branches = r.data.unwrap_or(json!([]));
            Json(json!({"branches": branches})).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

// ── Mirror handlers (via IPC) ──────────────────────────────

async fn trigger_sync(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::TriggerSync { slug })
        .await
    {
        Ok(r) if r.ok => Json(json!({"ok": true})).into_response(),
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

async fn sync_all(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::SyncAll)
        .await
    {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!({"ok": true}));
            Json(data).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

// ── SSH key handlers (via IPC) ─────────────────────────────

async fn get_ssh_key(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetSshKey)
        .await
    {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            Json(data).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

async fn generate_ssh_key(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GenerateSshKey)
        .await
    {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            Json(data).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

// ── Config handlers (via IPC) ──────────────────────────────

async fn get_config(State(state): State<ApiState>) -> impl IntoResponse {
    match state
        .orchestrator
        .request(&OrchestratorRequest::GetGitConfig)
        .await
    {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            Json(data).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

#[derive(Deserialize)]
struct UpdateConfigRequest {
    github_token: Option<String>,
    github_org: Option<String>,
}

async fn update_config(
    State(state): State<ApiState>,
    Json(req): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    let config = json!({
        "github_token": req.github_token,
        "github_org": req.github_org,
    });
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::UpdateGitConfig { config })
        .await
    {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!({"ok": true}));
            Json(data).into_response()
        }
        Ok(r) => err_from_ipc(r),
        Err(e) => err_ipc(e),
    }
}

// ── Smart HTTP protocol handlers (filesystem-only, unchanged) ──

#[derive(Deserialize)]
struct InfoRefsQuery {
    service: String,
}

async fn git_info_refs(
    State(state): State<ApiState>,
    Path(slug_git): Path<String>,
    Query(q): Query<InfoRefsQuery>,
) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    // Strip .git suffix: "myapp.git" -> "myapp"
    let slug = slug_git.strip_suffix(".git").unwrap_or(&slug_git);

    if !git.repo_exists(slug) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path_info = format!("/{slug}.git/info/refs");
    let query_string = format!("service={}", q.service);

    match hr_git::cgi::git_cgi(
        git.repo_path(slug).parent().unwrap(),
        &path_info,
        &query_string,
        "GET",
        "",
        &[],
    )
    .await
    {
        Ok(resp) => {
            let mut builder = axum::http::Response::builder().status(resp.status);
            builder = builder.header(header::CONTENT_TYPE, &resp.content_type);
            builder = builder.header(header::CACHE_CONTROL, "no-cache");
            for (k, v) in &resp.headers {
                let lower = k.to_lowercase();
                if lower != "content-type" && lower != "status" {
                    builder = builder.header(k.as_str(), v.as_str());
                }
            }
            builder
                .body(axum::body::Body::from(resp.body))
                .unwrap()
                .into_response()
        }
        Err(e) => {
            error!("git-http-backend error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn git_upload_pack(
    State(state): State<ApiState>,
    Path(slug_git): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let slug = slug_git.strip_suffix(".git").unwrap_or(&slug_git);

    if !git.repo_exists(slug) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let path_info = format!("/{slug}.git/git-upload-pack");

    match hr_git::cgi::git_cgi(
        git.repo_path(slug).parent().unwrap(),
        &path_info,
        "",
        "POST",
        content_type,
        &body,
    )
    .await
    {
        Ok(resp) => {
            let mut builder = axum::http::Response::builder().status(resp.status);
            builder = builder.header(header::CONTENT_TYPE, &resp.content_type);
            for (k, v) in &resp.headers {
                let lower = k.to_lowercase();
                if lower != "content-type" && lower != "status" {
                    builder = builder.header(k.as_str(), v.as_str());
                }
            }
            builder
                .body(axum::body::Body::from(resp.body))
                .unwrap()
                .into_response()
        }
        Err(e) => {
            error!("git-upload-pack error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn git_receive_pack(
    State(state): State<ApiState>,
    Path(slug_git): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let slug = slug_git.strip_suffix(".git").unwrap_or(&slug_git);

    if !git.repo_exists(slug) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let path_info = format!("/{slug}.git/git-receive-pack");

    match hr_git::cgi::git_cgi(
        git.repo_path(slug).parent().unwrap(),
        &path_info,
        "",
        "POST",
        content_type,
        &body,
    )
    .await
    {
        Ok(resp) => {
            let mut builder = axum::http::Response::builder().status(resp.status);
            builder = builder.header(header::CONTENT_TYPE, &resp.content_type);
            for (k, v) in &resp.headers {
                let lower = k.to_lowercase();
                if lower != "content-type" && lower != "status" {
                    builder = builder.header(k.as_str(), v.as_str());
                }
            }
            builder
                .body(axum::body::Body::from(resp.body))
                .unwrap()
                .into_response()
        }
        Err(e) => {
            error!("git-receive-pack error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
