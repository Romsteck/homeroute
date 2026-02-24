//! Git repository management: bare repos, commits, branches, mirrors, Smart HTTP.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
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
        .route(
            "/repos/{slug_git}/git-receive-pack",
            post(git_receive_pack),
        )
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024)) // 512 MB for git push
}

// ── Helpers ──────────────────────────────────────────────────

fn err_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Git service unavailable"})),
    )
        .into_response()
}

fn err_internal(e: impl std::fmt::Display) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": e.to_string()})),
    )
        .into_response()
}

// ── Management handlers ─────────────────────────────────────

async fn list_repos(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.list_repos().await {
        Ok(repos) => Json(json!({"repos": repos})).into_response(),
        Err(e) => err_internal(e),
    }
}

async fn get_repo(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.get_repo(&slug).await {
        Ok(Some(repo)) => Json(json!({"repo": repo})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Repository not found"})),
        )
            .into_response(),
        Err(e) => err_internal(e),
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
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.get_commits(&slug, q.limit).await {
        Ok(commits) => Json(json!({"commits": commits})).into_response(),
        Err(e) => err_internal(e),
    }
}

async fn get_branches(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.get_branches(&slug).await {
        Ok(branches) => Json(json!({"branches": branches})).into_response(),
        Err(e) => err_internal(e),
    }
}

// ── Mirror handlers ─────────────────────────────────────────

async fn trigger_sync(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.trigger_sync(&slug).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => err_internal(e),
    }
}

async fn sync_all(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };

    let config = match git.load_config().await {
        Ok(c) => c,
        Err(e) => return err_internal(e),
    };

    let mut synced = Vec::new();
    let mut errors = Vec::new();

    for (slug, mirror) in &config.mirrors {
        if !mirror.enabled {
            continue;
        }
        match git.trigger_sync(slug).await {
            Ok(()) => synced.push(slug.clone()),
            Err(e) => {
                error!(slug = %slug, error = %e, "sync-all: mirror sync failed");
                errors.push(json!({"slug": slug, "error": e.to_string()}));
            }
        }
    }

    // Reload config to get updated last_sync/last_error values
    let updated_mirrors = git.load_config().await.ok().map(|c| c.mirrors);

    Json(json!({
        "ok": errors.is_empty(),
        "synced": synced,
        "errors": errors,
        "mirrors": updated_mirrors,
    }))
    .into_response()
}

// ── SSH key handlers ────────────────────────────────────────

async fn get_ssh_key(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.get_ssh_key().await {
        Ok(key) => Json(json!(key)).into_response(),
        Err(e) => err_internal(e),
    }
}

async fn generate_ssh_key(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.generate_ssh_key().await {
        Ok(key) => Json(json!(key)).into_response(),
        Err(e) => err_internal(e),
    }
}

// ── Config handlers ─────────────────────────────────────────

async fn get_config(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    match git.load_config().await {
        Ok(config) => {
            // Mask the GitHub token in the response
            let masked_token = config.github_token.as_ref().map(|t| {
                if t.len() > 8 {
                    format!("{}...{}", &t[..4], &t[t.len() - 4..])
                } else {
                    "****".to_string()
                }
            });
            Json(json!({
                "github_token": masked_token,
                "github_org": config.github_org,
                "mirrors": config.mirrors,
            }))
            .into_response()
        }
        Err(e) => err_internal(e),
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
    let Some(ref git) = state.git else {
        return err_unavailable();
    };
    let mut config = match git.load_config().await {
        Ok(c) => c,
        Err(e) => return err_internal(e),
    };

    if let Some(token) = req.github_token {
        // Ignore masked tokens (returned by GET /config) to avoid overwriting the real one
        if !token.contains("...") && !token.contains("****") {
            config.github_token = Some(token);
        }
    }
    if let Some(org) = req.github_org {
        config.github_org = org;
    }

    // Auto-enable mirrors for ALL existing repos when org+token are both set
    let has_org = !config.github_org.is_empty();
    let has_token = config
        .github_token
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false);

    let mut mirror_errors = Vec::new();

    if has_org && has_token {
        let token = config.github_token.clone().unwrap();
        let org = config.github_org.clone();
        let gh = hr_git::github::GitHubClient::new(token);

        let repos = match git.list_repos().await {
            Ok(r) => r,
            Err(e) => return err_internal(e),
        };

        for repo in &repos {
            let slug = &repo.slug;

            // Create GitHub repo if it doesn't exist
            match gh.repo_exists(&org, slug).await {
                Ok(false) => {
                    if let Err(e) = gh.create_repo(slug, Some(&org), true).await {
                        error!(slug = %slug, error = %e, "Failed to create GitHub repo");
                        mirror_errors.push(json!({"slug": slug, "error": e.to_string()}));
                        continue;
                    }
                }
                Ok(true) => {}
                Err(e) => {
                    error!(slug = %slug, error = %e, "Failed to check GitHub repo");
                    mirror_errors.push(json!({"slug": slug, "error": e.to_string()}));
                    continue;
                }
            }

            // Enable mirror in the bare repo
            if let Err(e) = git.enable_mirror(slug, &org).await {
                error!(slug = %slug, error = %e, "Failed to enable mirror");
                mirror_errors.push(json!({"slug": slug, "error": e.to_string()}));
                continue;
            }

            // Save mirror config
            let ssh_url = format!("git@github.com:{org}/{slug}.git");
            config.mirrors.insert(
                slug.clone(),
                hr_git::types::MirrorConfig {
                    enabled: true,
                    github_ssh_url: Some(ssh_url),
                    visibility: hr_git::types::RepoVisibility::Private,
                    last_sync: None,
                    last_error: None,
                },
            );
        }
    }

    match git.save_config(&config).await {
        Ok(()) => Json(json!({
            "ok": mirror_errors.is_empty(),
            "mirror_errors": mirror_errors,
        }))
        .into_response(),
        Err(e) => err_internal(e),
    }
}

// ── Smart HTTP protocol handlers ────────────────────────────

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
