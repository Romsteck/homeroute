//! MCP (Model Context Protocol) HTTP endpoint for hr-host-agent Project Manager.
//!
//! Implements JSON-RPC 2.0 over HTTP POST, with Bearer token authentication.
//! Tools: project.*, deploy.*, db.*, git.*

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info};

use crate::config::Config;
use crate::registry::ProjectRegistry;

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;

// ── Shared state ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub token: Arc<String>,
    pub registry: Arc<ProjectRegistry>,
    pub config: Arc<Config>,
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn mcp_handler(
    State(state): State<McpState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // ── Auth ──
    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == state.token.as_str())
        .unwrap_or(false);

    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32000, "message": "Unauthorized"}})),
        );
    }

    // ── Parse JSON-RPC request ──
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::OK,
                Json(error_response(Value::Null, PARSE_ERROR, format!("Parse error: {e}"))),
            );
        }
    };

    let id = request.id.clone().unwrap_or(Value::Null);

    if request.jsonrpc != "2.0" {
        return (
            StatusCode::OK,
            Json(error_response(id, INVALID_REQUEST, "Invalid JSON-RPC version".into())),
        );
    }

    debug!(method = %request.method, "MCP request");

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "notifications/initialized" => success_response(id, json!({})),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, request.params, &state).await,
        _ => error_response(id, METHOD_NOT_FOUND, format!("Method not found: {}", request.method)),
    };

    (StatusCode::OK, Json(response))
}

// ── Initialize ──────────────────────────────────────────────────────

fn handle_initialize(id: Value) -> Value {
    success_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "hr-host-agent-pm",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

// ── Tool definitions ────────────────────────────────────────────────

fn handle_tools_list(id: Value) -> Value {
    success_response(id, json!({ "tools": tool_definitions() }))
}

fn tool_definitions() -> Value {
    json!([
        // ── Project ──
        {
            "name": "project.status",
            "description": "Detailed project status: PROD service state, last deploy date, git info.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Project slug" } },
                "required": ["slug"]
            }
        },

        // ── Deploy ──
        {
            "name": "project.deploy",
            "description": "Full stack-aware deploy: build locally → transfer to PROD → restart service → health check.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Project slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "project.deploy_status",
            "description": "Get systemd service status (app.service) from the PROD container.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Project slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "project.deploy_logs",
            "description": "Get recent service logs from the PROD container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Project slug" },
                    "lines": { "type": "integer", "description": "Number of log lines (default: 50)" }
                },
                "required": ["slug"]
            }
        },

        // ── Database (PROD) ──
        {
            "name": "project.db_tables",
            "description": "List SQLite tables and row counts from the PROD database.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Project slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "project.db_schema",
            "description": "Read the full SQLite schema (.schema) from the PROD database.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Project slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "project.db_query",
            "description": "Execute a read-only SQL query on the PROD SQLite database. Only SELECT statements allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Project slug" },
                    "sql": { "type": "string", "description": "SQL SELECT query" }
                },
                "required": ["slug", "sql"]
            }
        }
    ])
}

// ── Tool dispatch ───────────────────────────────────────────────────

async fn handle_tools_call(id: Value, params: Value, state: &McpState) -> Value {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    info!(tool = tool_name, "MCP tools/call");

    match tool_name {
        // Project
        "project.status" => crate::tools::project::tool_status(id, &arguments, state).await,
        // Deploy
        "project.deploy" => crate::tools::deploy::tool_deploy(id, &arguments, state).await,
        "project.deploy_status" => crate::tools::deploy::tool_deploy_status(id, &arguments, state).await,
        "project.deploy_logs" => crate::tools::deploy::tool_deploy_logs(id, &arguments, state).await,
        // Database
        "project.db_tables" => crate::tools::database::tool_db_tables(id, &arguments, state).await,
        "project.db_schema" => crate::tools::database::tool_db_schema(id, &arguments, state).await,
        "project.db_query" => crate::tools::database::tool_db_query(id, &arguments, state).await,

        _ => error_response(id, METHOD_NOT_FOUND, format!("Unknown tool: {tool_name}")),
    }
}

// ── Response helpers ────────────────────────────────────────────────

pub fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub fn tool_success(id: Value, data: Value) -> Value {
    success_response(id, json!({
        "content": [{ "type": "text", "text": data.to_string() }]
    }))
}

pub fn tool_error(id: Value, message: &str) -> Value {
    success_response(id, json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    }))
}
