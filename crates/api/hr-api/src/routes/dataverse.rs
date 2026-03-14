use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/overview", get(overview))
        .route("/apps/{app_id}/schema", get(app_schema))
        .route("/apps/{app_id}/tables", get(app_tables))
        .route("/apps/{app_id}/tables/{table_name}", get(app_table))
        .route("/apps/{app_id}/tables/{table_name}/rows", get(query_rows))
        .route("/apps/{app_id}/tables/{table_name}/rows", post(insert_rows))
        .route("/apps/{app_id}/tables/{table_name}/rows", put(update_rows))
        .route("/apps/{app_id}/tables/{table_name}/rows", delete(delete_rows))
        .route("/apps/{app_id}/tables/{table_name}/count", get(count_rows))
        .route("/apps/{app_id}/relations", get(app_relations))
        .route("/apps/{app_id}/stats", get(app_stats))
}

// ── Helper ────────────────────────────────────────────────────

async fn proxy_query(state: &ApiState, app_id: &str, query: serde_json::Value) -> impl IntoResponse {
    let req = OrchestratorRequest::DataverseQuery {
        app_id: app_id.to_string(),
        query,
    };
    match state.orchestrator.request(&req).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            Json(json!({ "data": data })).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Unknown error".to_string());
            let status = if msg.contains("not connected") {
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            } else if msg.contains("timeout") {
                axum::http::StatusCode::GATEWAY_TIMEOUT
            } else {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
    }
}

// ── Schema routes (delegate to hr-orchestrator via IPC) ───────

async fn ipc_get_schema(state: &ApiState, app_id: &str) -> Result<serde_json::Value, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let req = OrchestratorRequest::DataverseGetSchema { app_id: app_id.to_string() };
    match state.orchestrator.request(&req).await {
        Ok(r) if r.ok => Ok(r.data.unwrap_or(json!(null))),
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Unknown error".to_string());
            Err((axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))))
        }
        Err(e) => Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")})))),
    }
}

async fn overview(
    State(state): State<ApiState>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::DataverseOverview).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!({"apps": []}));
            Json(data).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Unknown error".to_string());
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
    }
}

async fn app_schema(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match ipc_get_schema(&state, &app_id).await {
        Ok(schema) => Json(json!({
            "data": schema,
            "meta": { "app_id": app_id }
        })).into_response(),
        Err(resp) => resp.into_response(),
    }
}

async fn app_tables(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match ipc_get_schema(&state, &app_id).await {
        Ok(schema) => {
            let tables = schema.get("tables").cloned().unwrap_or(json!([]));
            Json(json!({
                "tables": tables,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Err(resp) => resp.into_response(),
    }
}

async fn app_table(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match ipc_get_schema(&state, &app_id).await {
        Ok(schema) => {
            let tables = schema.get("tables").and_then(|t| t.as_array());
            let found = tables.and_then(|arr| {
                arr.iter().find(|t| t.get("name").and_then(|n| n.as_str()) == Some(&table_name))
            });
            match found {
                Some(table) => Json(json!({
                    "table": table,
                    "meta": { "app_id": app_id }
                })).into_response(),
                None => (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": format!("Table '{}' not found", table_name)}))).into_response(),
            }
        }
        Err(resp) => resp.into_response(),
    }
}

async fn app_relations(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match ipc_get_schema(&state, &app_id).await {
        Ok(schema) => {
            let relations = schema.get("relations").cloned().unwrap_or(json!([]));
            Json(json!({
                "relations": relations,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Err(resp) => resp.into_response(),
    }
}

async fn app_stats(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match ipc_get_schema(&state, &app_id).await {
        Ok(schema) => {
            let tables = schema.get("tables").and_then(|t| t.as_array());
            let tables_count = tables.map(|a| a.len()).unwrap_or(0);
            let total_rows: u64 = tables.map(|arr| {
                arr.iter().filter_map(|t| t.get("rowsCount").and_then(|v| v.as_u64())).sum()
            }).unwrap_or(0);
            let relations_count = schema.get("relations").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0);
            let db_size_bytes = schema.get("dbSizeBytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let version = schema.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            Json(json!({
                "dbSizeBytes": db_size_bytes,
                "tablesCount": tables_count,
                "relationsCount": relations_count,
                "totalRows": total_rows,
                "version": version,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Err(resp) => resp.into_response(),
    }
}

// ── Data CRUD routes (proxy to agent via IPC) ─────────────────

#[derive(Deserialize)]
struct RowsQuery {
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    order_desc: Option<bool>,
    /// JSON-encoded filters array
    #[serde(default)]
    filters: Option<String>,
}

fn default_limit() -> u64 {
    100
}

async fn query_rows(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
    Query(params): Query<RowsQuery>,
) -> impl IntoResponse {
    let filters: Vec<serde_json::Value> = params.filters
        .and_then(|f| serde_json::from_str(&f).ok())
        .unwrap_or_default();

    let query = json!({
        "action": "query_rows",
        "table_name": table_name,
        "filters": filters,
        "limit": params.limit,
        "offset": params.offset,
        "order_by": params.order_by,
        "order_desc": params.order_desc.unwrap_or(false),
    });
    proxy_query(&state, &app_id, query).await.into_response()
}

#[derive(Deserialize)]
struct InsertBody {
    rows: Vec<serde_json::Value>,
}

async fn insert_rows(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
    Json(body): Json<InsertBody>,
) -> impl IntoResponse {
    let query = json!({
        "action": "insert_rows",
        "table_name": table_name,
        "rows": body.rows,
    });
    proxy_query(&state, &app_id, query).await.into_response()
}

#[derive(Deserialize)]
struct UpdateBody {
    updates: serde_json::Value,
    filters: Vec<serde_json::Value>,
}

async fn update_rows(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
    Json(body): Json<UpdateBody>,
) -> impl IntoResponse {
    let query = json!({
        "action": "update_rows",
        "table_name": table_name,
        "updates": body.updates,
        "filters": body.filters,
    });
    proxy_query(&state, &app_id, query).await.into_response()
}

#[derive(Deserialize)]
struct DeleteBody {
    filters: Vec<serde_json::Value>,
}

async fn delete_rows(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
    Json(body): Json<DeleteBody>,
) -> impl IntoResponse {
    let query = json!({
        "action": "delete_rows",
        "table_name": table_name,
        "filters": body.filters,
    });
    proxy_query(&state, &app_id, query).await.into_response()
}

async fn count_rows(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
    Query(params): Query<RowsQuery>,
) -> impl IntoResponse {
    let filters: Vec<serde_json::Value> = params.filters
        .and_then(|f| serde_json::from_str(&f).ok())
        .unwrap_or_default();

    let query = json!({
        "action": "count_rows",
        "table_name": table_name,
        "filters": filters,
    });
    proxy_query(&state, &app_id, query).await.into_response()
}
