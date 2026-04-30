//! REST API routes for managing applications via hr-apps (V3 model).
//!
//! All handlers delegate to hr-orchestrator over IPC. The orchestrator hosts the
//! `AppSupervisor` (from `hr-apps`) which owns the per-app process lifecycle, the
//! managed SQLite databases, and the Claude context generator.

use std::collections::BTreeMap;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info, warn};

use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/apps", get(list_apps).post(create_app))
        .route(
            "/apps/{slug}",
            get(get_app).patch(update_app).delete(delete_app),
        )
        .route("/apps/{slug}/control", post(control_app))
        .route("/apps/{slug}/build", post(build_app))
        .route("/apps/{slug}/deploy", post(build_app))
        .route("/apps/{slug}/ship", post(ship_app))
        .route("/apps/{slug}/build-event", post(emit_build_event))
        .route("/apps/{slug}/status", get(app_status))
        .route("/apps/{slug}/logs", get(app_logs))
        .route("/apps/{slug}/exec", post(app_exec))
        .route("/apps/{slug}/env", get(get_app_env).put(update_app_env))
        .route("/apps/{slug}/regenerate-context", post(regenerate_context))
        .route("/apps/{slug}/todos", get(app_todos))
}

// ── Helpers ─────────────────────────────────────────────────────

fn validate_slug(slug: &str) -> Result<(), axum::response::Response> {
    if hr_apps::valid_slug(slug) {
        Ok(())
    } else {
        warn!(slug, "Rejected invalid slug");
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "slug must match ^[a-z][a-z0-9-]*$ (max 64 chars)"
            })),
        )
            .into_response())
    }
}

fn ipc_err_response(e: anyhow::Error) -> axum::response::Response {
    error!(error = %e, "Orchestrator IPC error");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"success": false, "error": format!("Orchestrator unavailable: {e}")})),
    )
        .into_response()
}

fn ipc_response(resp: hr_ipc::types::IpcResponse) -> axum::response::Response {
    if resp.ok {
        Json(json!({"success": true, "data": resp.data})).into_response()
    } else {
        let err = resp.error.unwrap_or_else(|| "unknown error".into());
        let status = if err.to_lowercase().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, Json(json!({"success": false, "error": err}))).into_response()
    }
}

// ── List / Get ──────────────────────────────────────────────────

#[tracing::instrument(skip(state))]
async fn list_apps(State(state): State<ApiState>) -> impl IntoResponse {
    let started = Instant::now();
    info!("Listing applications");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppList)
        .await
    {
        Ok(resp) => {
            info!(
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "list_apps done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn get_app(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Fetching application");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppGet { slug: slug.clone() })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "get_app done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Create / Update / Delete ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateAppRequest {
    slug: String,
    name: String,
    stack: String,
    #[serde(default = "default_visibility")]
    visibility: String,
    #[serde(default)]
    run_command: Option<String>,
    #[serde(default)]
    build_command: Option<String>,
    #[serde(default)]
    health_path: Option<String>,
    #[serde(default)]
    build_artefact: Option<String>,
}

fn default_visibility() -> String {
    "private".to_string()
}

#[tracing::instrument(skip(state, body), fields(slug = %body.slug))]
async fn create_app(
    State(state): State<ApiState>,
    Json(body): Json<CreateAppRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&body.slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug = %body.slug, stack = %body.stack, "Creating application");
    let req = OrchestratorRequest::AppCreate {
        slug: body.slug.clone(),
        name: body.name,
        stack: body.stack,
        has_db: true,
        visibility: body.visibility,
        run_command: body.run_command,
        build_command: body.build_command,
        health_path: body.health_path,
        build_artefact: body.build_artefact,
    };
    match state.orchestrator.request_long(&req).await {
        Ok(resp) => {
            info!(
                slug = %body.slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "create_app done"
            );
            if resp.ok {
                (
                    StatusCode::CREATED,
                    Json(json!({"success": true, "data": resp.data})),
                )
                    .into_response()
            } else {
                ipc_response(resp)
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateAppRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    run_command: Option<String>,
    #[serde(default)]
    build_command: Option<String>,
    #[serde(default)]
    health_path: Option<String>,
    #[serde(default)]
    env_vars: Option<BTreeMap<String, String>>,
    #[serde(default)]
    has_db: Option<bool>,
    #[serde(default)]
    build_artefact: Option<String>,
}

#[tracing::instrument(skip(state, body))]
async fn update_app(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<UpdateAppRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Updating application");
    let req = OrchestratorRequest::AppUpdate {
        slug: slug.clone(),
        name: body.name,
        visibility: body.visibility,
        run_command: body.run_command,
        build_command: body.build_command,
        health_path: body.health_path,
        env_vars: body.env_vars,
        has_db: body.has_db,
        build_artefact: body.build_artefact,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "update_app done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Debug, Deserialize, Default)]
struct DeleteAppQuery {
    #[serde(default)]
    keep_data: bool,
}

#[tracing::instrument(skip(state))]
async fn delete_app(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Query(q): Query<DeleteAppQuery>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, keep_data = q.keep_data, "Deleting application");
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::AppDelete {
            slug: slug.clone(),
            keep_data: q.keep_data,
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "delete_app done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Control / Status / Logs / Exec ──────────────────────────────

#[derive(Debug, Deserialize)]
struct ControlRequest {
    action: String,
}

#[tracing::instrument(skip(state, body))]
async fn control_app(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<ControlRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let action = body.action.to_lowercase();
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        warn!(slug, action, "Rejected invalid control action");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "action must be one of: start, stop, restart (use POST /apps/{slug}/build for builds)"})),
        )
            .into_response();
    }
    let started = Instant::now();
    info!(slug, action, "Control app");
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::AppControl {
            slug: slug.clone(),
            action: action.clone(),
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                action,
                duration_ms = started.elapsed().as_millis() as u64,
                "control_app done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn app_todos(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Fetching app todos");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppTodosList {
            slug: slug.clone(),
            status: None,
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "app_todos done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn app_status(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Fetching app status");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppStatus { slug: slug.clone() })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "app_status done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Debug, Deserialize, Default)]
struct LogsQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    level: Option<String>,
}

#[tracing::instrument(skip(state))]
async fn app_logs(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Query(q): Query<LogsQuery>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, limit = ?q.limit, level = ?q.level, "Fetching app logs");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppLogs {
            slug: slug.clone(),
            limit: q.limit,
            level: q.level,
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "app_logs done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Debug, Deserialize)]
struct ExecRequest {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[tracing::instrument(skip(state, body))]
async fn app_exec(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<ExecRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if body.command.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "command is required"})),
        )
            .into_response();
    }
    let started = Instant::now();
    let cmd_preview: String = body.command.chars().take(120).collect();
    info!(slug, cmd = %cmd_preview, timeout_secs = ?body.timeout_secs, "Exec in app");
    let timeout_secs = body.timeout_secs.unwrap_or(60).clamp(1, 600);
    let req = OrchestratorRequest::AppExec {
        slug: slug.clone(),
        command: body.command,
        timeout_secs: Some(timeout_secs),
    };
    let timeout = std::time::Duration::from_secs(timeout_secs + 5);
    match state.orchestrator.request_with_timeout(&req, timeout).await {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "app_exec done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Build ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct BuildRequest {
    #[serde(default)]
    timeout_secs: Option<u64>,
}

const BUILD_DEFAULT_TIMEOUT_SECS: u64 = 1800;
const BUILD_MIN_TIMEOUT_SECS: u64 = 60;
const BUILD_MAX_TIMEOUT_SECS: u64 = 7200;

#[tracing::instrument(skip(state, body))]
async fn build_app(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<BuildRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let timeout_secs = body
        .timeout_secs
        .unwrap_or(BUILD_DEFAULT_TIMEOUT_SECS)
        .clamp(BUILD_MIN_TIMEOUT_SECS, BUILD_MAX_TIMEOUT_SECS);
    let started = Instant::now();
    info!(slug, timeout_secs, "Building application");
    // IPC timeout = build timeout + 60s grace so the IPC layer never fires
    // before the build pipeline itself times out (which has a clearer error).
    let ipc_timeout = std::time::Duration::from_secs(timeout_secs + 60);
    let req = OrchestratorRequest::AppBuild {
        slug: slug.clone(),
        timeout_secs: Some(timeout_secs),
    };
    match state.orchestrator.request_with_timeout(&req, ipc_timeout).await {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "build_app done"
            );
            if resp.ok {
                Json(json!({"success": true, "data": resp.data})).into_response()
            } else {
                let err = resp.error.unwrap_or_else(|| "unknown error".into());
                if err.contains("BUILD_BUSY") {
                    warn!(slug, "build_app conflict: BUILD_BUSY");
                    (
                        StatusCode::CONFLICT,
                        Json(json!({
                            "success": false,
                            "error": "BUILD_BUSY",
                            "message": err,
                        })),
                    )
                        .into_response()
                } else {
                    error!(slug, error = %err, "build_app failed");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"success": false, "error": err})),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Ship (rsync pre-built artefact + restart, no compile) ─────────

const SHIP_DEFAULT_TIMEOUT_SECS: u64 = 900;

#[tracing::instrument(skip(state, body))]
async fn ship_app(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<BuildRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let timeout_secs = body
        .timeout_secs
        .unwrap_or(SHIP_DEFAULT_TIMEOUT_SECS)
        .clamp(BUILD_MIN_TIMEOUT_SECS, BUILD_MAX_TIMEOUT_SECS);
    let started = Instant::now();
    info!(slug, timeout_secs, "Shipping pre-built artefacts");
    let ipc_timeout = std::time::Duration::from_secs(timeout_secs + 60);
    let req = OrchestratorRequest::AppShip {
        slug: slug.clone(),
        timeout_secs: Some(timeout_secs),
    };
    match state.orchestrator.request_with_timeout(&req, ipc_timeout).await {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "ship_app done"
            );
            if resp.ok {
                Json(json!({"success": true, "data": resp.data})).into_response()
            } else {
                let err = resp.error.unwrap_or_else(|| "unknown error".into());
                if err.contains("BUILD_BUSY") {
                    (
                        StatusCode::CONFLICT,
                        Json(json!({"success": false, "error": "BUILD_BUSY", "message": err})),
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"success": false, "error": err})),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Build event broadcast (used by local app-build skill) ─────────

#[derive(Debug, Deserialize)]
struct BuildEventRequest {
    status: String,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    step: Option<u32>,
    #[serde(default)]
    total_steps: Option<u32>,
}

#[tracing::instrument(skip(state, body))]
async fn emit_build_event(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<BuildEventRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let req = OrchestratorRequest::AppEmitBuildEvent {
        slug: slug.clone(),
        status: body.status,
        phase: body.phase,
        message: body.message,
        duration_ms: body.duration_ms,
        error: body.error,
        step: body.step,
        total_steps: body.total_steps,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

// ── Env vars ─────────────────────────────────────────────────────

#[tracing::instrument(skip(state))]
async fn get_app_env(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    info!(slug, "Fetching app env vars");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppGet { slug: slug.clone() })
        .await
    {
        Ok(resp) if resp.ok => {
            let env_vars = resp
                .data
                .as_ref()
                .and_then(|d| d.get("env_vars"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            Json(json!({"success": true, "data": env_vars})).into_response()
        }
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn update_app_env(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<BTreeMap<String, String>>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, count = body.len(), "Updating app env vars");
    let req = OrchestratorRequest::AppUpdate {
        slug: slug.clone(),
        name: None,
        visibility: None,
        run_command: None,
        build_command: None,
        health_path: None,
        env_vars: Some(body),
        has_db: None,
        build_artefact: None,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "update_app_env done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Claude context regen ────────────────────────────────────────

#[tracing::instrument(skip(state))]
async fn regenerate_context(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Regenerating Claude context file");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppRegenerateContext { slug: slug.clone() })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "regenerate_context done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}
