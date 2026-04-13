//! MCP (Model Context Protocol) HTTP endpoint for hr-orchestrator.
//!
//! Implements JSON-RPC 2.0 over HTTP POST, with Bearer token authentication.
//! Tools: hosts.*, deploy.*, apps.*, monitoring.*, git.*, store.*, reverseproxy.*, app.*, db.*

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use hr_common::events::{PowerAction, WakeResult};
use hr_registry::AgentRegistry;
use hr_registry::protocol::HostRegistryMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, info, warn};

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

// JSON-RPC error codes
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

// ── Shared state ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub token: Arc<String>,
    pub registry: Arc<AgentRegistry>,
    pub git: Arc<hr_git::GitService>,
    pub edge: Arc<hr_ipc::EdgeClient>,
    pub apps_ctx: Option<crate::apps_handler::AppsContext>,
}

impl McpState {
    pub fn from_env(
        registry: Arc<AgentRegistry>,
        git: Arc<hr_git::GitService>,
        edge: Arc<hr_ipc::EdgeClient>,
    ) -> Option<Self> {
        let token = std::env::var("MCP_TOKEN").ok()?;
        if token.is_empty() {
            return None;
        }
        Some(Self {
            token: Arc::new(token),
            registry,
            git,
            edge,
            apps_ctx: None,
        })
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn mcp_handler(
    State(state): State<McpState>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let project_slug = query.get("project").cloned();
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
            Json(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32000, "message": "Unauthorized"}}),
            ),
        );
    }

    // ── Parse JSON-RPC request ──
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::OK,
                Json(error_response(
                    Value::Null,
                    PARSE_ERROR,
                    format!("Parse error: {e}"),
                )),
            );
        }
    };

    let id = request.id.clone().unwrap_or(Value::Null);

    if request.jsonrpc != "2.0" {
        return (
            StatusCode::OK,
            Json(error_response(
                id,
                INVALID_REQUEST,
                "Invalid JSON-RPC version".into(),
            )),
        );
    }

    debug!(method = %request.method, "MCP request");

    // ── Route method ──
    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "tools/list" => handle_tools_list(id, &project_slug),
        "tools/call" => handle_tools_call(id, request.params, &state, project_slug).await,
        _ => error_response(
            id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    };

    (StatusCode::OK, Json(response))
}

// ── Tool definitions ────────────────────────────────────────────────

fn tool_definitions() -> Value {
    let mut tools = tool_definitions_core();
    tools.as_array_mut().unwrap().extend(
        tool_definitions_extended()
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    tools
        .as_array_mut()
        .unwrap()
        .extend(tool_definitions_apps().as_array().unwrap().iter().cloned());
    tools
}

fn tool_definitions_core() -> Value {
    json!([
        // ── Hosts ──
        {
            "name": "hosts.list",
            "description": "List all hosts with connection status, power state, and metrics.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "hosts.wake",
            "description": "Wake a host via Wake-on-LAN.",
            "inputSchema": {
                "type": "object",
                "properties": { "host_id": { "type": "string", "description": "Host ID" } },
                "required": ["host_id"]
            }
        },
        {
            "name": "hosts.reboot",
            "description": "Reboot a host.",
            "inputSchema": {
                "type": "object",
                "properties": { "host_id": { "type": "string", "description": "Host ID" } },
                "required": ["host_id"]
            }
        },
        {
            "name": "hosts.shutdown",
            "description": "Shutdown a host.",
            "inputSchema": {
                "type": "object",
                "properties": { "host_id": { "type": "string", "description": "Host ID" } },
                "required": ["host_id"]
            }
        },
        {
            "name": "hosts.exec",
            "description": "Execute a shell command on a host via SSH.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host_id": { "type": "string", "description": "Host ID" },
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["host_id", "command"]
            }
        },
        {
            "name": "hosts.create",
            "description": "Create a new host entry.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Host display name" },
                    "ip": { "type": "string", "description": "IP address" },
                    "mac": { "type": "string", "description": "MAC address for WOL (optional)" },
                    "description": { "type": "string", "description": "Description (optional)" }
                },
                "required": ["name", "ip"]
            }
        },
        {
            "name": "hosts.delete",
            "description": "Delete a host entry.",
            "inputSchema": {
                "type": "object",
                "properties": { "host_id": { "type": "string", "description": "Host ID" } },
                "required": ["host_id"]
            }
        },
        {
            "name": "hosts.set_wol_mac",
            "description": "Set the WOL MAC address for a host.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host_id": { "type": "string", "description": "Host ID" },
                    "mac": { "type": "string", "description": "MAC address (e.g. AA:BB:CC:DD:EE:FF)" }
                },
                "required": ["host_id", "mac"]
            }
        },
        // ── Monitoring ──
        {
            "name": "monitoring.system_status",
            "description": "Global system overview: each connected host with CPU/RAM/disk/load, each container with agent status/CPU/RAM, and uptime.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "monitoring.host_metrics",
            "description": "Detailed metrics for a specific host: CPU, memory, disk, load averages, and network interfaces.",
            "inputSchema": {
                "type": "object",
                "properties": { "host_id": { "type": "string", "description": "Host ID" } },
                "required": ["host_id"]
            }
        },
        {
            "name": "monitoring.app_health",
            "description": "Health check an application by curling its HTTP endpoint inside the container. Tries /api/health then / on the app's target port.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "Application ID" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "monitoring.edge_stats",
            "description": "Edge proxy statistics: per-domain request counts, 5xx errors, and TLS certificate expiry dates.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "monitoring.alerts",
            "description": "Active alerts based on system thresholds: disk >80%, RAM >90%, CPU >80% for 5+ min, TLS cert expiring <30 days, host offline (no heartbeat >2min), container down.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        // ── Git ──
        {
            "name": "git.repos",
            "description": "List all git repositories managed by HomeRoute, with size, branch count, and last commit date.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "git.log",
            "description": "Get the last N commits of a git repository.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository slug" },
                    "limit": { "type": "integer", "description": "Number of commits (default 20, max 100)", "default": 20 }
                },
                "required": ["repo"]
            }
        },
        {
            "name": "git.branches",
            "description": "List branches of a git repository.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo": { "type": "string", "description": "Repository slug" } },
                "required": ["repo"]
            }
        },
        {
            "name": "git.sync",
            "description": "Trigger a mirror sync for a git repository.",
            "inputSchema": {
                "type": "object",
                "properties": { "repo": { "type": "string", "description": "Repository slug" } },
                "required": ["repo"]
            }
        },
        {
            "name": "git.ssh_key",
            "description": "Get the SSH public key used for git mirror operations.",
            "inputSchema": { "type": "object", "properties": {} }
        },
    ])
}

fn tool_definitions_extended() -> Value {
    json!([
        // ── Store ──
        {
            "name": "store.list",
            "description": "List all apps in the HomeRoute app store.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "store.get",
            "description": "Get details of a specific app in the store.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "App slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "store.upload",
            "description": "Publish a new APK release for a HomeRoute store app. Pass the APK binary as base64 in apk_base64.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug":             { "type": "string", "description": "App slug (alphanumeric, -, _)" },
                    "version":          { "type": "string", "description": "Release version (e.g. 1.2.3)" },
                    "apk_base64":       { "type": "string", "description": "APK binary, base64-encoded" },
                    "app_name":         { "type": "string", "description": "App display name (required on first publish)" },
                    "description":      { "type": "string", "description": "App description" },
                    "category":         { "type": "string", "description": "Category (default: other)" },
                    "changelog":        { "type": "string", "description": "Release changelog" },
                    "publisher_app_id": { "type": "string", "description": "Publisher app id" }
                },
                "required": ["slug", "version", "apk_base64"]
            }
        },
        // ── Docs ──
        {
            "name": "docs.list",
            "description": "List all apps with their documentation status. Returns app_id, name, and which sections are filled.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "docs.get",
            "description": "Get documentation content for an app. Returns all sections or a specific one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application identifier (directory name in docs/)" },
                    "section": { "type": "string", "description": "Optional section: meta, structure, features, backend, notes. Omit for all." }
                },
                "required": ["app_id"]
            }
        },
        {
            "name": "docs.create",
            "description": "Create empty documentation files for a new app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application identifier" }
                },
                "required": ["app_id"]
            }
        },
        {
            "name": "docs.update",
            "description": "Update the content of a documentation section for an app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application identifier" },
                    "section": { "type": "string", "description": "Section to update: meta, structure, features, backend, notes" },
                    "content": { "type": "string", "description": "New content (Markdown for md sections, JSON string for meta)" }
                },
                "required": ["app_id", "section", "content"]
            }
        },
        {
            "name": "docs.search",
            "description": "Full-text search across all documentation files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "docs.completeness",
            "description": "Check which documentation sections are filled vs empty for an app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application identifier" }
                },
                "required": ["app_id"]
            }
        },
        // ── Reverse Proxy ──
        {
            "name": "reverseproxy.list",
            "description": "List all reverse proxy routes with their domain, target, enabled status, and options.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "reverseproxy.add",
            "description": "Add a new reverse proxy route. Specify either subdomain (appended to base domain) or customDomain for a fully custom domain.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subdomain": { "type": "string", "description": "Subdomain (e.g. 'app' for app.example.com)" },
                    "customDomain": { "type": "string", "description": "Full custom domain (overrides subdomain)" },
                    "targetHost": { "type": "string", "description": "Target host IP or hostname (default: localhost)" },
                    "targetPort": { "type": "integer", "description": "Target port (default: 80)" },
                    "localOnly": { "type": "boolean", "description": "Restrict to local network only (default: false)" },
                    "requireAuth": { "type": "boolean", "description": "Require HomeRoute authentication (default: false)" },
                    "enabled": { "type": "boolean", "description": "Enable route immediately (default: true)" }
                },
                "required": []
            }
        },
        {
            "name": "reverseproxy.delete",
            "description": "Delete a reverse proxy route by its ID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Route ID" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "reverseproxy.toggle",
            "description": "Toggle a reverse proxy route on or off.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Route ID" }
                },
                "required": ["id"]
            }
        },
    ])
}

// ── Method handlers ─────────────────────────────────────────────────

fn handle_initialize(id: Value) -> Value {
    success_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "hr-orchestrator",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(id: Value, project_slug: &Option<String>) -> Value {
    if project_slug.is_some() {
        // Project-scoped: only app/db/docs/studio/git tools
        success_response(id, json!({ "tools": tool_definitions_project() }))
    } else {
        // Global: all tools (infra + apps)
        success_response(id, json!({ "tools": tool_definitions() }))
    }
}

fn tool_definitions_project() -> Value {
    json!([
        // ── Process control ──
        { "name": "status", "description": "Get the current process state (running/stopped/crashed, PID, port, uptime, restart count).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "start", "description": "Start the application process.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "stop", "description": "Stop the application process.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "restart", "description": "Restart the application process (stop + start).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "exec", "description": "Execute a shell command in the project directory (e.g. pnpm build, cargo build --release).", "inputSchema": { "type": "object", "properties": { "command": { "type": "string", "description": "Shell command to execute" }, "timeout_secs": { "type": "integer", "default": 60 } }, "required": ["command"] } },
        { "name": "logs", "description": "Get recent application logs.", "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer", "default": 100 }, "level": { "type": "string", "description": "Filter by level (info, warn, error)" } } } },
        // ── Database ──
        { "name": "db_tables", "description": "List all tables in the application's SQLite database.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "db_schema", "description": "Describe a table's schema (columns, types, row count).", "inputSchema": { "type": "object", "properties": { "table": { "type": "string" } }, "required": ["table"] } },
        { "name": "db_query", "description": "Run a SELECT query against the database.", "inputSchema": { "type": "object", "properties": { "sql": { "type": "string" }, "params": { "type": "array", "items": {}, "default": [] } }, "required": ["sql"] } },
        { "name": "db_find", "description": "Query table rows with structured filters, sort, pagination and relation expand. No SQL required.", "inputSchema": { "type": "object", "properties": { "table": { "type": "string" }, "filters": { "type": "array", "description": "List of {column, op, value?}. op: eq|ne|gt|lt|gte|lte|like|in|is_null|is_not_null" }, "limit": { "type": "integer", "default": 100 }, "offset": { "type": "integer", "default": 0 }, "order_by": { "type": "string" }, "order_desc": { "type": "boolean", "default": false }, "expand": { "type": "array", "items": { "type": "string" }, "description": "Foreign-key relations to hydrate" } }, "required": ["table"] } },
        { "name": "db_exec", "description": "Execute a mutation (INSERT, UPDATE, DELETE) against the database.", "inputSchema": { "type": "object", "properties": { "sql": { "type": "string" }, "params": { "type": "array", "items": {}, "default": [] } }, "required": ["sql"] } },
        // ── Documentation ──
        { "name": "docs_read", "description": "Read the project documentation (all sections or a specific one).", "inputSchema": { "type": "object", "properties": { "section": { "type": "string", "enum": ["meta", "structure", "features", "backend", "notes"] } } } },
        { "name": "docs_write", "description": "Update a documentation section.", "inputSchema": { "type": "object", "properties": { "section": { "type": "string", "enum": ["meta", "structure", "features", "backend", "notes"] }, "content": { "type": "string" } }, "required": ["section", "content"] } },
        // ── Git ──
        { "name": "git_log", "description": "Get recent git commit history.", "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer", "default": 20 } } } },
        { "name": "git_branches", "description": "List git branches.", "inputSchema": { "type": "object", "properties": {} } }
    ])
}

async fn handle_tools_call(id: Value, params: Value, state: &McpState, project_slug: Option<String>) -> Value {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let mut arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    // Pre-contextualize: inject project slug into tools that need it
    if let Some(ref slug) = project_slug {
        let needs_slug = tool_name.starts_with("db.") || tool_name.starts_with("docs.") || matches!(
            tool_name,
            "app.status" | "app.control" | "app.logs" | "app.exec" | "app.get" |
            "app.health" | "app.regenerate_context" | "app.delete" | "app.build" |
            "git.log" | "git.branches" |
            "studio.refresh_context" |
            "secrets.list" | "secrets.get" | "secrets.set" | "secrets.delete" |
            // Project-scoped simplified names
            "status" | "start" | "stop" | "restart" | "exec" | "logs" |
            "db_tables" | "db_schema" | "db_query" | "db_find" | "db_exec" |
            "docs_read" | "docs_write" | "git_log" | "git_branches"
        );
        if needs_slug {
            if arguments.get("slug").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                arguments["slug"] = json!(slug);
            }
            if arguments.get("app_id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                arguments["app_id"] = json!(slug);
            }
            if arguments.get("repo").and_then(|v| v.as_str()).unwrap_or("").is_empty() && tool_name.starts_with("git.") {
                arguments["repo"] = json!(slug);
            }
        }
    }

    info!(tool = tool_name, project = ?project_slug, "MCP tools/call");

    match tool_name {
        // ── Hosts ──
        "hosts.list" => tool_hosts_list(id, state).await,
        "hosts.wake" => tool_hosts_wake(id, &arguments, state).await,
        "hosts.reboot" => tool_hosts_power(id, &arguments, state, PowerAction::Reboot).await,
        "hosts.shutdown" => tool_hosts_power(id, &arguments, state, PowerAction::Shutdown).await,
        "hosts.exec" => tool_hosts_exec(id, &arguments).await,
        "hosts.create" => tool_hosts_create(id, &arguments).await,
        "hosts.delete" => tool_hosts_delete(id, &arguments).await,
        "hosts.set_wol_mac" => tool_hosts_set_wol_mac(id, &arguments).await,
        // ── Monitoring ──
        "monitoring.system_status" => tool_monitoring_system_status(id, state).await,
        "monitoring.host_metrics" => tool_monitoring_host_metrics(id, &arguments, state).await,
        "monitoring.app_health" => tool_monitoring_app_health(id, &arguments, state).await,
        "monitoring.edge_stats" => tool_monitoring_edge_stats(id, state).await,
        "monitoring.alerts" => tool_monitoring_alerts(id, state).await,
        // ── Git ──
        "git.repos" => tool_git_repos(id, state).await,
        "git.log" => tool_git_log(id, &arguments, state).await,
        "git.branches" => tool_git_branches(id, &arguments).await,
        "git.sync" => tool_git_sync(id, &arguments).await,
        "git.ssh_key" => tool_git_ssh_key(id).await,
        // ── Store ──
        "store.list" => tool_store_list(id).await,
        "store.get" => tool_store_get(id, &arguments).await,
        "store.upload" => tool_store_upload(id, &arguments).await,
        // ── Reverse Proxy ──
        "reverseproxy.list" => tool_reverseproxy_list(id).await,
        "reverseproxy.add" => tool_reverseproxy_add(id, &arguments).await,
        "reverseproxy.delete" => tool_reverseproxy_delete(id, &arguments).await,
        "reverseproxy.toggle" => tool_reverseproxy_toggle(id, &arguments).await,
        // ── Docs ──
        "docs.list" => tool_docs_list(id).await,
        "docs.get" => tool_docs_get(id, &arguments).await,
        "docs.create" => tool_docs_create(id, &arguments).await,
        "docs.update" => tool_docs_update(id, &arguments).await,
        "docs.search" => tool_docs_search(id, &arguments).await,
        "docs.completeness" => tool_docs_completeness(id, &arguments).await,
        // ── Database ──
        // ── App* (V3 — hr-apps direct supervision) ──
        "app.list" => tool_app_list(id, state).await,
        "app.get" => tool_app_get(id, &arguments, state).await,
        "app.control" => tool_app_control(id, &arguments, state).await,
        "app.status" => tool_app_status(id, &arguments, state).await,
        "app.exec" => tool_app_exec(id, &arguments, state).await,
        "app.build" => tool_app_build(id, &arguments, state).await,
        "app.logs" => tool_app_logs(id, &arguments, state).await,
        "app.create" => tool_app_create(id, &arguments, state).await,
        "app.delete" => tool_app_delete(id, &arguments, state).await,
        "app.regenerate_context" => tool_app_regenerate_context(id, &arguments, state).await,
        // ── Studio ──
        "studio.refresh_context" => tool_studio_refresh_context(id, &arguments, state).await,
        "studio.refresh_all" => tool_studio_refresh_all(id, state).await,
        // ── DB* (V3 — per-app SQLite) ──
        "db.tables" | "db.list_tables" => tool_db_tables(id, &arguments, state).await,
        "db.describe" | "db.describe_table" => tool_db_describe(id, &arguments, state).await,
        "db.query" | "db.query_data" => tool_db_query(id, &arguments, state).await,
        "db.find" => tool_db_find(id, &arguments, state).await,
        "db.execute" | "db.insert_data" | "db.update_data" | "db.delete_data" => tool_db_execute(id, &arguments, state).await,
        "db.overview" => tool_db_overview(id, &arguments, state).await,
        "db.count_rows" => tool_db_count_rows(id, &arguments, state).await,
        "db.get_schema" => tool_db_get_schema(id, &arguments, state).await,
        "db.sync_schema" => tool_db_sync_schema(id, &arguments, state).await,
        "db.create_table" => tool_db_create_table(id, &arguments, state).await,
        "db.drop_table" => tool_db_drop_table(id, &arguments, state).await,
        "db.add_column" => tool_db_add_column(id, &arguments, state).await,
        "db.remove_column" => tool_db_remove_column(id, &arguments, state).await,
        "db.create_relation" => tool_db_create_relation(id, &arguments, state).await,
        // ── Project-scoped simplified names (used when ?project=slug) ──
        "status" => tool_app_status(id, &arguments, state).await,
        "start" => {
            let mut a = arguments.clone();
            a["action"] = json!("start");
            tool_app_control(id, &a, state).await
        }
        "stop" => {
            let mut a = arguments.clone();
            a["action"] = json!("stop");
            tool_app_control(id, &a, state).await
        }
        "restart" => {
            let mut a = arguments.clone();
            a["action"] = json!("restart");
            tool_app_control(id, &a, state).await
        }
        "exec" => tool_app_exec(id, &arguments, state).await,
        "logs" => tool_app_logs(id, &arguments, state).await,
        "db_tables" => tool_db_tables(id, &arguments, state).await,
        "db_schema" => tool_db_describe(id, &arguments, state).await,
        "db_query" => tool_db_query(id, &arguments, state).await,
        "db_find" => tool_db_find(id, &arguments, state).await,
        "db_exec" => tool_db_execute(id, &arguments, state).await,
        "db_get_schema" => tool_db_get_schema(id, &arguments, state).await,
        "db_sync_schema" => tool_db_sync_schema(id, &arguments, state).await,
        "db_create_table" => tool_db_create_table(id, &arguments, state).await,
        "db_drop_table" => tool_db_drop_table(id, &arguments, state).await,
        "db_add_column" => tool_db_add_column(id, &arguments, state).await,
        "db_remove_column" => tool_db_remove_column(id, &arguments, state).await,
        "db_create_relation" => tool_db_create_relation(id, &arguments, state).await,
        "docs_read" => tool_docs_get(id, &arguments).await,
        "docs_write" => tool_docs_update(id, &arguments).await,
        "git_log" => tool_git_log(id, &arguments, state).await,
        "git_branches" => tool_git_branches(id, &arguments).await,
        _ => {
            warn!(tool = tool_name, "Unknown tool");
            error_response(id, METHOD_NOT_FOUND, format!("Tool not found: {tool_name}"))
        }
    }
}

// ── Host tools ──────────────────────────────────────────────────────

async fn tool_hosts_list(id: Value, state: &McpState) -> Value {
    let conns = state.registry.host_connections.read().await;

    let hosts: Vec<Value> = conns
        .iter()
        .map(|(host_id, conn)| {
            json!({
                "host_id": host_id,
                "host_name": conn.host_name,
                "connected_at": conn.connected_at.to_rfc3339(),
                "last_heartbeat": conn.last_heartbeat.to_rfc3339(),
                "version": conn.version,
                "metrics": conn.metrics,
                "containers": conn.containers,
            })
        })
        .collect();
    drop(conns);

    let mut result = Vec::with_capacity(hosts.len());
    for mut host in hosts {
        let hid = host["host_id"].as_str().unwrap_or_default();
        let power = state.registry.get_host_power_state(hid).await;
        host["power_state"] = json!(format!("{power}"));
        result.push(host);
    }

    tool_success(id, json!(result))
}

async fn tool_hosts_wake(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };

    match state.registry.request_wake_host(host_id).await {
        Ok(result) => {
            let action = match result {
                WakeResult::WolSent => "wol_sent",
                WakeResult::AlreadyOnline => "already_online",
                WakeResult::AlreadyWaking => "already_waking",
            };
            tool_success(id, json!({ "action": action, "host_id": host_id }))
        }
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_hosts_power(id: Value, args: &Value, state: &McpState, action: PowerAction) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };

    if let Err(e) = state.registry.request_power_action(host_id, action).await {
        return tool_error(id, &e);
    }

    let msg = match action {
        PowerAction::Shutdown => HostRegistryMessage::PowerOff,
        PowerAction::Reboot => HostRegistryMessage::Reboot,
    };

    match state.registry.send_host_command(host_id, msg).await {
        Ok(()) => {
            let action_name = match action {
                PowerAction::Shutdown => "shutdown",
                PowerAction::Reboot => "reboot",
            };
            tool_success(id, json!({ "action": action_name, "host_id": host_id }))
        }
        Err(e) => tool_error(id, &e),
    }
}

// ── Deploy tools ────────────────────────────────────────────────────

/// Resolve an app_id to its container info for deploy status/logs.
// ── Monitoring tools ────────────────────────────────────────────────

async fn tool_monitoring_system_status(id: Value, state: &McpState) -> Value {
    // Collect host data
    let conns = state.registry.host_connections.read().await;
    let mut hosts = Vec::new();
    for (host_id, conn) in conns.iter() {
        let power = state.registry.get_host_power_state(host_id).await;
        let uptime_secs = (chrono::Utc::now() - conn.connected_at).num_seconds();
        hosts.push(json!({
            "host_id": host_id,
            "host_name": conn.host_name,
            "power_state": format!("{power}"),
            "uptime_seconds": uptime_secs,
            "metrics": conn.metrics,
            "containers_on_host": conn.containers.len(),
        }));
    }
    drop(conns);

    // Collect app/container data
    let apps = state.registry.list_applications().await;
    let app_statuses: Vec<Value> = apps
        .iter()
        .map(|app| {
            let mut entry = json!({
                "id": app.id,
                "name": app.name,
                "slug": app.slug,
                "status": app.status,
                "environment": app.environment,
                "host_id": app.host_id,
            });
            if let Some(ref m) = app.metrics {
                entry["cpu_percent"] = json!(m.cpu_percent);
                entry["memory_bytes"] = json!(m.memory_bytes);
            }
            entry
        })
        .collect();

    tool_success(
        id,
        json!({
            "hosts": hosts,
            "apps": app_statuses,
            "total_hosts": hosts.len(),
            "total_apps": apps.len(),
        }),
    )
}

async fn tool_monitoring_host_metrics(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };

    let conns = state.registry.host_connections.read().await;
    let conn = match conns.get(host_id) {
        Some(c) => c,
        None => return tool_error(id, &format!("Host not connected: {host_id}")),
    };

    let power = state.registry.get_host_power_state(host_id).await;
    let uptime_secs = (chrono::Utc::now() - conn.connected_at).num_seconds();

    let result = json!({
        "host_id": host_id,
        "host_name": conn.host_name,
        "power_state": format!("{power}"),
        "connected_at": conn.connected_at.to_rfc3339(),
        "last_heartbeat": conn.last_heartbeat.to_rfc3339(),
        "uptime_seconds": uptime_secs,
        "version": conn.version,
        "metrics": conn.metrics,
        "containers": conn.containers,
        "interfaces": conn.interfaces,
    });

    tool_success(id, result)
}

async fn tool_monitoring_app_health(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };

    let app = match state.registry.get_application(app_id).await {
        Some(a) => a,
        None => return tool_error(id, &format!("Application not found: {app_id}")),
    };

    let port = app.frontend.target_port;
    // Try /api/health first, then fall back to /
    let cmd = format!(
        concat!(
            "STATUS=$(curl -s -o /tmp/_health_body -w '%{{http_code}}' --connect-timeout 5 --max-time 10 http://localhost:{port}/api/health 2>/dev/null); ",
            "if [ \"$STATUS\" = \"000\" ]; then ",
            "  STATUS=$(curl -s -o /tmp/_health_body -w '%{{http_code}}' --connect-timeout 5 --max-time 10 http://localhost:{port}/ 2>/dev/null); ",
            "fi; ",
            "BODY=$(head -c 2048 /tmp/_health_body 2>/dev/null); ",
            "echo \"$STATUS\"; echo \"---\"; echo \"$BODY\""
        ),
        port = port
    );

    match state
        .registry
        .exec_in_remote_container(&app.host_id, &app.container_name, vec![cmd])
        .await
    {
        Ok((_, stdout, _)) => {
            let parts: Vec<&str> = stdout.splitn(3, "---\n").collect();
            let status_code = parts.first().unwrap_or(&"000").trim();
            let body = parts.get(1).unwrap_or(&"").trim();

            tool_success(
                id,
                json!({
                    "app_id": app_id,
                    "container": app.container_name,
                    "port": port,
                    "status_code": status_code,
                    "body": body,
                    "healthy": status_code.starts_with('2'),
                }),
            )
        }
        Err(e) => tool_error(id, &format!("Health check failed: {e}")),
    }
}

async fn tool_monitoring_edge_stats(id: Value, state: &McpState) -> Value {
    match state
        .edge
        .request(&hr_ipc::edge::EdgeRequest::GetStats)
        .await
    {
        Ok(resp) => {
            if resp.ok {
                tool_success(id, resp.data.unwrap_or(json!({})))
            } else {
                tool_error(id, resp.error.as_deref().unwrap_or("Unknown edge error"))
            }
        }
        Err(e) => tool_error(id, &format!("Failed to reach hr-edge: {e}")),
    }
}

async fn tool_monitoring_alerts(id: Value, state: &McpState) -> Value {
    let mut alerts: Vec<Value> = Vec::new();
    let now = chrono::Utc::now();

    // ── Host-level alerts ──
    let conns = state.registry.host_connections.read().await;
    for (host_id, conn) in conns.iter() {
        let since_hb = (now - conn.last_heartbeat).num_seconds();

        // Host offline: no heartbeat > 2 min
        if since_hb > 120 {
            alerts.push(json!({
                "severity": "critical",
                "source": format!("host:{}", host_id),
                "message": format!("Host '{}' no heartbeat since {}s", conn.host_name, since_hb),
            }));
        }

        if let Some(ref m) = conn.metrics {
            // Disk > 80%
            if m.disk_total_bytes > 0 {
                let disk_pct = m.disk_used_bytes as f64 / m.disk_total_bytes as f64 * 100.0;
                if disk_pct > 80.0 {
                    let severity = if disk_pct > 95.0 {
                        "critical"
                    } else {
                        "warning"
                    };
                    alerts.push(json!({
                        "severity": severity,
                        "source": format!("host:{}", host_id),
                        "message": format!("Disk usage {:.1}% on '{}'", disk_pct, conn.host_name),
                    }));
                }
            }

            // RAM > 90%
            if m.memory_total_bytes > 0 {
                let ram_pct = m.memory_used_bytes as f64 / m.memory_total_bytes as f64 * 100.0;
                if ram_pct > 90.0 {
                    let severity = if ram_pct > 95.0 {
                        "critical"
                    } else {
                        "warning"
                    };
                    alerts.push(json!({
                        "severity": severity,
                        "source": format!("host:{}", host_id),
                        "message": format!("RAM usage {:.1}% on '{}'", ram_pct, conn.host_name),
                    }));
                }
            }

            // CPU > 80% (instant value, noted in message)
            if m.cpu_percent > 80.0 {
                let severity = if m.cpu_percent > 95.0 {
                    "critical"
                } else {
                    "warning"
                };
                alerts.push(json!({
                    "severity": severity,
                    "source": format!("host:{}", host_id),
                    "message": format!("CPU {:.1}% on '{}' (sustained >80% check)", m.cpu_percent, conn.host_name),
                }));
            }
        }
    }
    drop(conns);

    // ── Container / app alerts ──
    let apps = state.registry.list_applications().await;
    for app in &apps {
        if app.enabled
            && (app.status == hr_registry::types::AgentStatus::Disconnected
                || app.status == hr_registry::types::AgentStatus::Pending)
        {
            alerts.push(json!({
                "severity": "critical",
                "source": format!("container:{}", app.container_name),
                "message": format!("Container '{}' is {:?} (app: {})", app.container_name, app.status, app.name),
            }));
        }
    }

    // ── TLS certificate alerts (via edge GetStats) ──
    match state
        .edge
        .request(&hr_ipc::edge::EdgeRequest::GetStats)
        .await
    {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                if let Some(domains) = data.get("domains").and_then(|v| v.as_array()) {
                    for domain in domains {
                        if let Some(expires) =
                            domain.get("cert_expires_at").and_then(|v| v.as_str())
                        {
                            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
                                let days = (exp.with_timezone(&chrono::Utc) - now).num_days();
                                if days < 30 {
                                    let severity = if days < 7 { "critical" } else { "warning" };
                                    let domain_name = domain
                                        .get("domain")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    alerts.push(json!({
                                        "severity": severity,
                                        "source": format!("cert:{}", domain_name),
                                        "message": format!("TLS cert for '{}' expires in {} days", domain_name, days),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // ── TLS certificate alerts (via AcmeListCertificates) ──
    match state
        .edge
        .request(&hr_ipc::edge::EdgeRequest::AcmeListCertificates)
        .await
    {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                if let Some(certs) = data.get("certificates").and_then(|v| v.as_array()) {
                    for cert in certs {
                        if let Some(expires) = cert.get("expires_at").and_then(|v| v.as_str()) {
                            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
                                let days = (exp.with_timezone(&chrono::Utc) - now).num_days();
                                if days < 30 {
                                    let severity = if days < 7 { "critical" } else { "warning" };
                                    let domain_name = cert
                                        .get("domains")
                                        .and_then(|v| v.as_array())
                                        .and_then(|a| a.first())
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    // Avoid duplicates from edge stats
                                    let source = format!("cert:{}", domain_name);
                                    if !alerts.iter().any(|a| {
                                        a.get("source").and_then(|s| s.as_str()) == Some(&source)
                                    }) {
                                        alerts.push(json!({
                                            "severity": severity,
                                            "source": source,
                                            "message": format!("TLS cert for '{}' expires in {} days", domain_name, days),
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Sort: critical first, then warning
    alerts.sort_by(|a, b| {
        let sa = a.get("severity").and_then(|v| v.as_str()).unwrap_or("");
        let sb = b.get("severity").and_then(|v| v.as_str()).unwrap_or("");
        sb.cmp(sa) // "critical" > "warning" alphabetically reversed
    });

    let total = alerts.len();
    tool_success(
        id,
        json!({
            "alerts": alerts,
            "total": total,
        }),
    )
}

// ── New host tools (via internal API) ────────────────────────────────

async fn tool_hosts_exec(id: Value, args: &Value) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing command".into());
    };
    match internal_api_post(
        &format!("/hosts/{host_id}/exec"),
        json!({"command": command}),
    )
    .await
    {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_hosts_create(id: Value, args: &Value) -> Value {
    let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing name".into());
    };
    let Some(ip) = args.get("ip").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing ip".into());
    };
    let mut body = json!({"name": name, "ip": ip});
    if let Some(mac) = args.get("mac").and_then(|v| v.as_str()) {
        body["mac"] = json!(mac);
    }
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        body["description"] = json!(desc);
    }
    match internal_api_post("/hosts", body).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_hosts_delete(id: Value, args: &Value) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };
    match internal_api_delete(&format!("/hosts/{host_id}")).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_hosts_set_wol_mac(id: Value, args: &Value) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };
    let Some(mac) = args.get("mac").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing mac".into());
    };
    match internal_api_post(&format!("/hosts/{host_id}/wol-mac"), json!({"mac": mac})).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── New apps tools ───────────────────────────────────────────────────

// ── Git tools ───────────────────────────────────────────────────────

async fn tool_git_repos(id: Value, state: &McpState) -> Value {
    match state.git.list_repos().await {
        Ok(repos) => {
            let result: Vec<Value> = repos
                .iter()
                .map(|r| {
                    json!({
                        "slug": r.slug,
                        "size_bytes": r.size_bytes,
                        "head_ref": r.head_ref,
                        "commit_count": r.commit_count,
                        "last_commit": r.last_commit,
                        "branches": r.branches,
                    })
                })
                .collect();
            tool_success(id, json!(result))
        }
        Err(e) => tool_error(id, &format!("Failed to list repos: {e}")),
    }
}

async fn tool_git_log(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(repo) = args.get("repo").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing repo".into());
    };

    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(100) as usize;

    match state.git.get_commits(repo, limit).await {
        Ok(commits) => {
            let result: Vec<Value> = commits
                .iter()
                .map(|c| {
                    json!({
                        "hash": c.hash,
                        "author_name": c.author_name,
                        "author_email": c.author_email,
                        "date": c.date,
                        "message": c.message,
                    })
                })
                .collect();
            tool_success(
                id,
                json!({
                    "repo": repo,
                    "commits": result,
                    "count": result.len(),
                }),
            )
        }
        Err(e) => tool_error(id, &format!("Failed to get commits: {e}")),
    }
}

// ── New git tools (via internal API) ─────────────────────────────────

async fn tool_git_branches(id: Value, args: &Value) -> Value {
    let Some(repo) = args.get("repo").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing repo".into());
    };
    match internal_api_get(&format!("/git/repos/{repo}/branches")).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_git_sync(id: Value, args: &Value) -> Value {
    let Some(repo) = args.get("repo").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing repo".into());
    };
    match internal_api_post(&format!("/git/repos/{repo}/mirror/sync"), json!({})).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_git_ssh_key(id: Value) -> Value {
    match internal_api_get("/git/ssh-key").await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── Store tools ──────────────────────────────────────────────────────

async fn tool_store_list(id: Value) -> Value {
    match internal_api_get("/store/apps").await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_store_get(id: Value, args: &Value) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    match internal_api_get(&format!("/store/apps/{slug}")).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_store_upload(id: Value, args: &Value) -> Value {
    use base64::Engine;

    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(version) = args.get("version").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing version".into());
    };
    let Some(apk_b64) = args.get("apk_base64").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing apk_base64".into());
    };

    let apk_bytes = match base64::engine::general_purpose::STANDARD.decode(apk_b64.trim()) {
        Ok(b) => b,
        Err(e) => {
            warn!(slug, "store.upload: invalid base64: {e}");
            return error_response(id, INVALID_PARAMS, format!("Invalid base64: {e}"));
        }
    };

    if apk_bytes.is_empty() {
        return error_response(id, INVALID_PARAMS, "Empty APK payload".into());
    }

    info!(
        slug = slug,
        version = version,
        size = apk_bytes.len(),
        "store.upload received"
    );

    let mut headers: Vec<(String, String)> = vec![("X-Version".into(), version.to_string())];
    for (arg_key, header_key) in [
        ("app_name", "X-App-Name"),
        ("description", "X-App-Description"),
        ("category", "X-App-Category"),
        ("changelog", "X-Changelog"),
        ("publisher_app_id", "X-Publisher-App-Id"),
    ] {
        if let Some(v) = args.get(arg_key).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                headers.push((header_key.into(), v.to_string()));
            }
        }
    }

    match internal_api_post_binary(
        &format!("/store/apps/{slug}/releases"),
        apk_bytes,
        headers,
    )
    .await
    {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── Internal API helpers ─────────────────────────────────────────────

const INTERNAL_API_BASE: &str = "http://127.0.0.1:4000/api";
const INTERNAL_TOKEN_HEADER: &str = "X-Internal-Token";

fn internal_token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| std::env::var("MCP_TOKEN").expect("MCP_TOKEN env var must be set"))
}

fn internal_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

async fn internal_api_get(path: &str) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .get(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, internal_token())
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>()
            .await
            .map_err(|e| format!("Parse error: {e}"))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_post(path: &str, body: Value) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .post(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, internal_token())
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>()
            .await
            .or_else(|_| Ok(json!({"status": "ok"})))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_post_binary(
    path: &str,
    bytes: Vec<u8>,
    headers: Vec<(String, String)>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Client build failed: {e}"))?;
    let mut req = client
        .post(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, internal_token())
        .header("Content-Type", "application/octet-stream");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req
        .body(bytes)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>()
            .await
            .or_else(|_| Ok(json!({"status": "ok"})))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_delete(path: &str) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .delete(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, internal_token())
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>()
            .await
            .or_else(|_| Ok(json!({"status": "deleted"})))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

// ── Reverse Proxy tools ──────────────────────────────────────────────

async fn tool_reverseproxy_list(id: Value) -> Value {
    match internal_api_get("/reverseproxy/hosts").await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_reverseproxy_add(id: Value, args: &Value) -> Value {
    let mut body = json!({});
    if let Some(v) = args.get("subdomain").and_then(|v| v.as_str()) {
        body["subdomain"] = json!(v);
    }
    if let Some(v) = args.get("customDomain").and_then(|v| v.as_str()) {
        body["customDomain"] = json!(v);
    }
    if let Some(v) = args.get("targetHost").and_then(|v| v.as_str()) {
        body["targetHost"] = json!(v);
    }
    if let Some(v) = args.get("targetPort").and_then(|v| v.as_u64()) {
        body["targetPort"] = json!(v);
    }
    if let Some(v) = args.get("localOnly").and_then(|v| v.as_bool()) {
        body["localOnly"] = json!(v);
    }
    if let Some(v) = args.get("requireAuth").and_then(|v| v.as_bool()) {
        body["requireAuth"] = json!(v);
    }
    if let Some(v) = args.get("enabled").and_then(|v| v.as_bool()) {
        body["enabled"] = json!(v);
    }
    match internal_api_post("/reverseproxy/hosts", body).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_reverseproxy_delete(id: Value, args: &Value) -> Value {
    let Some(route_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    match internal_api_delete(&format!("/reverseproxy/hosts/{route_id}")).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_reverseproxy_toggle(id: Value, args: &Value) -> Value {
    let Some(route_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    match internal_api_post(&format!("/reverseproxy/hosts/{route_id}/toggle"), json!({})).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── Docs tools ──────────────────────────────────────────────────────

const DOCS_DIR: &str = "/opt/homeroute/data/docs";
const DOCS_SECTIONS: &[&str] = &["structure", "features", "backend", "notes"];

fn docs_validate_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && !app_id.contains('/')
        && !app_id.contains("..")
        && app_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn docs_validate_section(section: &str) -> bool {
    section == "meta" || DOCS_SECTIONS.contains(&section)
}

async fn tool_docs_list(id: Value) -> Value {
    let dir = std::path::Path::new(DOCS_DIR);
    if !dir.exists() {
        return tool_success(id, json!({ "apps": [] }));
    }
    let mut apps = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return tool_error(id, "Failed to read docs directory");
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let app_id = entry.file_name().to_string_lossy().to_string();
        let app_dir = entry.path();

        // Read meta.json for name
        let name = std::fs::read_to_string(app_dir.join("meta.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
            .unwrap_or_else(|| app_id.clone());

        // Count filled sections
        let mut filled = 0u32;
        let total = 5u32;
        if app_dir.join("meta.json").exists() {
            let content = std::fs::read_to_string(app_dir.join("meta.json")).unwrap_or_default();
            if content.trim().len() > 2 {
                filled += 1;
            }
        }
        for section in DOCS_SECTIONS {
            let path = app_dir.join(format!("{section}.md"));
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                if !content.trim().is_empty() {
                    filled += 1;
                }
            }
        }

        apps.push(json!({
            "app_id": app_id,
            "name": name,
            "filled_sections": filled,
            "total_sections": total,
        }));
    }
    apps.sort_by(|a, b| {
        a.get("app_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("app_id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    tool_success(id, json!({ "apps": apps }))
}

async fn tool_docs_get(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    if !docs_validate_id(app_id) {
        return tool_error(id, "Invalid app_id");
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(app_id);
    if !app_dir.exists() {
        return tool_error(id, &format!("No docs found for '{app_id}'"));
    }

    let section = args.get("section").and_then(|v| v.as_str());
    if let Some(s) = section {
        if !docs_validate_section(s) {
            return tool_error(
                id,
                &format!("Invalid section '{s}'. Valid: meta, structure, features, backend, notes"),
            );
        }
        let filename = if s == "meta" {
            "meta.json".to_string()
        } else {
            format!("{s}.md")
        };
        let content = std::fs::read_to_string(app_dir.join(&filename)).unwrap_or_default();
        return tool_success(
            id,
            json!({ "app_id": app_id, "section": s, "content": content }),
        );
    }

    // Return all sections
    let meta = std::fs::read_to_string(app_dir.join("meta.json")).unwrap_or_default();
    let mut sections = json!({ "meta": meta });
    for s in DOCS_SECTIONS {
        let content = std::fs::read_to_string(app_dir.join(format!("{s}.md"))).unwrap_or_default();
        sections[s] = json!(content);
    }
    tool_success(id, json!({ "app_id": app_id, "sections": sections }))
}

async fn tool_docs_create(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    if !docs_validate_id(app_id) {
        return tool_error(id, "Invalid app_id");
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(app_id);
    if app_dir.exists() {
        return tool_error(id, &format!("Docs already exist for '{app_id}'"));
    }
    if let Err(e) = std::fs::create_dir_all(&app_dir) {
        return tool_error(id, &format!("Failed to create directory: {e}"));
    }
    // Create empty meta.json
    let meta = json!({ "name": app_id, "stack": "", "description": "", "logo": "" });
    let _ = std::fs::write(
        app_dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
    // Create empty markdown files
    for s in DOCS_SECTIONS {
        let _ = std::fs::write(app_dir.join(format!("{s}.md")), "");
    }
    info!(app_id, "Created docs");
    tool_success(
        id,
        json!({ "created": app_id, "sections": ["meta", "structure", "features", "backend", "notes"] }),
    )
}

async fn tool_docs_update(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    let Some(section) = args.get("section").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing section".into());
    };
    let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing content".into());
    };
    if !docs_validate_id(app_id) {
        return tool_error(id, "Invalid app_id");
    }
    if !docs_validate_section(section) {
        return tool_error(id, &format!("Invalid section '{section}'"));
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(app_id);
    if !app_dir.exists() {
        // Auto-create if not exists
        if let Err(e) = std::fs::create_dir_all(&app_dir) {
            return tool_error(id, &format!("Failed to create directory: {e}"));
        }
    }
    let filename = if section == "meta" {
        "meta.json".to_string()
    } else {
        format!("{section}.md")
    };
    let path = app_dir.join(&filename);
    if let Err(e) = std::fs::write(&path, content) {
        return tool_error(id, &format!("Failed to write: {e}"));
    }
    info!(app_id, section, "Updated docs section");
    tool_success(
        id,
        json!({ "app_id": app_id, "section": section, "updated": true }),
    )
}

async fn tool_docs_search(id: Value, args: &Value) -> Value {
    let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing query".into());
    };
    let query_lower = query.to_lowercase();
    let dir = std::path::Path::new(DOCS_DIR);
    if !dir.exists() {
        return tool_success(id, json!({ "results": [] }));
    }
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return tool_error(id, "Failed to read docs directory");
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let app_id = entry.file_name().to_string_lossy().to_string();
        let app_dir = entry.path();
        // Search meta.json
        if let Ok(content) = std::fs::read_to_string(app_dir.join("meta.json")) {
            if content.to_lowercase().contains(&query_lower) {
                results.push(json!({ "app_id": app_id, "section": "meta", "snippet": docs_snippet(&content, &query_lower) }));
            }
        }
        // Search markdown sections
        for s in DOCS_SECTIONS {
            let path = app_dir.join(format!("{s}.md"));
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.to_lowercase().contains(&query_lower) {
                    results.push(json!({ "app_id": app_id, "section": s, "snippet": docs_snippet(&content, &query_lower) }));
                }
            }
        }
    }
    tool_success(
        id,
        json!({ "query": query, "results": results, "count": results.len() }),
    )
}

fn docs_snippet(content: &str, query_lower: &str) -> String {
    let lower = content.to_lowercase();
    if let Some(pos) = lower.find(query_lower) {
        let start = pos.saturating_sub(40);
        let end = (pos + query_lower.len() + 40).min(content.len());
        // Ensure valid UTF-8 boundaries
        let start = content.floor_char_boundary(start);
        let end = content.ceil_char_boundary(end);
        let mut snippet = String::new();
        if start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(&content[start..end]);
        if end < content.len() {
            snippet.push_str("...");
        }
        snippet
    } else {
        content.chars().take(80).collect()
    }
}

async fn tool_docs_completeness(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    if !docs_validate_id(app_id) {
        return tool_error(id, "Invalid app_id");
    }
    let app_dir = std::path::Path::new(DOCS_DIR).join(app_id);
    if !app_dir.exists() {
        return tool_error(id, &format!("No docs found for '{app_id}'"));
    }
    let mut sections = Vec::new();
    // Check meta
    let meta_filled = std::fs::read_to_string(app_dir.join("meta.json"))
        .map(|s| s.trim().len() > 2)
        .unwrap_or(false);
    sections.push(json!({ "section": "meta", "filled": meta_filled }));
    // Check markdown sections
    for s in DOCS_SECTIONS {
        let filled = std::fs::read_to_string(app_dir.join(format!("{s}.md")))
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        sections.push(json!({ "section": s, "filled": filled }));
    }
    let filled_count = sections
        .iter()
        .filter(|s| s["filled"].as_bool().unwrap_or(false))
        .count();
    tool_success(
        id,
        json!({
            "app_id": app_id,
            "sections": sections,
            "filled": filled_count,
            "total": 5,
            "complete": filled_count == 5,
        }),
    )
}

// (db tools removed -- now managed per-environment by env-agent)
// (db tools removed -- now managed per-environment by env-agent)

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_success(id: Value, data: Value) -> Value {
    success_response(
        id,
        json!({
            "content": [{ "type": "text", "text": data.to_string() }]
        }),
    )
}

fn tool_error(id: Value, message: &str) -> Value {
    success_response(
        id,
        json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    )
}

// ── App* / DB* tool definitions (V3 — hr-apps) ──────────────────────

fn tool_definitions_apps() -> Value {
    json!([
        {
            "name": "app.list",
            "description": "List all HomeRoute applications managed by the AppSupervisor.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "app.get",
            "description": "Get details for a single application by slug.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "app.create",
            "description": "Create a new application (assigns port, git repo, edge route).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "name": { "type": "string" },
                    "stack": { "type": "string", "enum": ["next-js", "axum-vite", "axum"] },
                    "visibility": { "type": "string", "enum": ["public", "private"], "default": "private" },
                    "run_command": { "type": "string" },
                    "build_command": { "type": "string" },
                    "health_path": { "type": "string" },
                    "build_artefact": { "type": "string", "description": "Override artefact path(s) rsynced back after `app.build`. One per line, relative to src/." }
                },
                "required": ["slug", "name", "stack"]
            }
        },
        {
            "name": "app.control",
            "description": "Control an application process: start, stop, restart, or rebuild.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "action": { "type": "string", "enum": ["start", "stop", "restart", "rebuild"] }
                },
                "required": ["slug", "action"]
            }
        },
        {
            "name": "app.status",
            "description": "Get runtime status of an application (pid, state, port, uptime).",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "app.exec",
            "description": "Execute a shell command in the context of an application.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "command": { "type": "string" },
                    "timeout_secs": { "type": "integer", "default": 60 }
                },
                "required": ["slug", "command"]
            }
        },
        {
            "name": "app.build",
            "description": "Build an app remotely on CloudMaster (rsync src up, build, rsync artefacts down). Synchronous; bounded by `timeout_secs` (default 1800 = 30 min). Stacks: axum, axum-vite, next-js. Returns AppExecResult (stdout/stderr/exit_code/duration_ms).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "timeout_secs": { "type": "integer", "default": 1800 }
                },
                "required": ["slug"]
            }
        },
        {
            "name": "app.logs",
            "description": "Get recent logs for an application.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "limit": { "type": "integer", "default": 100 },
                    "level": { "type": "string" }
                },
                "required": ["slug"]
            }
        },
        {
            "name": "app.delete",
            "description": "Delete an application. Set keep_data=true to preserve source and DB.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "keep_data": { "type": "boolean", "default": false }
                },
                "required": ["slug"]
            }
        },
        {
            "name": "app.regenerate_context",
            "description": "Regenerate Claude context files (CLAUDE.md, .claude/) for an app.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "db.tables",
            "description": "List user-defined tables in an app's SQLite database.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "db.describe",
            "description": "Describe a table's schema (columns, types, row count).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" }
                },
                "required": ["slug", "table"]
            }
        },
        {
            "name": "db.query",
            "description": "Run a SELECT query against an app's SQLite database.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "sql": { "type": "string" },
                    "params": { "type": "array", "items": {}, "default": [] }
                },
                "required": ["slug", "sql"]
            }
        },
        {
            "name": "db.find",
            "description": "Query rows of a table with structured filters, sort, pagination and relation expand. No SQL required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" },
                    "filters": {
                        "type": "array",
                        "description": "List of {column, op, value?}. op ∈ eq|ne|gt|lt|gte|lte|like|in|is_null|is_not_null"
                    },
                    "limit": { "type": "integer", "default": 100, "description": "Capped at 1000" },
                    "offset": { "type": "integer", "default": 0 },
                    "order_by": { "type": "string" },
                    "order_desc": { "type": "boolean", "default": false },
                    "expand": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Foreign-key relations to hydrate inline"
                    }
                },
                "required": ["slug", "table"]
            }
        },
        {
            "name": "db.execute",
            "description": "Execute a mutation (INSERT, UPDATE, DELETE) against an app's SQLite database.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "sql": { "type": "string" },
                    "params": { "type": "array", "items": {}, "default": [] }
                },
                "required": ["slug", "sql"]
            }
        },
        {
            "name": "db.overview",
            "description": "Get an overview of an app's database (table count and list).",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "db.count_rows",
            "description": "Count rows in a specific table of an app's database.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" }
                },
                "required": ["slug", "table"]
            }
        },
        {
            "name": "db.get_schema",
            "description": "Get the full database schema (all tables, columns, and relations).",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "db.sync_schema",
            "description": "Sync existing SQLite tables into Dataverse metadata. Use after manual DDL changes.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "db.create_table",
            "description": "Create a new table. Columns id, created_at, updated_at are added automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "definition": {
                        "type": "object",
                        "description": "Table definition with name (string) and columns (array of {name, field_type, required?, unique?, default_value?, description?})"
                    }
                },
                "required": ["slug", "definition"]
            }
        },
        {
            "name": "db.drop_table",
            "description": "Drop a table from the database.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" }
                },
                "required": ["slug", "table"]
            }
        },
        {
            "name": "db.add_column",
            "description": "Add a column to an existing table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" },
                    "column": {
                        "type": "object",
                        "description": "Column definition with name, field_type, required?, unique?, default_value?, description?"
                    }
                },
                "required": ["slug", "table", "column"]
            }
        },
        {
            "name": "db.remove_column",
            "description": "Remove a column from a table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "table": { "type": "string" },
                    "column": { "type": "string", "description": "Column name to remove" }
                },
                "required": ["slug", "table", "column"]
            }
        },
        {
            "name": "db.create_relation",
            "description": "Create a foreign key relation between two tables.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "relation": {
                        "type": "object",
                        "description": "Relation with from_table, from_column, to_table, to_column, relation_type (one_to_many|many_to_many|self_referential), cascade? ({on_delete, on_update}: cascade|set_null|restrict)"
                    }
                },
                "required": ["slug", "relation"]
            }
        },
        {
            "name": "studio.refresh_context",
            "description": "Regenerate Claude Code context files (CLAUDE.md, .claude/) for a specific app.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string" } },
                "required": ["slug"]
            }
        },
        {
            "name": "studio.refresh_all",
            "description": "Regenerate Claude Code context files for all apps.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

// ── App* tool handlers ──────────────────────────────────────────────

fn require_apps_ctx<'a>(
    id: &Value,
    state: &'a McpState,
) -> Result<&'a crate::apps_handler::AppsContext, Value> {
    state
        .apps_ctx
        .as_ref()
        .ok_or_else(|| tool_error(id.clone(), "hr-apps not initialized"))
}

async fn tool_app_list(id: Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let resp = ctx.list().await;
    ipc_resp_to_mcp(id, resp)
}

async fn tool_app_get(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.get(slug).await)
}

async fn tool_app_create(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing name".into());
    };
    let Some(stack) = args.get("stack").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing stack".into());
    };
    let visibility = args
        .get("visibility")
        .and_then(|v| v.as_str())
        .unwrap_or("private");
    let run_command = args
        .get("run_command")
        .and_then(|v| v.as_str())
        .map(String::from);
    let build_command = args
        .get("build_command")
        .and_then(|v| v.as_str())
        .map(String::from);
    let health_path = args
        .get("health_path")
        .and_then(|v| v.as_str())
        .map(String::from);
    let build_artefact = args
        .get("build_artefact")
        .and_then(|v| v.as_str())
        .map(String::from);
    ipc_resp_to_mcp(
        id,
        ctx.create(
            slug.to_string(),
            name.to_string(),
            stack.to_string(),
            true,
            visibility.to_string(),
            run_command,
            build_command,
            health_path,
            build_artefact,
        )
        .await,
    )
}

async fn tool_app_build(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let timeout_secs = args.get("timeout_secs").and_then(|v| v.as_u64());
    ipc_resp_to_mcp(id, ctx.build(slug.to_string(), timeout_secs).await)
}

async fn tool_app_control(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing action".into());
    };
    ipc_resp_to_mcp(id, ctx.control(slug.to_string(), action.to_string()).await)
}

async fn tool_app_status(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.status(slug).await)
}

async fn tool_app_exec(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing command".into());
    };
    let timeout_secs = args.get("timeout_secs").and_then(|v| v.as_u64());
    ipc_resp_to_mcp(
        id,
        ctx.exec(slug.to_string(), command.to_string(), timeout_secs)
            .await,
    )
}

async fn tool_app_logs(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let level = args.get("level").and_then(|v| v.as_str()).map(String::from);
    ipc_resp_to_mcp(id, ctx.logs(slug.to_string(), limit, level).await)
}

async fn tool_app_delete(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let keep_data = args
        .get("keep_data")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    ipc_resp_to_mcp(id, ctx.delete(slug.to_string(), keep_data).await)
}

async fn tool_app_regenerate_context(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.regenerate_context(slug.to_string()).await)
}

// ── DB tool handlers ────────────────────────────────────────────────

async fn tool_db_tables(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.db_list_tables(slug.to_string()).await)
}

async fn tool_db_describe(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_describe_table(slug.to_string(), table.to_string())
            .await,
    )
}

async fn tool_db_query(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(sql) = args.get("sql").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing sql".into());
    };
    let params: Vec<Value> = args
        .get("params")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    ipc_resp_to_mcp(
        id,
        ctx.db_query(slug.to_string(), sql.to_string(), params)
            .await,
    )
}

// ── db.find (structured query: filters, sort, pagination, expand) ─

#[tracing::instrument(skip(state, args))]
async fn tool_db_find(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    let filters: Vec<Value> = args
        .get("filters")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let limit = args.get("limit").and_then(|v| v.as_u64());
    let offset = args.get("offset").and_then(|v| v.as_u64());
    let order_by = args
        .get("order_by")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let order_desc = args.get("order_desc").and_then(|v| v.as_bool());
    let expand: Vec<String> = args
        .get("expand")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    ipc_resp_to_mcp(
        id,
        ctx.db_query_rows(
            slug.to_string(),
            table.to_string(),
            filters,
            limit,
            offset,
            order_by,
            order_desc,
            expand,
        )
        .await,
    )
}

/// Convert an `IpcResponse` into a JSON-RPC response Value.
fn ipc_resp_to_mcp(id: Value, resp: hr_ipc::types::IpcResponse) -> Value {
    if resp.ok {
        tool_success(id, resp.data.unwrap_or(json!({"ok": true})))
    } else {
        tool_error(id, resp.error.as_deref().unwrap_or("unknown error"))
    }
}

// ── db.execute (mutations: INSERT/UPDATE/DELETE) ──────────────────

async fn tool_db_execute(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(sql) = args.get("sql").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing sql".into());
    };
    let params: Vec<Value> = args
        .get("params")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    ipc_resp_to_mcp(id, ctx.db_execute(slug.to_string(), sql.to_string(), params).await)
}

// ── db.overview ──────────────────────────────────────────────────────

async fn tool_db_overview(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    // List tables then describe each
    let tables_resp = ctx.db_list_tables(slug.to_string()).await;
    if !tables_resp.ok {
        return ipc_resp_to_mcp(id, tables_resp);
    }
    let tables = tables_resp
        .data
        .and_then(|d| d.get("tables").cloned())
        .and_then(|t| t.as_array().cloned())
        .unwrap_or_default();
    tool_success(id, json!({
        "slug": slug,
        "tables_count": tables.len(),
        "tables": tables,
    }))
}

// ── db.count_rows ────────────────────────────────────────────────────

async fn tool_db_count_rows(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    let sql = format!("SELECT COUNT(*) as count FROM \"{}\"", table.replace('"', ""));
    ipc_resp_to_mcp(id, ctx.db_query(slug.to_string(), sql, vec![]).await)
}

// ── db.get_schema / db.sync_schema ───────────────────────────────────

async fn tool_db_get_schema(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.db_get_schema(slug.to_string()).await)
}

async fn tool_db_sync_schema(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.db_sync_schema(slug.to_string()).await)
}

// ── db.create_table / db.drop_table ──────────────────────────────────

async fn tool_db_create_table(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(definition) = args.get("definition").cloned() else {
        return error_response(id, INVALID_PARAMS, "Missing definition".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_create_table(slug.to_string(), definition).await,
    )
}

async fn tool_db_drop_table(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_drop_table(slug.to_string(), table.to_string()).await,
    )
}

// ── db.add_column / db.remove_column ─────────────────────────────────

async fn tool_db_add_column(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    let Some(column) = args.get("column").cloned() else {
        return error_response(id, INVALID_PARAMS, "Missing column".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_add_column(slug.to_string(), table.to_string(), column)
            .await,
    )
}

async fn tool_db_remove_column(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(table) = args.get("table").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing table".into());
    };
    let Some(column) = args.get("column").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing column".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_remove_column(slug.to_string(), table.to_string(), column.to_string())
            .await,
    )
}

// ── db.create_relation ───────────────────────────────────────────────

async fn tool_db_create_relation(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(relation) = args.get("relation").cloned() else {
        return error_response(id, INVALID_PARAMS, "Missing relation".into());
    };
    ipc_resp_to_mcp(
        id,
        ctx.db_create_relation(slug.to_string(), relation).await,
    )
}

// ── studio.refresh_context ───────────────────────────────────────────

async fn tool_studio_refresh_context(id: Value, args: &Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    ipc_resp_to_mcp(id, ctx.regenerate_context(slug.to_string()).await)
}

// ── studio.refresh_all ───────────────────────────────────────────────

async fn tool_studio_refresh_all(id: Value, state: &McpState) -> Value {
    let ctx = match require_apps_ctx(&id, state) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let apps = ctx.supervisor.registry.list().await;
    let mut refreshed = 0u32;
    for app in &apps {
        let _ = ctx.regenerate_context(app.slug.clone()).await;
        refreshed += 1;
    }
    tool_success(id, json!({ "refreshed": refreshed, "total": apps.len() }))
}
