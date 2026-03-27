//! MCP (Model Context Protocol) HTTP endpoint for hr-orchestrator.
//!
//! Implements JSON-RPC 2.0 over HTTP POST, with Bearer token authentication.
//! Tools: hosts.*, containers.*, deploy.*, apps.*, db.*, monitoring.*, git.*

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use hr_common::events::{PowerAction, WakeResult};
use hr_registry::protocol::HostRegistryMessage;
use hr_registry::AgentRegistry;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::backup_pipeline::BackupPipeline;
use crate::container_manager::ContainerManager;
use crate::db_manager::DbManager;

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
    pub container_manager: Arc<ContainerManager>,
    pub git: Arc<hr_git::GitService>,
    pub edge: Arc<hr_ipc::EdgeClient>,
    pub backup: Arc<BackupPipeline>,
    pub db: Arc<DbManager>,
    pub env_manager: Arc<crate::env_manager::EnvironmentManager>,
    pub pipeline_engine: Arc<hr_pipeline::PipelineEngine>,
}

impl McpState {
    pub fn from_env(
        registry: Arc<AgentRegistry>,
        container_manager: Arc<ContainerManager>,
        git: Arc<hr_git::GitService>,
        edge: Arc<hr_ipc::EdgeClient>,
        backup: Arc<BackupPipeline>,
        db: Arc<DbManager>,
        env_manager: Arc<crate::env_manager::EnvironmentManager>,
        pipeline_engine: Arc<hr_pipeline::PipelineEngine>,
    ) -> Option<Self> {
        let token = std::env::var("MCP_TOKEN").ok()?;
        if token.is_empty() {
            return None;
        }
        Some(Self {
            token: Arc::new(token),
            registry,
            container_manager,
            git,
            edge,
            backup,
            db,
            env_manager,
            pipeline_engine,
        })
    }
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

    // ── Route method ──
    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, request.params, &state).await,
        _ => error_response(id, METHOD_NOT_FOUND, format!("Method not found: {}", request.method)),
    };

    (StatusCode::OK, Json(response))
}

// ── Tool definitions ────────────────────────────────────────────────

fn tool_definitions() -> Value {
    let mut tools = tool_definitions_core();
    tools.as_array_mut().unwrap().extend(tool_definitions_extended().as_array().unwrap().iter().cloned());
    tools.as_array_mut().unwrap().extend(tool_definitions_env().as_array().unwrap().iter().cloned());
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
        // ── Containers ──
        {
            "name": "containers.list",
            "description": "List all containers with status, IP, agent version, and metrics.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "containers.start",
            "description": "Start a stopped container.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Container ID" } },
                "required": ["id"]
            }
        },
        {
            "name": "containers.stop",
            "description": "Stop a running container.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Container ID" } },
                "required": ["id"]
            }
        },
        {
            "name": "containers.restart",
            "description": "Restart a container (stop then start).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Container ID" } },
                "required": ["id"]
            }
        },
        {
            "name": "containers.create",
            "description": "Create a new container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Container name" },
                    "host_id": { "type": "string", "description": "Host ID to create container on" },
                    "ip": { "type": "string", "description": "IP address (optional, auto-assigned if omitted)" },
                    "description": { "type": "string", "description": "Description (optional)" }
                },
                "required": ["name", "host_id"]
            }
        },
        {
            "name": "containers.delete",
            "description": "Delete a container.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Container ID" } },
                "required": ["id"]
            }
        },
        {
            "name": "containers.update",
            "description": "Update container configuration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Container ID" },
                    "name": { "type": "string", "description": "New name (optional)" },
                    "ip": { "type": "string", "description": "New IP (optional)" },
                    "description": { "type": "string", "description": "New description (optional)" }
                },
                "required": ["id"]
            }
        },
        // ── Deploy ──
        {
            "name": "deploy.status",
            "description": "Get the systemd service status (app.service) of a production container. Returns active state, PID, start time, and binary info.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "Application ID" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "deploy.logs",
            "description": "Get the last N lines of journalctl logs for app.service in a production container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application ID" },
                    "lines": { "type": "integer", "description": "Number of log lines (default 50)", "default": 50 }
                },
                "required": ["app_id"]
            }
        },
        // ── Apps ──
        {
            "name": "apps.list",
            "description": "List all registered applications with status and container name.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "apps.get",
            "description": "Get detailed information about a specific application.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "Application ID" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "apps.exec",
            "description": "Execute a shell command inside an application's container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application ID" },
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["app_id", "command"]
            }
        },
        {
            "name": "apps.prod_exec",
            "description": "Execute a shell command inside the production container of an application.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "Application ID" },
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["app_id", "command"]
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
            "description": "Detailed metrics for a specific host: CPU, memory, disk, load averages, network interfaces, and managed containers.",
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
        {
            "name": "monitoring.envs",
            "description": "Cross-environment monitoring summary: each environment with status, agent health, running/total apps, host, and uptime.",
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
        // ── Database ──
        {
            "name": "db.overview",
            "description": "List all apps that have a database, with table counts and sizes.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "db.list_tables",
            "description": "List all tables in an app's database with column counts and row counts.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug (e.g. trader, wallet, home)" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "db.describe_table",
            "description": "Get full schema of a table (columns, types, constraints).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string", "description": "Table name" }
                },
                "required": ["app_id", "table_name"]
            }
        },
        {
            "name": "db.create_table",
            "description": "Create a new table with columns. Auto-adds id, created_at, updated_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "name": { "type": "string", "description": "Table name (snake_case)" },
                    "slug": { "type": "string", "description": "URL slug (kebab-case)" },
                    "description": { "type": "string", "description": "Table description" },
                    "columns": {
                        "type": "array",
                        "description": "Column definitions",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "field_type": { "type": "string", "description": "text, number, decimal, boolean, date_time, date, email, url, json, uuid, choice, lookup, etc." },
                                "required": { "type": "boolean" },
                                "unique": { "type": "boolean" },
                                "default_value": { "type": "string" },
                                "choices": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["name", "field_type"]
                        }
                    }
                },
                "required": ["app_id", "name", "slug", "columns"]
            }
        },
        {
            "name": "db.add_column",
            "description": "Add a column to an existing table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "name": { "type": "string", "description": "Column name" },
                    "field_type": { "type": "string", "description": "text, number, decimal, boolean, date_time, etc." },
                    "required": { "type": "boolean" },
                    "unique": { "type": "boolean" },
                    "default_value": { "type": "string" }
                },
                "required": ["app_id", "table_name", "name", "field_type"]
            }
        },
        {
            "name": "db.remove_column",
            "description": "Remove a column from a table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "column_name": { "type": "string" }
                },
                "required": ["app_id", "table_name", "column_name"]
            }
        },
        {
            "name": "db.drop_table",
            "description": "Drop a table and all its data. Requires confirm=true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "confirm": { "type": "boolean", "description": "Must be true to confirm deletion" }
                },
                "required": ["app_id", "table_name", "confirm"]
            }
        },
        {
            "name": "db.create_relation",
            "description": "Create a foreign key relationship between two tables.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "from_table": { "type": "string" },
                    "from_column": { "type": "string" },
                    "to_table": { "type": "string" },
                    "to_column": { "type": "string" },
                    "relation_type": { "type": "string", "description": "one_to_many, many_to_many, or self_referential" },
                    "on_delete": { "type": "string", "description": "cascade, set_null, or restrict (default: restrict)" },
                    "on_update": { "type": "string", "description": "cascade, set_null, or restrict (default: cascade)" }
                },
                "required": ["app_id", "from_table", "from_column", "to_table", "to_column", "relation_type"]
            }
        },
        {
            "name": "db.get_schema",
            "description": "Get the full database schema as JSON (tables, columns, relations).",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "db.get_db_info",
            "description": "Get database statistics: table count, total rows, schema version.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        },
        {
            "name": "db.query_data",
            "description": "Query rows from a table with optional filters, pagination, and sorting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "filters": {
                        "type": "array",
                        "description": "Filter conditions",
                        "items": {
                            "type": "object",
                            "properties": {
                                "column": { "type": "string" },
                                "op": { "type": "string", "description": "eq, ne, gt, lt, gte, lte, like, in, is_null, is_not_null" },
                                "value": {}
                            },
                            "required": ["column", "op"]
                        }
                    },
                    "limit": { "type": "integer", "description": "Max rows (default 100, max 1000)" },
                    "offset": { "type": "integer", "description": "Skip N rows" },
                    "order_by": { "type": "string", "description": "Column to sort by" },
                    "order_desc": { "type": "boolean", "description": "Sort descending" }
                },
                "required": ["app_id", "table_name"]
            }
        },
        {
            "name": "db.insert_data",
            "description": "Insert one or more rows into a table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "rows": {
                        "type": "array",
                        "description": "Array of objects to insert",
                        "items": { "type": "object" }
                    }
                },
                "required": ["app_id", "table_name", "rows"]
            }
        },
        {
            "name": "db.update_data",
            "description": "Update rows matching filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "updates": { "type": "object", "description": "Key-value pairs to update" },
                    "filters": {
                        "type": "array",
                        "description": "Filter conditions (at least one required)",
                        "items": {
                            "type": "object",
                            "properties": {
                                "column": { "type": "string" },
                                "op": { "type": "string" },
                                "value": {}
                            },
                            "required": ["column", "op"]
                        }
                    }
                },
                "required": ["app_id", "table_name", "updates", "filters"]
            }
        },
        {
            "name": "db.delete_data",
            "description": "Delete rows matching filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "filters": {
                        "type": "array",
                        "description": "Filter conditions (at least one required)",
                        "items": {
                            "type": "object",
                            "properties": {
                                "column": { "type": "string" },
                                "op": { "type": "string" },
                                "value": {}
                            },
                            "required": ["column", "op"]
                        }
                    }
                },
                "required": ["app_id", "table_name", "filters"]
            }
        },
        {
            "name": "db.count_rows",
            "description": "Count rows in a table, optionally with filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string", "description": "App slug" },
                    "table_name": { "type": "string" },
                    "filters": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "column": { "type": "string" },
                                "op": { "type": "string" },
                                "value": {}
                            },
                            "required": ["column", "op"]
                        }
                    }
                },
                "required": ["app_id", "table_name"]
            }
        }
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

fn handle_tools_list(id: Value) -> Value {
    success_response(id, json!({ "tools": tool_definitions() }))
}

async fn handle_tools_call(id: Value, params: Value, state: &McpState) -> Value {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    info!(tool = tool_name, "MCP tools/call");

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
        // ── Containers ──
        "containers.list" => tool_containers_list(id, state).await,
        "containers.start" => tool_container_action(id, &arguments, state, "start").await,
        "containers.stop" => tool_container_action(id, &arguments, state, "stop").await,
        "containers.restart" => tool_container_action(id, &arguments, state, "restart").await,
        "containers.create" => tool_containers_create(id, &arguments).await,
        "containers.delete" => tool_containers_delete(id, &arguments).await,
        "containers.update" => tool_containers_update(id, &arguments).await,
        // ── Deploy ──
        "deploy.status" => tool_deploy_status(id, &arguments, state).await,
        "deploy.logs" => tool_deploy_logs(id, &arguments, state).await,
        // ── Apps ──
        "apps.list" => tool_apps_list(id, state).await,
        "apps.get" => tool_apps_get(id, &arguments, state).await,
        "apps.exec" => tool_apps_exec(id, &arguments).await,
        "apps.prod_exec" => tool_apps_prod_exec(id, &arguments).await,
        // ── Monitoring ──
        "monitoring.system_status" => tool_monitoring_system_status(id, state).await,
        "monitoring.host_metrics" => tool_monitoring_host_metrics(id, &arguments, state).await,
        "monitoring.app_health" => tool_monitoring_app_health(id, &arguments, state).await,
        "monitoring.edge_stats" => tool_monitoring_edge_stats(id, state).await,
        "monitoring.alerts" => tool_monitoring_alerts(id, state).await,
        "monitoring.envs" => tool_monitoring_envs(id, state).await,
        // ── Git ──
        "git.repos" => tool_git_repos(id, state).await,
        "git.log" => tool_git_log(id, &arguments, state).await,
        "git.branches" => tool_git_branches(id, &arguments).await,
        "git.sync" => tool_git_sync(id, &arguments).await,
        "git.ssh_key" => tool_git_ssh_key(id).await,
        // ── Store ──
        "store.list" => tool_store_list(id).await,
        "store.get" => tool_store_get(id, &arguments).await,
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
        t if t.starts_with("db.") => handle_db_tool(id, t, &arguments, &state.db).await,
        // ── Environments ──
        "envs.list" => tool_envs_list(id, state).await,
        "envs.get" => tool_envs_get(id, &arguments, state).await,
        "envs.create" => tool_envs_create(id, &arguments, state).await,
        "envs.start" => tool_envs_action(id, &arguments, state, "start").await,
        "envs.stop" => tool_envs_action(id, &arguments, state, "stop").await,
        "envs.destroy" => tool_envs_destroy(id, &arguments, state).await,
        // ── Pipelines ──
        "pipeline.promote" => tool_pipeline_promote(id, &arguments, state).await,
        "pipeline.status" => tool_pipeline_status(id, &arguments, state).await,
        "pipeline.history" => tool_pipeline_history(id, state).await,
        "pipeline.cancel" => tool_pipeline_cancel(id, &arguments, state).await,
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

// ── Container tools ─────────────────────────────────────────────────

async fn tool_containers_list(id: Value, state: &McpState) -> Value {
    let containers = state.container_manager.list_containers().await;
    tool_success(id, json!(containers))
}

async fn tool_container_action(id: Value, args: &Value, state: &McpState, action: &str) -> Value {
    let Some(container_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };

    let result = match action {
        "start" => state.container_manager.start_container(container_id).await,
        "stop" => state.container_manager.stop_container(container_id).await,
        "restart" => {
            match state.container_manager.stop_container(container_id).await {
                Ok(true) => state.container_manager.start_container(container_id).await,
                Ok(false) => return tool_error(id, "Container not found"),
                Err(e) => return tool_error(id, &e),
            }
        }
        _ => unreachable!(),
    };

    match result {
        Ok(true) => tool_success(id, json!({ "action": action, "id": container_id })),
        Ok(false) => tool_error(id, &format!("Container not found: {container_id}")),
        Err(e) => tool_error(id, &e),
    }
}

// ── Deploy tools ────────────────────────────────────────────────────

/// Resolve an app_id to its container info for deploy status/logs.
async fn resolve_prod_app(state: &McpState, app_id: &str) -> Result<(String, String, String), String> {
    let app = state.registry.get_application(app_id).await
        .ok_or_else(|| format!("Application not found: {app_id}"))?;

    Ok((app.id, app.container_name, app.host_id))
}

async fn tool_deploy_status(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };

    let (_prod_id, container_name, host_id) = match resolve_prod_app(state, app_id).await {
        Ok(v) => v,
        Err(e) => return tool_error(id, &e),
    };

    let cmd = concat!(
        "echo -n \"SERVICE_ACTIVE=\"; systemctl is-active app.service 2>/dev/null || true; ",
        "echo -n \"SERVICE_STATUS=\"; systemctl show app.service ",
        "--property=ActiveState,SubState,MainPID,ExecMainStartTimestamp --no-pager 2>/dev/null || true; ",
        "echo -n \"BINARY_INFO=\"; stat --printf='%s %Y' /opt/app/app 2>/dev/null || echo \"not_found\""
    );

    match state.registry.exec_in_remote_container(&host_id, &container_name, vec![cmd.to_string()]).await {
        Ok((success, stdout, stderr)) => {
            tool_success(id, json!({
                "success": success,
                "container": container_name,
                "host_id": host_id,
                "raw": stdout,
                "stderr": stderr,
            }))
        }
        Err(e) => tool_error(id, &format!("Exec failed: {e}")),
    }
}

async fn tool_deploy_logs(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };

    let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50).min(500);

    let (_prod_id, container_name, host_id) = match resolve_prod_app(state, app_id).await {
        Ok(v) => v,
        Err(e) => return tool_error(id, &e),
    };

    let cmd = format!("journalctl -u app.service -n {lines} --no-pager 2>&1");

    match state.registry.exec_in_remote_container(&host_id, &container_name, vec![cmd]).await {
        Ok((success, stdout, _stderr)) => {
            tool_success(id, json!({
                "success": success,
                "container": container_name,
                "lines": lines,
                "logs": stdout,
            }))
        }
        Err(e) => tool_error(id, &format!("Exec failed: {e}")),
    }
}

// ── Apps tools ──────────────────────────────────────────────────────

async fn tool_apps_list(id: Value, state: &McpState) -> Value {
    let apps = state.registry.list_applications().await;

    let result: Vec<Value> = apps
        .iter()
        .map(|app| {
            json!({
                "id": app.id,
                "name": app.name,
                "slug": app.slug,
                "host_id": app.host_id,
                "environment": app.environment,
                "enabled": app.enabled,
                "container_name": app.container_name,
                "status": app.status,
                "ipv4_address": app.ipv4_address.map(|ip| ip.to_string()),
                "agent_version": app.agent_version,
                "last_heartbeat": app.last_heartbeat,
                "stack": app.stack,
            })
        })
        .collect();

    tool_success(id, json!(result))
}

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

    tool_success(id, json!({
        "hosts": hosts,
        "apps": app_statuses,
        "total_hosts": hosts.len(),
        "total_apps": apps.len(),
    }))
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

    match state.registry.exec_in_remote_container(&app.host_id, &app.container_name, vec![cmd]).await {
        Ok((_, stdout, _)) => {
            let parts: Vec<&str> = stdout.splitn(3, "---\n").collect();
            let status_code = parts.first().unwrap_or(&"000").trim();
            let body = parts.get(1).unwrap_or(&"").trim();

            tool_success(id, json!({
                "app_id": app_id,
                "container": app.container_name,
                "port": port,
                "status_code": status_code,
                "body": body,
                "healthy": status_code.starts_with('2'),
            }))
        }
        Err(e) => tool_error(id, &format!("Health check failed: {e}")),
    }
}

async fn tool_monitoring_edge_stats(id: Value, state: &McpState) -> Value {
    match state.edge.request(&hr_ipc::edge::EdgeRequest::GetStats).await {
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
                    let severity = if disk_pct > 95.0 { "critical" } else { "warning" };
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
                    let severity = if ram_pct > 95.0 { "critical" } else { "warning" };
                    alerts.push(json!({
                        "severity": severity,
                        "source": format!("host:{}", host_id),
                        "message": format!("RAM usage {:.1}% on '{}'", ram_pct, conn.host_name),
                    }));
                }
            }

            // CPU > 80% (instant value, noted in message)
            if m.cpu_percent > 80.0 {
                let severity = if m.cpu_percent > 95.0 { "critical" } else { "warning" };
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
    match state.edge.request(&hr_ipc::edge::EdgeRequest::GetStats).await {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                if let Some(domains) = data.get("domains").and_then(|v| v.as_array()) {
                    for domain in domains {
                        if let Some(expires) = domain.get("cert_expires_at").and_then(|v| v.as_str()) {
                            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
                                let days = (exp.with_timezone(&chrono::Utc) - now).num_days();
                                if days < 30 {
                                    let severity = if days < 7 { "critical" } else { "warning" };
                                    let domain_name = domain.get("domain").and_then(|v| v.as_str()).unwrap_or("unknown");
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
    match state.edge.request(&hr_ipc::edge::EdgeRequest::AcmeListCertificates).await {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                if let Some(certs) = data.get("certificates").and_then(|v| v.as_array()) {
                    for cert in certs {
                        if let Some(expires) = cert.get("expires_at").and_then(|v| v.as_str()) {
                            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
                                let days = (exp.with_timezone(&chrono::Utc) - now).num_days();
                                if days < 30 {
                                    let severity = if days < 7 { "critical" } else { "warning" };
                                    let domain_name = cert.get("domains")
                                        .and_then(|v| v.as_array())
                                        .and_then(|a| a.first())
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    // Avoid duplicates from edge stats
                                    let source = format!("cert:{}", domain_name);
                                    if !alerts.iter().any(|a| a.get("source").and_then(|s| s.as_str()) == Some(&source)) {
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
    tool_success(id, json!({
        "alerts": alerts,
        "total": total,
    }))
}

async fn tool_monitoring_envs(id: Value, state: &McpState) -> Value {
    let envs = state.env_manager.list_environments().await;
    let now = chrono::Utc::now();

    let result: Vec<Value> = envs.iter().map(|env| {
        let running_apps = env.apps.iter().filter(|a| a.running).count();
        let uptime_secs = env.last_heartbeat.map(|_| {
            let elapsed = (now - env.created_at).num_seconds();
            if elapsed < 0 { 0 } else { elapsed }
        });
        json!({
            "slug": env.slug,
            "name": env.name,
            "env_type": env.env_type,
            "host_id": env.host_id,
            "status": env.status,
            "agent_connected": env.agent_connected,
            "agent_version": env.agent_version,
            "apps_running": running_apps,
            "apps_total": env.apps.len(),
            "last_heartbeat": env.last_heartbeat.map(|hb| hb.to_rfc3339()),
            "uptime_secs": uptime_secs,
            "healthy": env.agent_connected && running_apps == env.apps.len(),
        })
    }).collect();

    let total = result.len();
    let healthy = result.iter().filter(|e| e.get("healthy").and_then(|v| v.as_bool()).unwrap_or(false)).count();
    tool_success(id, json!({
        "environments": result,
        "total": total,
        "healthy": healthy,
        "unhealthy": total - healthy,
    }))
}

// ── New host tools (via internal API) ────────────────────────────────

async fn tool_hosts_exec(id: Value, args: &Value) -> Value {
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing command".into());
    };
    match internal_api_post(&format!("/hosts/{host_id}/exec"), json!({"command": command})).await {
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

// ── New container tools (via internal API) ───────────────────────────

async fn tool_containers_create(id: Value, args: &Value) -> Value {
    let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing name".into());
    };
    let Some(host_id) = args.get("host_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing host_id".into());
    };
    let mut body = json!({"name": name, "host_id": host_id});
    if let Some(ip) = args.get("ip").and_then(|v| v.as_str()) {
        body["ip"] = json!(ip);
    }
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        body["description"] = json!(desc);
    }
    match internal_api_post("/containers", body).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_containers_delete(id: Value, args: &Value) -> Value {
    let Some(container_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    match internal_api_delete(&format!("/containers/{container_id}")).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_containers_update(id: Value, args: &Value) -> Value {
    let Some(container_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    let mut body = serde_json::Map::new();
    for field in &["name", "ip", "description"] {
        if let Some(val) = args.get(*field) {
            body.insert((*field).into(), val.clone());
        }
    }
    if body.is_empty() {
        return tool_error(id, "No fields to update provided");
    }
    match internal_api_put(&format!("/containers/{container_id}"), Value::Object(body)).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── New apps tools ───────────────────────────────────────────────────

async fn tool_apps_get(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    match state.registry.get_application(app_id).await {
        Some(app) => tool_success(id, json!({
            "id": app.id,
            "name": app.name,
            "slug": app.slug,
            "host_id": app.host_id,
            "environment": app.environment,
            "enabled": app.enabled,
            "container_name": app.container_name,
            "status": app.status,
            "ipv4_address": app.ipv4_address.map(|ip| ip.to_string()),
            "agent_version": app.agent_version,
            "last_heartbeat": app.last_heartbeat,
            "stack": app.stack,
            "target_port": app.frontend.target_port,
            "metrics": app.metrics,
        })),
        None => tool_error(id, &format!("Application not found: {app_id}")),
    }
}

async fn tool_apps_exec(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing command".into());
    };
    match internal_api_post(&format!("/applications/{app_id}/exec"), json!({"command": [command]})).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

async fn tool_apps_prod_exec(id: Value, args: &Value) -> Value {
    let Some(app_id) = args.get("app_id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_id".into());
    };
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing command".into());
    };
    match internal_api_post(&format!("/applications/{app_id}/exec"), json!({"command": command})).await {
        Ok(data) => tool_success(id, data),
        Err(e) => tool_error(id, &e),
    }
}

// ── Git tools ───────────────────────────────────────────────────────

async fn tool_git_repos(id: Value, state: &McpState) -> Value {
    match state.git.list_repos().await {
        Ok(repos) => {
            let result: Vec<Value> = repos
                .iter()
                .map(|r| json!({
                    "slug": r.slug,
                    "size_bytes": r.size_bytes,
                    "head_ref": r.head_ref,
                    "commit_count": r.commit_count,
                    "last_commit": r.last_commit,
                    "branches": r.branches,
                }))
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

    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(100) as usize;

    match state.git.get_commits(repo, limit).await {
        Ok(commits) => {
            let result: Vec<Value> = commits
                .iter()
                .map(|c| json!({
                    "hash": c.hash,
                    "author_name": c.author_name,
                    "author_email": c.author_email,
                    "date": c.date,
                    "message": c.message,
                }))
                .collect();
            tool_success(id, json!({
                "repo": repo,
                "commits": result,
                "count": result.len(),
            }))
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

// ── Internal API helpers ─────────────────────────────────────────────

const INTERNAL_API_BASE: &str = "http://127.0.0.1:4000/api";
const INTERNAL_TOKEN_HEADER: &str = "X-Internal-Token";
const INTERNAL_TOKEN: &str = "REDACTED_SECRET";

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
        .header(INTERNAL_TOKEN_HEADER, INTERNAL_TOKEN)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>().await.map_err(|e| format!("Parse error: {e}"))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_post(path: &str, body: Value) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .post(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, INTERNAL_TOKEN)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>().await.or_else(|_| Ok(json!({"status": "ok"})))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_put(path: &str, body: Value) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .put(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, INTERNAL_TOKEN)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>().await.or_else(|_| Ok(json!({"status": "ok"})))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API returned {status}: {body}"))
    }
}

async fn internal_api_delete(path: &str) -> Result<Value, String> {
    let client = internal_client();
    let resp = client
        .delete(format!("{INTERNAL_API_BASE}{path}"))
        .header(INTERNAL_TOKEN_HEADER, INTERNAL_TOKEN)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>().await.or_else(|_| Ok(json!({"status": "deleted"})))
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
        && app_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
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
        let mut total = 5u32;
        if app_dir.join("meta.json").exists() {
            let content = std::fs::read_to_string(app_dir.join("meta.json")).unwrap_or_default();
            if content.trim().len() > 2 { filled += 1; }
        }
        for section in DOCS_SECTIONS {
            let path = app_dir.join(format!("{section}.md"));
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                if !content.trim().is_empty() { filled += 1; }
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
        a.get("app_id").and_then(|v| v.as_str()).unwrap_or("")
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
            return tool_error(id, &format!("Invalid section '{s}'. Valid: meta, structure, features, backend, notes"));
        }
        let filename = if s == "meta" { "meta.json".to_string() } else { format!("{s}.md") };
        let content = std::fs::read_to_string(app_dir.join(&filename)).unwrap_or_default();
        return tool_success(id, json!({ "app_id": app_id, "section": s, "content": content }));
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
    let _ = std::fs::write(app_dir.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());
    // Create empty markdown files
    for s in DOCS_SECTIONS {
        let _ = std::fs::write(app_dir.join(format!("{s}.md")), "");
    }
    info!(app_id, "Created docs");
    tool_success(id, json!({ "created": app_id, "sections": ["meta", "structure", "features", "backend", "notes"] }))
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
    let filename = if section == "meta" { "meta.json".to_string() } else { format!("{section}.md") };
    let path = app_dir.join(&filename);
    if let Err(e) = std::fs::write(&path, content) {
        return tool_error(id, &format!("Failed to write: {e}"));
    }
    info!(app_id, section, "Updated docs section");
    tool_success(id, json!({ "app_id": app_id, "section": section, "updated": true }))
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
    tool_success(id, json!({ "query": query, "results": results, "count": results.len() }))
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
        if start > 0 { snippet.push_str("..."); }
        snippet.push_str(&content[start..end]);
        if end < content.len() { snippet.push_str("..."); }
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
    let filled_count = sections.iter().filter(|s| s["filled"].as_bool().unwrap_or(false)).count();
    tool_success(id, json!({
        "app_id": app_id,
        "sections": sections,
        "filled": filled_count,
        "total": 5,
        "complete": filled_count == 5,
    }))
}

// ── Helpers ─────────────────────────────────────────────────────────

// ── Database tools ──────────────────────────────────────────────────

async fn handle_db_tool(id: Value, tool: &str, args: &Value, db: &DbManager) -> Value {
    use hr_db::engine::DataverseEngine;
    use hr_db::query::*;
    use hr_db::schema::*;

    let get_app_id = || -> Result<&str, Value> {
        args.get("app_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tool_error(id.clone(), "app_id required"))
    };

    match tool {
        "db.overview" => {
            match db.overview().await {
                Ok(data) => tool_success(id, data),
                Err(e) => tool_error(id, &e),
            }
        }

        "db.list_tables" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_schema() {
                Ok(schema) => {
                    let tables: Vec<Value> = schema.tables.iter().map(|t| {
                        let rows = engine.count_rows(&t.name).unwrap_or(0);
                        json!({
                            "name": t.name,
                            "slug": t.slug,
                            "columns": t.columns.len(),
                            "rows": rows,
                            "description": t.description,
                        })
                    }).collect();
                    tool_success(id, json!(tables))
                }
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.describe_table" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => return tool_error(id, "table_name required"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_table(name) {
                Ok(Some(table)) => tool_success(id, serde_json::to_value(&table).unwrap_or(json!(null))),
                Ok(None) => tool_error(id, &format!("Table '{}' not found", name)),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.create_table" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => return tool_error(id, "name required"),
            };
            let slug = match args.get("slug").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return tool_error(id, "slug required"),
            };
            let desc = args.get("description").and_then(|v| v.as_str()).map(String::from);
            let cols_val = match args.get("columns") {
                Some(v) => v,
                None => return tool_error(id, "columns required"),
            };
            let columns: Vec<ColumnDefinition> = match serde_json::from_value(cols_val.clone()) {
                Ok(c) => c,
                Err(e) => return tool_error(id, &format!("Invalid columns: {}", e)),
            };

            let now = chrono::Utc::now();
            let table = TableDefinition {
                name: name.clone(),
                slug,
                columns,
                description: desc,
                created_at: now,
                updated_at: now,
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.create_table(&table) {
                Ok(version) => tool_success(id, json!({
                    "message": format!("Table '{}' created (schema version {})", name, version)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.add_column" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let name = match args.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => return tool_error(id, "name required"),
            };
            let ft_str = match args.get("field_type").and_then(|v| v.as_str()) {
                Some(f) => f,
                None => return tool_error(id, "field_type required"),
            };
            let field_type: FieldType = match serde_json::from_str(&format!("\"{}\"", ft_str)) {
                Ok(ft) => ft,
                Err(_) => return tool_error(id, &format!("Invalid field_type: {}", ft_str)),
            };
            let col = ColumnDefinition {
                name: name.clone(),
                field_type,
                required: args.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                unique: args.get("unique").and_then(|v| v.as_bool()).unwrap_or(false),
                default_value: args.get("default_value").and_then(|v| v.as_str()).map(String::from),
                description: None,
                choices: vec![],
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.add_column(table, &col) {
                Ok(version) => tool_success(id, json!({
                    "message": format!("Column '{}' added to '{}' (schema version {})", name, table, version)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.remove_column" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let col = match args.get("column_name").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return tool_error(id, "column_name required"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.remove_column(table, col) {
                Ok(version) => tool_success(id, json!({
                    "message": format!("Column '{}' removed from '{}' (schema version {})", col, table, version)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.drop_table" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => return tool_error(id, "table_name required"),
            };
            let confirm = args.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
            if !confirm {
                return tool_error(id, "Set confirm=true to confirm table deletion");
            }
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.drop_table(name) {
                Ok(version) => tool_success(id, json!({
                    "message": format!("Table '{}' dropped (schema version {})", name, version)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.create_relation" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let rel = RelationDefinition {
                from_table: match args.get("from_table").and_then(|v| v.as_str()) {
                    Some(v) => v.to_string(),
                    None => return tool_error(id, "from_table required"),
                },
                from_column: match args.get("from_column").and_then(|v| v.as_str()) {
                    Some(v) => v.to_string(),
                    None => return tool_error(id, "from_column required"),
                },
                to_table: match args.get("to_table").and_then(|v| v.as_str()) {
                    Some(v) => v.to_string(),
                    None => return tool_error(id, "to_table required"),
                },
                to_column: match args.get("to_column").and_then(|v| v.as_str()) {
                    Some(v) => v.to_string(),
                    None => return tool_error(id, "to_column required"),
                },
                relation_type: match args.get("relation_type").and_then(|v| v.as_str()) {
                    Some(rt) => match serde_json::from_str(&format!("\"{}\"", rt)) {
                        Ok(r) => r,
                        Err(e) => return tool_error(id, &format!("Invalid relation_type: {}", e)),
                    },
                    None => return tool_error(id, "relation_type required"),
                },
                cascade: CascadeRules {
                    on_delete: args.get("on_delete").and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or_default(),
                    on_update: args.get("on_update").and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or_default(),
                },
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.create_relation(&rel) {
                Ok(version) => tool_success(id, json!({
                    "message": format!("Relation {}.{} -> {}.{} created (schema version {})",
                        rel.from_table, rel.from_column, rel.to_table, rel.to_column, version)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.get_schema" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            match db.get_schema(app_id).await {
                Ok(schema) => tool_success(id, schema),
                Err(e) => tool_error(id, &e),
            }
        }

        "db.get_db_info" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_schema() {
                Ok(schema) => {
                    let mut total_rows: u64 = 0;
                    for t in &schema.tables {
                        total_rows += engine.count_rows(&t.name).unwrap_or(0);
                    }
                    let db_path = db.db_dir().join(format!("{}.db", app_id));
                    tool_success(id, json!({
                        "tables": schema.tables.len(),
                        "relations": schema.relations.len(),
                        "total_rows": total_rows,
                        "schema_version": schema.version,
                        "db_size_bytes": DataverseEngine::db_size_bytes(&db_path),
                    }))
                }
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.query_data" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let filters: Vec<Filter> = args.get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let pagination = Pagination {
                limit: args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100),
                offset: args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0),
                order_by: args.get("order_by").and_then(|v| v.as_str()).map(String::from),
                order_desc: args.get("order_desc").and_then(|v| v.as_bool()).unwrap_or(false),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match query_rows(engine.connection(), table, &filters, &pagination) {
                Ok(rows) => tool_success(id, json!(rows)),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.insert_data" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let rows: Vec<Value> = match args.get("rows").and_then(|v| v.as_array()) {
                Some(r) => r.clone(),
                None => return tool_error(id, "rows required (array)"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match insert_rows(engine.connection(), table, &rows) {
                Ok(count) => tool_success(id, json!({
                    "message": format!("{} row(s) inserted into '{}'", count, table)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.update_data" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let updates = match args.get("updates") {
                Some(u) => u,
                None => return tool_error(id, "updates required"),
            };
            let filters: Vec<Filter> = args.get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match update_rows(engine.connection(), table, updates, &filters) {
                Ok(count) => tool_success(id, json!({
                    "message": format!("{} row(s) updated in '{}'", count, table)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.delete_data" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let filters: Vec<Filter> = args.get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match delete_rows(engine.connection(), table, &filters) {
                Ok(count) => tool_success(id, json!({
                    "message": format!("{} row(s) deleted from '{}'", count, table)
                })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.count_rows" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return tool_error(id, "table_name required"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e,
                Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.count_rows(table) {
                Ok(count) => tool_success(id, json!({ "count": count })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        _ => {
            warn!(tool, "Unknown db tool");
            error_response(id, METHOD_NOT_FOUND, format!("Tool not found: {tool}"))
        }
    }
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_success(id: Value, data: Value) -> Value {
    success_response(id, json!({
        "content": [{ "type": "text", "text": data.to_string() }]
    }))
}

fn tool_error(id: Value, message: &str) -> Value {
    success_response(id, json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    }))
}

// ── Environment & Pipeline tool definitions ──────────────────────────

fn tool_definitions_env() -> Value {
    json!([
        // ── Environments ──
        {
            "name": "envs.list",
            "description": "List all environments with status, connected agents, and app counts.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "envs.get",
            "description": "Get detailed info about an environment, including its apps and status.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Environment slug (dev, prod, acc)" } },
                "required": ["slug"]
            }
        },
        {
            "name": "envs.create",
            "description": "Create a new environment (container + env-agent).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name (e.g. 'Development')" },
                    "slug": { "type": "string", "description": "URL-safe slug (e.g. 'dev')" },
                    "env_type": { "type": "string", "description": "Type: development, acceptance, production" },
                    "host_id": { "type": "string", "description": "Host to create on (default: medion)" }
                },
                "required": ["name", "slug", "env_type"]
            }
        },
        {
            "name": "envs.start",
            "description": "Start a stopped environment container.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Environment slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "envs.stop",
            "description": "Stop an environment container (graceful shutdown).",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Environment slug" } },
                "required": ["slug"]
            }
        },
        {
            "name": "envs.destroy",
            "description": "Destroy an environment and its container. Irreversible.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "Environment slug" } },
                "required": ["slug"]
            }
        },
        // ── Pipelines ──
        {
            "name": "pipeline.promote",
            "description": "Promote an app from one environment to another (build → test → migrate DB → deploy → health check).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_slug": { "type": "string", "description": "App to promote (e.g. 'trader')" },
                    "version": { "type": "string", "description": "Version tag (e.g. '2.3.1')" },
                    "source_env": { "type": "string", "description": "Source environment slug (e.g. 'dev')" },
                    "target_env": { "type": "string", "description": "Target environment slug (e.g. 'prod')" }
                },
                "required": ["app_slug", "version", "source_env", "target_env"]
            }
        },
        {
            "name": "pipeline.status",
            "description": "Get the status of a pipeline run.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Pipeline run ID" } },
                "required": ["id"]
            }
        },
        {
            "name": "pipeline.history",
            "description": "List recent pipeline runs.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "pipeline.cancel",
            "description": "Cancel a running pipeline.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Pipeline run ID" } },
                "required": ["id"]
            }
        }
    ])
}

// ── Environment tool handlers ────────────────────────────────────────

async fn tool_envs_list(id: Value, state: &McpState) -> Value {
    let envs = state.env_manager.list_environments().await;
    let result: Vec<Value> = envs.iter().map(|e| {
        json!({
            "id": e.id,
            "name": e.name,
            "slug": e.slug,
            "env_type": e.env_type,
            "host_id": e.host_id,
            "container_name": e.container_name,
            "ipv4_address": e.ipv4_address.map(|ip| ip.to_string()),
            "status": e.status,
            "agent_connected": e.agent_connected,
            "agent_version": e.agent_version,
            "apps_count": e.apps.len(),
            "apps_running": e.apps.iter().filter(|a| a.running).count(),
            "created_at": e.created_at.to_rfc3339(),
        })
    }).collect();
    tool_success(id, json!(result))
}

async fn tool_envs_get(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    match state.env_manager.get_environment(slug).await {
        Some(env) => tool_success(id, serde_json::to_value(&env).unwrap_or(json!(null))),
        None => tool_error(id, &format!("Environment '{}' not found", slug)),
    }
}

async fn tool_envs_create(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing name".into());
    };
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let Some(env_type_str) = args.get("env_type").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing env_type".into());
    };
    let env_type = match env_type_str {
        "development" | "dev" => hr_environment::EnvType::Development,
        "acceptance" | "acc" => hr_environment::EnvType::Acceptance,
        "production" | "prod" => hr_environment::EnvType::Production,
        _ => return error_response(id, INVALID_PARAMS, format!("Invalid env_type: {env_type_str}")),
    };
    let host_id = args.get("host_id").and_then(|v| v.as_str()).unwrap_or("medion");

    match state.env_manager.create_environment(name.into(), slug.into(), env_type, host_id.into()).await {
        Ok((env, token)) => tool_success(id, json!({
            "environment": serde_json::to_value(&env).unwrap_or(json!(null)),
            "token": token,
        })),
        Err(e) => tool_error(id, &format!("Failed to create environment: {e}")),
    }
}

async fn tool_envs_action(id: Value, args: &Value, state: &McpState, action: &str) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };
    let container_name = crate::env_manager::EnvironmentManager::container_name_for_env(slug);

    match action {
        "start" => {
            state.env_manager.update_environment_status(slug, hr_environment::EnvStatus::Provisioning).await;
            match state.container_manager.start_container(&container_name).await {
                Ok(_) => tool_success(id, json!({"action": action, "slug": slug, "status": "ok"})),
                Err(e) => tool_error(id, &format!("Failed to start environment '{}': {}", slug, e)),
            }
        }
        "stop" => {
            let _ = state.env_manager.send_to_env(slug, hr_environment::EnvOrchestratorMessage::Shutdown).await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            state.env_manager.update_environment_status(slug, hr_environment::EnvStatus::Stopped).await;
            match state.container_manager.stop_container(&container_name).await {
                Ok(_) => tool_success(id, json!({"action": action, "slug": slug, "status": "ok"})),
                Err(e) => tool_error(id, &format!("Failed to stop environment '{}': {}", slug, e)),
            }
        }
        _ => tool_error(id, &format!("Unknown action: {action}")),
    }
}

async fn tool_envs_destroy(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    // Disconnect env-agent if connected
    if state.env_manager.is_env_connected(slug).await {
        let _ = state.env_manager.send_to_env(slug, hr_environment::EnvOrchestratorMessage::Shutdown).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Delete the environment record
    if let Err(e) = state.env_manager.delete_environment_by_slug(slug).await {
        return tool_error(id, &format!("Failed to delete environment: {e}"));
    }

    tool_success(id, json!({"destroyed": slug}))
}

// ── Pipeline tool handlers ───────────────────────────────────────────

async fn tool_pipeline_promote(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing app_slug".into());
    };
    let Some(version) = args.get("version").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing version".into());
    };
    let Some(source_env) = args.get("source_env").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing source_env".into());
    };
    let Some(target_env) = args.get("target_env").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing target_env".into());
    };

    // Create a transport adapter that delegates to the env_manager
    let transport = Arc::new(EnvManagerTransport { env_manager: state.env_manager.clone() });

    match state.pipeline_engine.promote(
        &transport,
        app_slug.into(),
        version.into(),
        source_env.into(),
        target_env.into(),
        "mcp".into(),
        None,
    ).await {
        Ok(run) => tool_success(id, serde_json::to_value(&run).unwrap_or(json!(null))),
        Err(e) => tool_error(id, &format!("Failed to start pipeline: {e}")),
    }
}

async fn tool_pipeline_status(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(run_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    match state.pipeline_engine.get_run(run_id).await {
        Some(run) => tool_success(id, serde_json::to_value(&run).unwrap_or(json!(null))),
        None => tool_error(id, &format!("Pipeline run '{}' not found", run_id)),
    }
}

async fn tool_pipeline_history(id: Value, state: &McpState) -> Value {
    let runs = state.pipeline_engine.get_runs().await;
    tool_success(id, serde_json::to_value(&runs).unwrap_or(json!([])))
}

async fn tool_pipeline_cancel(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(run_id) = args.get("id").and_then(|v| v.as_str()) else {
        return error_response(id, INVALID_PARAMS, "Missing id".into());
    };
    match state.pipeline_engine.cancel(run_id).await {
        Ok(()) => tool_success(id, json!({"cancelled": run_id})),
        Err(e) => tool_error(id, &format!("Failed to cancel pipeline: {e}")),
    }
}

// ── PipelineTransport adapter ────────────────────────────────────────

struct EnvManagerTransport {
    env_manager: Arc<crate::env_manager::EnvironmentManager>,
}

impl hr_pipeline::PipelineTransport for EnvManagerTransport {
    async fn send_to_env(
        &self,
        env_slug: &str,
        msg: hr_environment::EnvOrchestratorMessage,
    ) -> anyhow::Result<()> {
        self.env_manager.send_to_env(env_slug, msg).await
    }

    async fn is_env_connected(&self, env_slug: &str) -> bool {
        self.env_manager.is_env_connected(env_slug).await
    }

    async fn get_app_version(
        &self,
        env_slug: &str,
        app_slug: &str,
    ) -> Option<String> {
        self.env_manager.get_app_version(env_slug, app_slug).await
    }
}
