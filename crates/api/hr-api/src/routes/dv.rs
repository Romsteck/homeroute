//! Routes REST OData-like pour le **Dataverse Gateway**.
//!
//! Surface (toutes sous `/api/dv/{slug}/...`) :
//! - `GET    /$schema`
//! - `GET    /{table}` (liste, OData $filter/$select/$orderby/$top/$skip/$count/$includeDeleted)
//! - `GET    /{table}/{id}` (single, ETag = version)
//! - `POST   /{table}` (insert)
//! - `PATCH  /{table}/{id}` (update, `If-Match: <version>` requis)
//! - `DELETE /{table}/{id}` (soft-delete, `If-Match` requis)
//! - `POST   /{table}/$restore/{id}` (restore, `If-Match` requis)
//! - `GET    /$audit`
//! - `POST   /$schema/rotate-token` (admin)
//!
//! L'identité de l'appelant est extraite des en-têtes :
//! - `X-Remote-User-Id` (forward-auth, propagé par hr-edge) → [`Identity::User`]
//! - `Authorization: Bearer <token>` → résolu en [`Identity::App`] via
//!   `DataverseManager::verify_token`
//!
//! En l'absence des deux, la réponse est 401.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hr_common::Identity;
use hr_ipc::orchestrator::OrchestratorRequest;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/dv/{slug}/$schema", get(get_schema))
        .route("/dv/{slug}/$schema/rotate-token", post(rotate_token))
        .route("/dv/{slug}/$audit", get(audit_list))
        .route(
            "/dv/{slug}/{table}",
            get(list_rows).post(insert_row),
        )
        .route(
            "/dv/{slug}/{table}/{id}",
            get(get_row).patch(update_row).delete(soft_delete_row),
        )
        .route(
            "/dv/{slug}/{table}/$restore/{id}",
            post(restore_row),
        )
}

// ── Identity extraction ─────────────────────────────────────────────────

async fn extract_identity(
    headers: &HeaderMap,
    state: &ApiState,
    slug: &str,
) -> Result<Identity, Response> {
    // 1. Bearer token → app identity (verified via IPC against
    //    `dataverse-secrets.json` owned by hr-orchestrator).
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ").map(str::trim) {
                if !token.is_empty() {
                    let resp = state
                        .orchestrator
                        .request(&OrchestratorRequest::DvVerifyToken {
                            slug: slug.to_string(),
                            token: token.to_string(),
                        })
                        .await
                        .map_err(ipc_err)?;
                    if !resp.ok {
                        return Err(error_resp(
                            StatusCode::UNAUTHORIZED,
                            "invalid bearer token",
                        ));
                    }
                    let uuid_str = resp
                        .data
                        .as_ref()
                        .and_then(|v| v.get("app_uuid"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let app_uuid = Uuid::parse_str(uuid_str).map_err(|_| {
                        error_resp(StatusCode::UNAUTHORIZED, "invalid bearer token")
                    })?;
                    return Ok(Identity::app(app_uuid, slug.to_string()));
                }
            }
        }
    }
    // 2. Forward-auth user via `X-Remote-User-Id`
    if let Some(uid) = headers
        .get("x-remote-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
    {
        let username = headers
            .get("x-remote-user")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        return Ok(Identity::user(uid, username));
    }
    Err(error_resp(StatusCode::UNAUTHORIZED, "auth required"))
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn validate_slug(slug: &str) -> Result<(), Response> {
    if hr_apps::valid_slug(slug) {
        Ok(())
    } else {
        Err(error_resp(StatusCode::BAD_REQUEST, "invalid slug"))
    }
}

fn validate_table(table: &str) -> Result<(), Response> {
    let ok = !table.is_empty()
        && table.len() <= 64
        && table
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(error_resp(StatusCode::BAD_REQUEST, "invalid table name"))
    }
}

fn error_resp(code: StatusCode, msg: &str) -> Response {
    (
        code,
        Json(json!({
            "error": {
                "code": code_label(code),
                "message": msg,
            }
        })),
    )
        .into_response()
}

fn code_label(code: StatusCode) -> &'static str {
    match code.as_u16() {
        400 => "BAD_REQUEST",
        401 => "UNAUTHORIZED",
        403 => "FORBIDDEN",
        404 => "NOT_FOUND",
        409 => "CONFLICT",
        412 => "PRECONDITION_FAILED",
        422 => "UNPROCESSABLE",
        428 => "PRECONDITION_REQUIRED",
        503 => "SERVICE_UNAVAILABLE",
        _ => "INTERNAL",
    }
}

fn ipc_err(e: anyhow::Error) -> Response {
    error!(error = %e, "orchestrator IPC error");
    error_resp(StatusCode::SERVICE_UNAVAILABLE, "orchestrator unavailable")
}

/// Translate a typed [`hr_ipc::types::IpcResponse`] into an HTTP response,
/// using sentinel error strings emitted by [`crate::dv_handler`] to pick
/// HTTP status codes.
fn ipc_to_http_data(resp: hr_ipc::types::IpcResponse) -> Response {
    if resp.ok {
        Json(resp.data.unwrap_or(Value::Null)).into_response()
    } else {
        let raw = resp.error.unwrap_or_else(|| "unknown error".into());
        let lower = raw.to_lowercase();
        let (code, label_msg) = if lower.starts_with("not_found") || lower == "not_found" {
            (StatusCode::NOT_FOUND, "not found")
        } else if lower.starts_with("precondition_failed") {
            (StatusCode::PRECONDITION_FAILED, "version mismatch")
        } else if lower.starts_with("invalid bearer") || lower.contains("auth required") {
            (StatusCode::UNAUTHORIZED, raw.as_str())
        } else if lower.contains("$filter")
            || lower.contains("$select")
            || lower.contains("$orderby")
            || lower.contains("payload column")
            || lower.contains("table '")
        {
            (StatusCode::UNPROCESSABLE_ENTITY, raw.as_str())
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, raw.as_str())
        };
        (
            code,
            Json(json!({
                "error": { "code": code_label(code), "message": label_msg }
            })),
        )
            .into_response()
    }
}

// ── Schema ──────────────────────────────────────────────────────────────

async fn get_schema(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = extract_identity(&headers, &state, &slug).await {
        return r;
    }
    match state
        .orchestrator
        .request(&OrchestratorRequest::DvSchema { slug: slug.clone() })
        .await
    {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

// ── List ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ListParams {
    #[serde(default, rename = "$filter")]
    filter: Option<String>,
    #[serde(default, rename = "$select")]
    select: Option<String>,
    #[serde(default, rename = "$orderby")]
    orderby: Option<String>,
    #[serde(default, rename = "$top")]
    top: Option<u32>,
    #[serde(default, rename = "$skip")]
    skip: Option<u32>,
    #[serde(default, rename = "$includeDeleted")]
    include_deleted: Option<bool>,
    #[serde(default, rename = "$count")]
    count: Option<bool>,
}

async fn list_rows(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
    Query(params): Query<ListParams>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };

    // Build the ListQuery JSON shape inline. Mirrors `hr_dataverse::query::ListQuery`
    // but avoids depending on hr-dataverse from the HTTP layer.
    let select: Vec<Value> = params
        .select
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|p| Value::String(p.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let orderby: Vec<Value> = parse_orderby(params.orderby.as_deref());
    let mut q = serde_json::Map::new();
    if let Some(f) = params.filter {
        q.insert("filter".into(), Value::String(f));
    }
    q.insert("select".into(), Value::Array(select));
    q.insert("orderby".into(), Value::Array(orderby));
    if let Some(t) = params.top {
        q.insert("top".into(), Value::Number(t.into()));
    }
    if let Some(s) = params.skip {
        q.insert("skip".into(), Value::Number(s.into()));
    }
    q.insert(
        "include_deleted".into(),
        Value::Bool(params.include_deleted.unwrap_or(false)),
    );
    q.insert("count".into(), Value::Bool(params.count.unwrap_or(false)));
    let query = Value::Object(q);
    let req = OrchestratorRequest::DvList {
        slug: slug.clone(),
        table: table.clone(),
        query,
        identity,
    };
    info!(slug = %slug, table = %table, "DvList →");
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

/// Parse the OData `$orderby` query string into the JSON shape expected
/// by `hr_dataverse::query::OrderBy` (`{column, direction}`).
fn parse_orderby(s: Option<&str>) -> Vec<Value> {
    let Some(s) = s else { return vec![] };
    s.split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            // OData syntax: "col asc" / "col desc" / "col"
            let mut parts = item.split_whitespace();
            let col = parts.next()?;
            let direction = match parts.next().map(str::to_ascii_lowercase).as_deref() {
                Some("desc") => "desc",
                _ => "asc",
            };
            Some(json!({"column": col, "direction": direction}))
        })
        .collect()
}

// ── Get ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct GetParams {
    #[serde(default, rename = "$includeDeleted")]
    include_deleted: Option<bool>,
}

async fn get_row(
    State(state): State<ApiState>,
    Path((slug, table, id)): Path<(String, String, String)>,
    Query(params): Query<GetParams>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let id_value = parse_id(&id);
    let req = OrchestratorRequest::DvGet {
        slug,
        table,
        id: id_value,
        include_deleted: params.include_deleted.unwrap_or(false),
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => {
            let mut response = ipc_to_http_data(resp.clone_for_etag_probe());
            if resp.ok {
                if let Some(version) = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("version"))
                    .and_then(|v| v.as_i64())
                {
                    if let Ok(value) = axum::http::HeaderValue::from_str(&version.to_string()) {
                        response.headers_mut().insert(axum::http::header::ETAG, value);
                    }
                }
            }
            response
        }
        Err(e) => ipc_err(e),
    }
}

fn parse_id(raw: &str) -> Value {
    if let Ok(i) = raw.parse::<i64>() {
        return Value::Number(serde_json::Number::from(i));
    }
    Value::String(raw.to_string())
}

// ── Insert ──────────────────────────────────────────────────────────────

async fn insert_row(
    State(state): State<ApiState>,
    Path((slug, table)): Path<(String, String)>,
    headers: HeaderMap,
    Json(payload): Json<BTreeMap<String, Value>>,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let req = OrchestratorRequest::DvInsert {
        slug,
        table,
        payload,
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => {
            let mut r = ipc_to_http_data(resp.clone_for_etag_probe());
            if resp.ok {
                *r.status_mut() = StatusCode::CREATED;
                if let Some(v) = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("version"))
                    .and_then(|v| v.as_i64())
                {
                    if let Ok(hv) = axum::http::HeaderValue::from_str(&v.to_string()) {
                        r.headers_mut().insert(axum::http::header::ETAG, hv);
                    }
                }
            }
            r
        }
        Err(e) => ipc_err(e),
    }
}

// ── Update ──────────────────────────────────────────────────────────────

async fn update_row(
    State(state): State<ApiState>,
    Path((slug, table, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(payload): Json<BTreeMap<String, Value>>,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let if_version = match parse_if_match(&headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let req = OrchestratorRequest::DvUpdate {
        slug,
        table,
        id: parse_id(&id),
        if_version,
        payload,
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => {
            let mut r = ipc_to_http_data(resp.clone_for_etag_probe());
            if resp.ok {
                if let Some(v) = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("version"))
                    .and_then(|v| v.as_i64())
                {
                    if let Ok(hv) = axum::http::HeaderValue::from_str(&v.to_string()) {
                        r.headers_mut().insert(axum::http::header::ETAG, hv);
                    }
                }
            }
            r
        }
        Err(e) => ipc_err(e),
    }
}

fn parse_if_match(headers: &HeaderMap) -> Result<i32, Response> {
    let Some(v) = headers.get(axum::http::header::IF_MATCH) else {
        return Err(error_resp(
            StatusCode::PRECONDITION_REQUIRED,
            "If-Match header required",
        ));
    };
    let s = v
        .to_str()
        .map_err(|_| error_resp(StatusCode::BAD_REQUEST, "If-Match: bad header"))?;
    // Strip optional quotes (RFC 7232: "<etag>" or W/"<etag>"). For our
    // simple integer ETag, accept bare numbers or quoted numbers.
    let trimmed = s.trim().trim_start_matches("W/").trim_matches('"');
    trimmed
        .parse::<i32>()
        .map_err(|_| error_resp(StatusCode::BAD_REQUEST, "If-Match: not an integer version"))
}

// ── Soft-delete & Restore ───────────────────────────────────────────────

async fn soft_delete_row(
    State(state): State<ApiState>,
    Path((slug, table, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let if_version = match parse_if_match(&headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let req = OrchestratorRequest::DvSoftDelete {
        slug,
        table,
        id: parse_id(&id),
        if_version,
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

async fn restore_row(
    State(state): State<ApiState>,
    Path((slug, table, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    if let Err(r) = validate_table(&table) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let if_version = match parse_if_match(&headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let req = OrchestratorRequest::DvRestore {
        slug,
        table,
        id: parse_id(&id),
        if_version,
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

// ── Audit ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct AuditParams {
    #[serde(default)]
    table: Option<String>,
    #[serde(default)]
    row: Option<String>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default, rename = "$top")]
    top: Option<u32>,
    #[serde(default, rename = "$skip")]
    skip: Option<u32>,
}

async fn audit_list(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    Query(params): Query<AuditParams>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    let req = OrchestratorRequest::DvAuditList {
        slug,
        table: params.table,
        row_id: params.row,
        op: params.op,
        since: params.since,
        top: params.top,
        skip: params.skip,
        identity,
    };
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

// ── Token rotation (admin) ─────────────────────────────────────────────

async fn rotate_token(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate_slug(&slug) {
        return r;
    }
    // Token rotation is admin-only. Require a logged-in human user.
    let identity = match extract_identity(&headers, &state, &slug).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    if !matches!(identity, Identity::User { .. }) {
        warn!(slug, "non-user identity attempted rotate-token");
        return error_resp(StatusCode::FORBIDDEN, "rotate-token requires a user session");
    }
    let req = OrchestratorRequest::DvRotateToken { slug };
    match state.orchestrator.request(&req).await {
        Ok(resp) => ipc_to_http_data(resp),
        Err(e) => ipc_err(e),
    }
}

// ── ApiState extension helper ──────────────────────────────────────────

trait IpcResponseDup {
    fn clone_for_etag_probe(&self) -> hr_ipc::types::IpcResponse;
}

impl IpcResponseDup for hr_ipc::types::IpcResponse {
    fn clone_for_etag_probe(&self) -> hr_ipc::types::IpcResponse {
        hr_ipc::types::IpcResponse {
            ok: self.ok,
            error: self.error.clone(),
            data: self.data.clone(),
        }
    }
}
