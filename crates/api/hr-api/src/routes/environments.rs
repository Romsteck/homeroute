//! REST API routes for Maker Portal: environments and pipelines.
//! Environment handlers delegate to hr-orchestrator via IPC (OrchestratorClient).
//! Pipeline handlers proxy to the orchestrator MCP endpoint.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tracing::{error, info};

use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/environments", get(list_environments).post(create_environment))
        .route("/environments/{slug}", get(get_environment).put(update_environment).delete(destroy_environment))
        .route("/environments/{slug}/start", post(start_environment))
        .route("/environments/{slug}/stop", post(stop_environment))
        .route("/environments/{slug}/apps", get(get_environment_apps))
        .route("/environments/{slug}/monitoring", get(get_environment_monitoring))
        .route("/environments/{slug}/apps/{app_slug}/control", post(control_app))
        .route("/environments/{slug}/apps/{app_slug}/logs", get(get_app_logs))
        .route("/environments/{slug}/db/tables", get(get_db_tables))
        .route("/environments/{slug}/db/schema", get(get_db_schema))
        .route("/environments/{slug}/db/query", get(query_db_data))
        .route("/environments/{slug}/db/count", get(count_db_rows))
        .route("/environments/{slug}/db/rows", post(insert_db_rows).put(update_db_rows).delete(delete_db_rows))
        .route("/monitoring/envs", get(monitoring_envs_summary))
        .route("/pipelines/promote", post(promote_pipeline))
        .route("/pipelines/definitions", get(list_pipeline_definitions))
        .route("/pipelines", get(list_pipelines))
        .route("/pipelines/{id}", get(get_pipeline))
        .route("/pipelines/{id}/cancel", post(cancel_pipeline))
}

// ── Helpers ─────────────────────────────────────────────────────

fn ipc_ok_response(resp: hr_ipc::types::IpcResponse) -> axum::response::Response {
    if resp.ok {
        Json(serde_json::json!({"success": true, "data": resp.data})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response()
    }
}

fn ipc_err_response(e: anyhow::Error) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"success": false, "error": format!("Orchestrator unavailable: {e}")})),
    )
        .into_response()
}

/// POST to orchestrator MCP endpoint and return the tool result.
async fn mcp_call(tool: &str, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
    let mcp_token = std::env::var("MCP_TOKEN").unwrap_or_default();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": arguments,
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:4001/mcp")
        .header("Authorization", format!("Bearer {}", mcp_token))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("MCP request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("MCP returned status {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse MCP response: {e}"))?;

    // JSON-RPC: check for error
    if let Some(err) = json.get("error") {
        return Err(format!("MCP error: {}", err));
    }

    // Extract result content
    Ok(json.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// POST to an env-agent MCP endpoint (resolves env IP first).
/// Used for tools that live on the env-agent, not the orchestrator (app.logs, app.control, etc.)
async fn env_mcp_call(
    state: &ApiState,
    env_slug: &str,
    tool: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Resolve env IP via IPC
    let resp = state
        .orchestrator
        .request(&OrchestratorRequest::GetEnvironment { id: env_slug.to_string() })
        .await
        .map_err(|e| format!("IPC error: {e}"))?;

    if !resp.ok {
        return Err(format!("Environment '{}' not found", env_slug));
    }

    let env_ip = resp.data
        .as_ref()
        .and_then(|d| d.get("ipv4_address"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("No IP for environment '{}'", env_slug))?;

    let url = format!("http://{}:4010/mcp", env_ip);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": arguments,
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Env-agent request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Env-agent returned status {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse env-agent response: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("Env-agent error: {}", err));
    }

    // Extract text content from JSON-RPC result
    let result = json.get("result").cloned().unwrap_or(serde_json::Value::Null);
    if let Some(text) = result.pointer("/content/0/text").and_then(|t| t.as_str()) {
        // Try to parse text as JSON
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
            return Ok(parsed);
        }
        return Ok(serde_json::Value::String(text.to_string()));
    }
    Ok(result)
}

// ── Environment handlers ────────────────────────────────────────

/// GET /api/environments
async fn list_environments(State(state): State<ApiState>) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::ListEnvironments).await {
        Ok(resp) if resp.ok => {
            Json(serde_json::json!({"success": true, "environments": resp.data})).into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

/// GET /api/environments/:slug
async fn get_environment(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::GetEnvironment { id: slug }).await {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Deserialize)]
struct CreateEnvironmentRequest {
    name: String,
    slug: String,
    env_type: Option<String>,
    host_id: Option<String>,
}

/// POST /api/environments
async fn create_environment(
    State(state): State<ApiState>,
    Json(req): Json<CreateEnvironmentRequest>,
) -> impl IntoResponse {
    let request = serde_json::json!({
        "name": req.name,
        "slug": req.slug,
        "env_type": req.env_type.unwrap_or_else(|| "dev".to_string()),
        "host_id": req.host_id,
    });

    info!(slug = req.slug, "Creating environment");

    match state
        .orchestrator
        .request(&OrchestratorRequest::CreateEnvironment { request })
        .await
    {
        Ok(resp) if resp.ok => (
            StatusCode::CREATED,
            Json(serde_json::json!({"success": true, "data": resp.data})),
        )
            .into_response(),
        Ok(resp) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

/// POST /api/environments/:slug/start
async fn start_environment(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    info!(slug = slug, "Starting environment");
    match state.orchestrator.request(&OrchestratorRequest::StartEnvironment { id: slug }).await {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

/// POST /api/environments/:slug/stop
async fn stop_environment(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    info!(slug = slug, "Stopping environment");
    match state.orchestrator.request(&OrchestratorRequest::StopEnvironment { id: slug }).await {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

/// DELETE /api/environments/:slug
async fn destroy_environment(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    info!(slug = slug, "Destroying environment");
    match state.orchestrator.request(&OrchestratorRequest::DeleteEnvironment { id: slug }).await {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Deserialize)]
struct UpdateEnvironmentRequest {
    name: Option<String>,
    slug: Option<String>,
}

/// PUT /api/environments/:slug
async fn update_environment(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(req): Json<UpdateEnvironmentRequest>,
) -> impl IntoResponse {
    let request = serde_json::json!({
        "name": req.name,
        "slug": req.slug,
    });

    info!(slug = slug, "Updating environment");

    match state
        .orchestrator
        .request(&OrchestratorRequest::UpdateEnvironment { id: slug, request })
        .await
    {
        Ok(resp) => ipc_ok_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

// ── Environment apps & monitoring handlers ──────────────────────

/// GET /api/environments/:slug/apps
async fn get_environment_apps(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    // Get the environment via IPC and extract apps from the response
    match state.orchestrator.request(&OrchestratorRequest::GetEnvironment { id: slug.clone() }).await {
        Ok(resp) if resp.ok => {
            // Extract apps array from environment data
            let apps = resp.data
                .as_ref()
                .and_then(|d| d.get("apps").cloned())
                .unwrap_or(serde_json::Value::Array(vec![]));
            Json(serde_json::json!({"success": true, "apps": apps})).into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

/// GET /api/environments/:slug/monitoring
async fn get_environment_monitoring(Path(slug): Path<String>) -> impl IntoResponse {
    match mcp_call("monitoring.app_health", serde_json::json!({"env_slug": slug})).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Environment monitoring failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ControlAppRequest {
    action: String,
}

/// POST /api/environments/:slug/apps/:app_slug/control
async fn control_app(
    State(state): State<ApiState>,
    Path((slug, app_slug)): Path<(String, String)>,
    Json(req): Json<ControlAppRequest>,
) -> impl IntoResponse {
    info!(env = slug, app = app_slug, action = req.action, "Controlling app in environment");
    let tool = format!("app.{}", req.action);
    match env_mcp_call(
        &state,
        &slug,
        &tool,
        serde_json::json!({
            "slug": app_slug,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("App control failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct AppLogsQuery {
    lines: Option<u32>,
}

/// GET /api/environments/:slug/apps/:app_slug/logs
async fn get_app_logs(
    State(state): State<ApiState>,
    Path((slug, app_slug)): Path<(String, String)>,
    Query(query): Query<AppLogsQuery>,
) -> impl IntoResponse {
    let lines = query.lines.unwrap_or(100);
    match env_mcp_call(
        &state,
        &slug,
        "app.logs",
        serde_json::json!({
            "slug": app_slug,
            "lines": lines,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("App logs failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

/// GET /api/monitoring/envs — Cross-environment monitoring summary
async fn monitoring_envs_summary(State(state): State<ApiState>) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::ListEnvironments).await {
        Ok(resp) if resp.ok => {
            // Return the environments list as a summary — each env already has status info
            let envs = resp.data.unwrap_or(serde_json::Value::Array(vec![]));
            Json(serde_json::json!({"success": true, "environments": envs})).into_response()
        }
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": resp.error})),
        )
            .into_response(),
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Deserialize)]
struct DbTablesParams {
    app_slug: Option<String>,
}

/// GET /api/environments/:slug/db/tables
async fn get_db_tables(
    Path(slug): Path<String>,
    Query(params): Query<DbTablesParams>,
) -> impl IntoResponse {
    let mut args = serde_json::json!({"env_slug": slug});
    if let Some(app) = &params.app_slug {
        args["app_slug"] = serde_json::json!(app);
    }
    match mcp_call("db.list_tables", args).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB list tables failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct DbSchemaParams {
    table: String,
    app_slug: Option<String>,
}

/// GET /api/environments/:slug/db/schema?table=...
async fn get_db_schema(
    Path(slug): Path<String>,
    Query(params): Query<DbSchemaParams>,
) -> impl IntoResponse {
    let mut args = serde_json::json!({
        "env_slug": slug,
        "table": params.table,
    });
    if let Some(app) = &params.app_slug {
        args["app_slug"] = serde_json::json!(app);
    }
    match mcp_call("db.get_schema", args).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB schema failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct DbQueryParams {
    table: String,
    app_slug: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    order_by: Option<String>,
    order_desc: Option<bool>,
    /// Filters as JSON string: [{"column":"name","op":"like","value":"%test%"}]
    filters: Option<String>,
}

/// GET /api/environments/:slug/db/query
async fn query_db_data(
    Path(slug): Path<String>,
    Query(params): Query<DbQueryParams>,
) -> impl IntoResponse {
    let mut args = serde_json::json!({
        "env_slug": slug,
        "table": params.table,
        "limit": params.limit.unwrap_or(50),
        "offset": params.offset.unwrap_or(0),
    });
    if let Some(app) = &params.app_slug {
        args["app_slug"] = serde_json::json!(app);
    }
    if let Some(order) = &params.order_by {
        args["order_by"] = serde_json::json!(order);
    }
    if let Some(desc) = params.order_desc {
        args["order_desc"] = serde_json::json!(desc);
    }
    if let Some(filters_str) = &params.filters {
        if let Ok(filters) = serde_json::from_str::<serde_json::Value>(filters_str) {
            args["filters"] = filters;
        }
    }
    match mcp_call("db.query_data", args).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB query failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

// ── DB write handlers ──────────────────────────────────────────

#[derive(Deserialize)]
struct CountParams {
    table: String,
    app_slug: Option<String>,
    filters: Option<String>,
}

/// GET /api/environments/:slug/db/count
async fn count_db_rows(
    Path(slug): Path<String>,
    Query(params): Query<CountParams>,
) -> impl IntoResponse {
    let mut args = serde_json::json!({
        "env_slug": slug,
        "table_name": params.table,
    });
    if let Some(app) = &params.app_slug {
        args["app_slug"] = serde_json::json!(app);
    }
    if let Some(filters_str) = &params.filters {
        if let Ok(filters) = serde_json::from_str::<serde_json::Value>(filters_str) {
            args["filters"] = filters;
        }
    }
    match mcp_call("db.count_rows", args).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB count rows failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct InsertRowsRequest {
    app_slug: String,
    table: String,
    rows: Vec<serde_json::Value>,
}

/// POST /api/environments/:slug/db/rows
async fn insert_db_rows(
    Path(slug): Path<String>,
    Json(req): Json<InsertRowsRequest>,
) -> impl IntoResponse {
    match mcp_call(
        "db.insert_data",
        serde_json::json!({
            "env_slug": slug,
            "app_slug": req.app_slug,
            "table_name": req.table,
            "rows": req.rows,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB insert failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct UpdateRowsRequest {
    app_slug: String,
    table: String,
    updates: serde_json::Value,
    filters: Vec<serde_json::Value>,
}

/// PUT /api/environments/:slug/db/rows
async fn update_db_rows(
    Path(slug): Path<String>,
    Json(req): Json<UpdateRowsRequest>,
) -> impl IntoResponse {
    match mcp_call(
        "db.update_data",
        serde_json::json!({
            "env_slug": slug,
            "app_slug": req.app_slug,
            "table_name": req.table,
            "updates": req.updates,
            "filters": req.filters,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB update failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct DeleteRowsRequest {
    app_slug: String,
    table: String,
    filters: Vec<serde_json::Value>,
}

/// DELETE /api/environments/:slug/db/rows
async fn delete_db_rows(
    Path(slug): Path<String>,
    Json(req): Json<DeleteRowsRequest>,
) -> impl IntoResponse {
    match mcp_call(
        "db.delete_data",
        serde_json::json!({
            "env_slug": slug,
            "app_slug": req.app_slug,
            "table_name": req.table,
            "filters": req.filters,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("DB delete failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

// ── Pipeline handlers (MCP proxy) ──────────────────────────────

#[derive(Deserialize)]
struct PromoteRequest {
    app_slug: String,
    version: String,
    source_env: String,
    target_env: String,
}

/// POST /api/pipelines/promote
async fn promote_pipeline(Json(req): Json<PromoteRequest>) -> impl IntoResponse {
    match mcp_call(
        "pipeline.promote",
        serde_json::json!({
            "app_slug": req.app_slug,
            "version": req.version,
            "source_env": req.source_env,
            "target_env": req.target_env,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Pipeline promote failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

/// GET /api/pipelines
async fn list_pipelines() -> impl IntoResponse {
    match mcp_call("pipeline.history", serde_json::json!({})).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Pipeline list failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

/// GET /api/pipelines/:id
async fn get_pipeline(Path(id): Path<String>) -> impl IntoResponse {
    match mcp_call("pipeline.status", serde_json::json!({"pipeline_id": id})).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Pipeline get failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

/// POST /api/pipelines/:id/cancel
async fn cancel_pipeline(Path(id): Path<String>) -> impl IntoResponse {
    match mcp_call("pipeline.cancel", serde_json::json!({"pipeline_id": id})).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Pipeline cancel failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}

/// GET /api/pipelines/definitions
async fn list_pipeline_definitions() -> impl IntoResponse {
    match mcp_call("pipeline.definitions", serde_json::json!({})).await {
        Ok(result) => Json(serde_json::json!({"success": true, "data": result})).into_response(),
        Err(e) => {
            error!("Pipeline definitions list failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"success": false, "error": e})),
            )
                .into_response()
        }
    }
}
