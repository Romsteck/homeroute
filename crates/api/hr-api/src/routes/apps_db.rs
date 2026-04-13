//! REST API routes for the per-app managed SQLite database (`hr-apps::DbManager`).
//!
//! Each app gets its own SQLite file at `/opt/homeroute/apps/{slug}/db.sqlite`,
//! exposed via the orchestrator IPC client.

use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{error, info, warn};

use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/apps/{slug}/db/schema", get(get_schema))
        .route("/apps/{slug}/db/sync", post(sync_schema))
        .route(
            "/apps/{slug}/db/tables",
            get(list_tables).post(create_table),
        )
        .route(
            "/apps/{slug}/db/tables/{table}",
            get(describe_table).delete(drop_table),
        )
        .route(
            "/apps/{slug}/db/tables/{table}/rows",
            post(query_rows_endpoint),
        )
        .route(
            "/apps/{slug}/db/tables/{table}/columns",
            post(add_column),
        )
        .route(
            "/apps/{slug}/db/tables/{table}/columns/{col}",
            delete(remove_column),
        )
        .route("/apps/{slug}/db/relations", post(create_relation))
        .route("/apps/{slug}/db/query", post(query_db))
        .route("/apps/{slug}/db/execute", post(execute_db))
}

fn validate_slug(slug: &str) -> Result<(), axum::response::Response> {
    if hr_apps::valid_slug(slug) {
        Ok(())
    } else {
        warn!(slug, "Rejected invalid slug");
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "slug must match ^[a-z][a-z0-9-]*$ (max 64 chars)"})),
        )
            .into_response())
    }
}

fn validate_table_name(table: &str) -> Result<(), axum::response::Response> {
    let valid = !table.is_empty()
        && table.len() <= 64
        && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        warn!(table, "Rejected invalid table name");
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "table name must be alphanumeric/underscore (max 64 chars)"})),
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

// ── Handlers ────────────────────────────────────────────────────

#[tracing::instrument(skip(state))]
async fn list_tables(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let started = Instant::now();
    info!(slug, "Listing app DB tables");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbListTables { slug: slug.clone() })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                "list_tables done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn describe_table(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table_name(&table) {
        return r;
    }
    let started = Instant::now();
    info!(slug, table, "Describing app DB table");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbDescribeTable {
            slug: slug.clone(),
            table: table.clone(),
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                table,
                duration_ms = started.elapsed().as_millis() as u64,
                "describe_table done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[derive(Debug, Deserialize)]
struct QueryRequest {
    sql: String,
    #[serde(default)]
    params: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct QueryRowsRequest {
    #[serde(default)]
    filters: Vec<Value>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    order_desc: Option<bool>,
    #[serde(default)]
    expand: Vec<String>,
}

#[tracing::instrument(skip(state, body))]
async fn query_rows_endpoint(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
    Json(body): Json<QueryRowsRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table_name(&table) {
        return r;
    }
    let started = Instant::now();
    info!(
        slug,
        table,
        filters = body.filters.len(),
        expand = ?body.expand,
        "Query app DB rows (structured)"
    );
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::AppDbQueryRows {
            slug: slug.clone(),
            table: table.clone(),
            filters: body.filters,
            limit: body.limit,
            offset: body.offset,
            order_by: body.order_by,
            order_desc: body.order_desc,
            expand: body.expand,
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                table,
                duration_ms = started.elapsed().as_millis() as u64,
                "query_rows done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn query_db(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<QueryRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if body.sql.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "sql is required"})),
        )
            .into_response();
    }
    let started = Instant::now();
    let sql_preview: String = body.sql.chars().take(120).collect();
    info!(slug, sql = %sql_preview, params = body.params.len(), "Query app DB");
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::AppDbQuery {
            slug: slug.clone(),
            sql: body.sql,
            params: body.params,
        })
        .await
    {
        Ok(resp) => {
            info!(
                slug,
                duration_ms = started.elapsed().as_millis() as u64,
                ok = resp.ok,
                "query_db done"
            );
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn execute_db(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<QueryRequest>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if body.sql.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "sql is required"})),
        )
            .into_response();
    }
    let started = Instant::now();
    info!(slug, sql_preview = %body.sql.chars().take(80).collect::<String>(), "Execute app DB mutation");
    match state
        .orchestrator
        .request_long(&OrchestratorRequest::AppDbExecute {
            slug: slug.clone(),
            sql: body.sql,
            params: body.params,
        })
        .await
    {
        Ok(resp) => {
            info!(slug, duration_ms = started.elapsed().as_millis() as u64, "execute_db done");
            ipc_response(resp)
        }
        Err(e) => ipc_err_response(e),
    }
}

// ── Schema Management ──────────────────────────────────────

#[tracing::instrument(skip(state))]
async fn get_schema(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    info!(slug, "Get full DB schema");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbGetSchema { slug: slug.clone() })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn sync_schema(State(state): State<ApiState>, Path(slug): Path<String>) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    info!(slug, "Sync DB schema");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbSyncSchema { slug: slug.clone() })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn create_table(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    info!(slug, "Create table");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbCreateTable {
            slug: slug.clone(),
            definition: body,
        })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn drop_table(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table_name(&table) {
        return r;
    }
    info!(slug, table, "Drop table");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbDropTable {
            slug: slug.clone(),
            table,
        })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn add_column(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table_name(&table) {
        return r;
    }
    info!(slug, table, "Add column");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbAddColumn {
            slug: slug.clone(),
            table,
            column: body,
        })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state))]
async fn remove_column(
    State(state): State<ApiState>,
    Path((slug, table, col)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table_name(&table) {
        return r;
    }
    info!(slug, table, col, "Remove column");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbRemoveColumn {
            slug: slug.clone(),
            table,
            column: col,
        })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}

#[tracing::instrument(skip(state, body))]
async fn create_relation(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    info!(slug, "Create relation");
    match state
        .orchestrator
        .request(&OrchestratorRequest::AppDbCreateRelation {
            slug: slug.clone(),
            relation: body,
        })
        .await
    {
        Ok(resp) => ipc_response(resp),
        Err(e) => ipc_err_response(e),
    }
}
