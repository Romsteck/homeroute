use axum::{
    body::Body,
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
        .route("/apps/{app_id}/migrations", get(app_migrations))
        .route("/apps/{app_id}/backup", get(backup_download))
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

// ── Existing read-only routes ─────────────────────────────────

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
    match state.orchestrator.request(&OrchestratorRequest::DataverseGetSchema { app_id: app_id.clone() }).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            Json(json!({
                "data": data,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Unknown error".to_string());
            let status = if msg.contains("not found") || msg.contains("No schema") {
                axum::http::StatusCode::NOT_FOUND
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

async fn app_tables(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::DataverseGetSchema { app_id: app_id.clone() }).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            let tables = data.get("tables").cloned().unwrap_or(json!([]));
            Json(json!({
                "tables": tables,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "No schema data for this application".to_string());
            (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
    }
}

async fn app_table(
    State(state): State<ApiState>,
    Path((app_id, table_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::DataverseGetSchema { app_id: app_id.clone() }).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            let tables = data.get("tables").and_then(|t| t.as_array());
            match tables {
                Some(tables) => {
                    match tables.iter().find(|t| t.get("name").and_then(|n| n.as_str()) == Some(&table_name)) {
                        Some(table) => Json(json!({
                            "table": table,
                            "meta": { "app_id": app_id }
                        })).into_response(),
                        None => (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": format!("Table '{}' not found", table_name)}))).into_response(),
                    }
                }
                None => (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": "No schema data for this application"}))).into_response(),
            }
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "No schema data for this application".to_string());
            (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
    }
}

async fn app_relations(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::DataverseGetSchema { app_id: app_id.clone() }).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            let relations = data.get("relations").cloned().unwrap_or(json!([]));
            Json(json!({
                "relations": relations,
                "meta": { "app_id": app_id }
            })).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "No schema data for this application".to_string());
            (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
    }
}

async fn app_stats(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    match state.orchestrator.request(&OrchestratorRequest::DataverseGetSchema { app_id: app_id.clone() }).await {
        Ok(r) if r.ok => {
            let data = r.data.unwrap_or(json!(null));
            // Extract stats from schema data
            let tables = data.get("tables").and_then(|t| t.as_array());
            let total_rows: u64 = tables.map(|ts| {
                ts.iter().filter_map(|t| t.get("row_count").and_then(|r| r.as_u64())).sum()
            }).unwrap_or(0);
            let tables_count = tables.map(|ts| ts.len()).unwrap_or(0);
            let relations_count = data.get("relations").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0);
            let db_size_bytes = data.get("db_size_bytes").and_then(|d| d.as_u64()).unwrap_or(0);
            let version = data.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            let last_updated = data.get("last_updated").and_then(|l| l.as_str()).unwrap_or("");
            Json(json!({
                "dbSizeBytes": db_size_bytes,
                "tablesCount": tables_count,
                "relationsCount": relations_count,
                "totalRows": total_rows,
                "version": version,
                "meta": { "app_id": app_id, "last_updated": last_updated }
            })).into_response()
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "No schema data for this application".to_string());
            (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
        }
        Err(e) => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response()
        }
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
        "type": "query_rows",
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
        "type": "insert_rows",
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
        "type": "update_rows",
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
        "type": "delete_rows",
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
        "type": "count_rows",
        "table_name": table_name,
        "filters": filters,
    });
    proxy_query(&state, &app_id, query).await.into_response()
}

async fn app_migrations(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    let query = json!({ "type": "get_migrations" });
    proxy_query(&state, &app_id, query).await.into_response()
}

// ── Backup route ──────────────────────────────────────────────

async fn backup_download(
    State(state): State<ApiState>,
    Path(app_id): Path<String>,
) -> impl IntoResponse {
    // Look up the app via IPC
    let app = match state.orchestrator.request(&OrchestratorRequest::GetApplication { id: app_id.clone() }).await {
        Ok(r) if r.ok => r.data.unwrap_or(json!(null)),
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Application not found".to_string());
            return (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response();
        }
        Err(e) => {
            return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": format!("IPC error: {e}")}))).into_response();
        }
    };

    let slug = app.get("slug").and_then(|s| s.as_str()).unwrap_or("app").to_string();
    let container_name = app.get("container_name").and_then(|s| s.as_str()).unwrap_or("").to_string();
    let host_id = app.get("host_id").and_then(|s| s.as_str()).unwrap_or("local").to_string();

    // Get storage path via IPC
    let storage_path = match state.orchestrator.request(&OrchestratorRequest::GetContainerConfig).await {
        Ok(r) if r.ok => {
            r.data
                .and_then(|d| d.get("container_storage_path").and_then(|s| s.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "/var/lib/machines".to_string())
        }
        _ => "/var/lib/machines".to_string(),
    };

    // Only support local containers for now
    if host_id != "local" {
        return (axum::http::StatusCode::NOT_IMPLEMENTED, Json(json!({"error": "Backup only supported for local containers"}))).into_response();
    }

    let db_path = std::path::PathBuf::from(&storage_path)
        .join(&container_name)
        .join("root/workspace/.dataverse/app.db");

    if !db_path.exists() {
        return (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": "No Dataverse database found for this application"}))).into_response();
    }

    // Create a backup copy using sqlite3 .backup to ensure WAL consistency
    let backup_path = std::env::temp_dir().join(format!("dataverse-backup-{}.db", app_id));
    let backup_result = tokio::process::Command::new("sqlite3")
        .arg(&db_path)
        .arg(format!(".backup '{}'", backup_path.display()))
        .output()
        .await;

    let backup_file = match backup_result {
        Ok(output) if output.status.success() => backup_path.clone(),
        _ => {
            // Fallback: direct copy if sqlite3 is not available
            if let Err(e) = tokio::fs::copy(&db_path, &backup_path).await {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to copy database: {}", e)}))).into_response();
            }
            backup_path.clone()
        }
    };

    // Read the backup file into memory
    let bytes = match tokio::fs::read(&backup_file).await {
        Ok(b) => b,
        Err(e) => {
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to read backup: {}", e)}))).into_response();
        }
    };

    let body = Body::from(bytes);

    let filename = format!("dataverse-{}.db", slug);

    // Clean up the temp file after a delay (fire and forget)
    let cleanup_path = backup_file;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let _ = tokio::fs::remove_file(cleanup_path).await;
    });

    axum::http::Response::builder()
        .status(200)
        .header("Content-Type", "application/x-sqlite3")
        .header("Content-Disposition", format!("attachment; filename=\"{}\"", filename))
        .body(body)
        .unwrap()
        .into_response()
}
