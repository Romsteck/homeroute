//! MCP server for the env-agent (HTTP + stdio).
//!
//! Tool categories:
//!   db.*        — Database operations (via DbManager + hr-db)
//!   app.*       — App lifecycle (via AppSupervisor / systemctl)
//!   env.*       — Environment introspection
//!   studio.*    — Claude Code context management

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use hr_db::schema::*;
use hr_environment::types::EnvPermissions;

use crate::context::ContextGenerator;
use crate::db_manager::DbManager;
use crate::secrets::SecretsManager;
use crate::supervisor::AppSupervisor;

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

const METHOD_NOT_FOUND: i32 = -32601;

// ── Shared state (HTTP handler) ─────────────────────────────────────

#[derive(Clone)]
pub struct McpState {
    pub db: Arc<DbManager>,
    pub supervisor: Arc<AppSupervisor>,
    pub config: Arc<hr_environment::config::EnvAgentConfig>,
    pub context: Arc<ContextGenerator>,
    pub secrets: Arc<SecretsManager>,
}

// ── HTTP Handler ────────────────────────────────────────────────────

pub async fn mcp_handler(
    State(state): State<McpState>,
    body: String,
) -> impl IntoResponse {
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::OK,
                Json(error_response(
                    Value::Null,
                    -32700,
                    format!("Parse error: {e}"),
                )),
            );
        }
    };

    let id = request.id.clone().unwrap_or(Value::Null);

    debug!(method = %request.method, "MCP request");

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id, &state),
        "notifications/initialized" => return (StatusCode::OK, Json(json!({}))),
        "tools/list" => handle_tools_list(id, &state),
        "tools/call" => handle_tools_call(id, request.params, &state).await,
        _ => error_response(
            id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    };

    (StatusCode::OK, Json(response))
}

// ── Initialize ──────────────────────────────────────────────────────

fn handle_initialize(id: Value, state: &McpState) -> Value {
    let env_type = state.config.env_type();
    success_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "env-agent",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": format!(
                "env-agent MCP server for environment '{}' (type: {}).\n\
                 Tools: db.* (database), app.* (lifecycle), env.* (introspection), studio.* (context).\n\
                 Permissions are enforced based on the environment type.",
                state.config.env_slug, env_type
            )
        }),
    )
}

// ── Tools list ──────────────────────────────────────────────────────

fn handle_tools_list(id: Value, state: &McpState) -> Value {
    let perms = EnvPermissions::for_type(state.config.env_type());
    let tools = build_tool_definitions(&perms, true);
    success_response(id, json!({ "tools": tools }))
}

/// Build tool definitions. When `include_studio` is true, includes studio.* tools
/// (only meaningful for the HTTP handler which has access to ContextGenerator).
fn build_tool_definitions(perms: &EnvPermissions, include_studio: bool) -> Value {
    let mut tools = vec![
        // ── env.* ──
        json!({
            "name": "env.info",
            "description": "Get environment info: slug, type, permissions, configured apps.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "env.permissions",
            "description": "Get the permission matrix for this environment type.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        // ── app.* ──
        json!({
            "name": "app.list",
            "description": "List all configured apps with status (running/stopped/failed).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "app.status",
            "description": "Get the status of a specific app.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "App slug" } },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "app.start",
            "description": "Start an app's systemd service.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "App slug" } },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "app.stop",
            "description": "Stop an app's systemd service.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "App slug" } },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "app.restart",
            "description": "Restart an app's systemd service.",
            "inputSchema": {
                "type": "object",
                "properties": { "slug": { "type": "string", "description": "App slug" } },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "app.logs",
            "description": "Get recent log lines for an app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "App slug" },
                    "lines": { "type": "integer", "description": "Number of lines (default 100)" }
                },
                "required": ["slug"]
            }
        }),
    ];

    // Only the HTTP handler has ContextGenerator access
    if include_studio {
        tools.extend([
            json!({
                "name": "app.health",
                "description": "Perform an HTTP health check on an app.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "slug": { "type": "string", "description": "App slug" } },
                    "required": ["slug"]
                }
            }),
            json!({
                "name": "studio.refresh_context",
                "description": "Regenerate CLAUDE.md and .claude/ files for an app.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "slug": { "type": "string", "description": "App slug" } },
                    "required": ["slug"]
                }
            }),
            json!({
                "name": "studio.refresh_all",
                "description": "Regenerate context files for all apps in the environment.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
        ]);
    }

    // ── secrets.* ──
    tools.extend([
        json!({
            "name": "secrets.list",
            "description": "List secret keys stored for an app (values not shown).",
            "inputSchema": {
                "type": "object",
                "properties": { "app_slug": { "type": "string", "description": "App slug" } },
                "required": ["app_slug"]
            }
        }),
        json!({
            "name": "secrets.get",
            "description": "Get a secret value for an app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_slug": { "type": "string", "description": "App slug" },
                    "key": { "type": "string", "description": "Secret key name" }
                },
                "required": ["app_slug", "key"]
            }
        }),
    ]);

    // secrets.set and secrets.delete only in dev/acc envs
    if perms.env_vars_write {
        tools.extend([
            json!({
                "name": "secrets.set",
                "description": "Set a secret for an app. Only allowed in dev/acc environments.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_slug": { "type": "string", "description": "App slug" },
                        "key": { "type": "string", "description": "Secret key name" },
                        "value": { "type": "string", "description": "Secret value" }
                    },
                    "required": ["app_slug", "key", "value"]
                }
            }),
            json!({
                "name": "secrets.delete",
                "description": "Delete a secret for an app. Only allowed in dev/acc environments.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_slug": { "type": "string", "description": "App slug" },
                        "key": { "type": "string", "description": "Secret key name" }
                    },
                    "required": ["app_slug", "key"]
                }
            }),
        ]);
    }

    // ── db.* (read-only always available) ──
    tools.extend([
        json!({
            "name": "db.overview",
            "description": "List all apps with databases, table counts and sizes.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "db.list_tables",
            "description": "List tables in an app's database with column/row counts.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        }),
        json!({
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
        }),
        json!({
            "name": "db.get_schema",
            "description": "Get the full database schema as JSON (tables, columns, relations).",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        }),
        json!({
            "name": "db.get_db_info",
            "description": "Get database statistics: table count, total rows, schema version.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        }),
        json!({
            "name": "db.query_data",
            "description": "Query rows from a table with optional filters, pagination, sorting.",
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
                                "op": { "type": "string", "description": "eq, ne, gt, lt, gte, lte, like, in, is_null, is_not_null" },
                                "value": {}
                            },
                            "required": ["column", "op"]
                        }
                    },
                    "limit": { "type": "integer", "description": "Max rows (default 100)" },
                    "offset": { "type": "integer", "description": "Skip N rows" },
                    "order_by": { "type": "string", "description": "Column to sort by" },
                    "order_desc": { "type": "boolean", "description": "Sort descending" }
                },
                "required": ["app_id", "table_name"]
            }
        }),
        json!({
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
        }),
        json!({
            "name": "db.snapshot",
            "description": "Create a backup snapshot of an app's database.",
            "inputSchema": {
                "type": "object",
                "properties": { "app_id": { "type": "string", "description": "App slug" } },
                "required": ["app_id"]
            }
        }),
    ]);

    // ── db schema write tools ──
    if perms.db_schema_write {
        tools.extend([
            json!({
                "name": "db.create_table",
                "description": "Create a new table with columns. Auto-adds id, created_at, updated_at.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string", "description": "App slug" },
                        "name": { "type": "string", "description": "Table name (snake_case)" },
                        "slug": { "type": "string", "description": "URL slug (kebab-case)" },
                        "description": { "type": "string" },
                        "columns": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "field_type": { "type": "string", "description": "text, number, decimal, boolean, date_time, etc." },
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
            }),
            json!({
                "name": "db.add_column",
                "description": "Add a column to an existing table.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "name": { "type": "string" },
                        "field_type": { "type": "string" },
                        "required": { "type": "boolean" },
                        "unique": { "type": "boolean" },
                        "default_value": { "type": "string" }
                    },
                    "required": ["app_id", "table_name", "name", "field_type"]
                }
            }),
            json!({
                "name": "db.remove_column",
                "description": "Remove a column from a table.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "column_name": { "type": "string" }
                    },
                    "required": ["app_id", "table_name", "column_name"]
                }
            }),
            json!({
                "name": "db.drop_table",
                "description": "Drop a table and all its data. Requires confirm=true.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "confirm": { "type": "boolean", "description": "Must be true" }
                    },
                    "required": ["app_id", "table_name", "confirm"]
                }
            }),
            json!({
                "name": "db.create_relation",
                "description": "Create a foreign key relationship between two tables.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "from_table": { "type": "string" },
                        "from_column": { "type": "string" },
                        "to_table": { "type": "string" },
                        "to_column": { "type": "string" },
                        "relation_type": { "type": "string", "description": "one_to_many, many_to_many, or self_referential" },
                        "on_delete": { "type": "string" },
                        "on_update": { "type": "string" }
                    },
                    "required": ["app_id", "from_table", "from_column", "to_table", "to_column", "relation_type"]
                }
            }),
        ]);
    }

    // ── db data write tools ──
    if perms.db_data_write {
        tools.extend([
            json!({
                "name": "db.insert_data",
                "description": "Insert one or more rows into a table.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "rows": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["app_id", "table_name", "rows"]
                }
            }),
            json!({
                "name": "db.update_data",
                "description": "Update rows matching filters.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "updates": { "type": "object" },
                        "filters": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": { "column": { "type": "string" }, "op": { "type": "string" }, "value": {} },
                                "required": ["column", "op"]
                            }
                        }
                    },
                    "required": ["app_id", "table_name", "updates", "filters"]
                }
            }),
            json!({
                "name": "db.delete_data",
                "description": "Delete rows matching filters.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "app_id": { "type": "string" },
                        "table_name": { "type": "string" },
                        "filters": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": { "column": { "type": "string" }, "op": { "type": "string" }, "value": {} },
                                "required": ["column", "op"]
                            }
                        }
                    },
                    "required": ["app_id", "table_name", "filters"]
                }
            }),
        ]);
    }

    json!(tools)
}

// ── Tool dispatch (HTTP) ────────────────────────────────────────────

async fn handle_tools_call(id: Value, params: Value, state: &McpState) -> Value {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return error_response(id, -32602, "Missing tool name".into()),
    };
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    debug!(tool = tool_name, "Tool call");

    match tool_name {
        "env.info" => tool_env_info(id, state),
        "env.permissions" => tool_env_permissions(id, state),
        "app.list" => tool_app_list(id, state).await,
        "app.status" => tool_app_status(id, &arguments, state).await,
        "app.health" => tool_app_health(id, &arguments, state).await,
        "app.logs" => tool_app_logs(id, &arguments, state).await,
        "app.start" => tool_app_start(id, &arguments, state).await,
        "app.stop" => tool_app_stop(id, &arguments, state).await,
        "app.restart" => tool_app_restart(id, &arguments, state).await,
        // ── secrets.* ──
        "secrets.list" => tool_secrets_list(id, &arguments, state),
        "secrets.get" => tool_secrets_get(id, &arguments, state),
        "secrets.set" => tool_secrets_set(id, &arguments, state),
        "secrets.delete" => tool_secrets_delete(id, &arguments, state),
        t if t.starts_with("db.") => {
            let perms = EnvPermissions::for_type(state.config.env_type());
            handle_db_tool_inner(id, t, &arguments, &state.db, &perms).await
        }
        "studio.refresh_context" => tool_studio_refresh(id, &arguments, state).await,
        "studio.refresh_all" => tool_studio_refresh_all(id, state).await,
        _ => {
            warn!(tool = tool_name, "Unknown tool");
            error_response(id, METHOD_NOT_FOUND, format!("Tool not found: {tool_name}"))
        }
    }
}

// ── env tools (HTTP) ────────────────────────────────────────────────

fn tool_env_info(id: Value, state: &McpState) -> Value {
    let cfg = &state.config;
    let perms = EnvPermissions::for_type(cfg.env_type());
    tool_success(
        id,
        json!({
            "env_slug": cfg.env_slug,
            "env_type": cfg.env_type().to_string(),
            "mcp_port": cfg.mcp_port,
            "apps_path": cfg.apps_path,
            "db_path": cfg.db_path,
            "apps": cfg.apps.iter().map(|a| json!({
                "slug": &a.slug,
                "name": &a.name,
                "port": a.port,
                "stack": format!("{:?}", a.stack),
                "has_db": a.has_db,
            })).collect::<Vec<_>>(),
            "permissions": serde_json::to_value(&perms).unwrap_or(json!(null)),
        }),
    )
}

fn tool_env_permissions(id: Value, state: &McpState) -> Value {
    let perms = EnvPermissions::for_type(state.config.env_type());
    tool_success(id, serde_json::to_value(&perms).unwrap_or(json!(null)))
}

// ── app tools (HTTP, via AppSupervisor) ─────────────────────────────

async fn tool_app_list(id: Value, state: &McpState) -> Value {
    let apps = state.supervisor.list_apps().await;
    tool_success(id, json!(apps))
}

async fn tool_app_status(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.supervisor.app_status(slug).await {
        Ok(status) => tool_success(id, json!({ "slug": slug, "status": status.to_string() })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_app_health(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.supervisor.health_check(slug).await {
        Ok(healthy) => tool_success(id, json!({ "slug": slug, "healthy": healthy })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_app_logs(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as u32;
    match state.supervisor.logs(slug, lines).await {
        Ok(output) => tool_success(id, json!({ "slug": slug, "logs": output })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_app_start(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.supervisor.start_app(slug).await {
        Ok(()) => tool_success(id, json!({ "message": format!("App '{}' started", slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_app_stop(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.supervisor.stop_app(slug).await {
        Ok(()) => tool_success(id, json!({ "message": format!("App '{}' stopped", slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_app_restart(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.supervisor.restart_app(slug).await {
        Ok(()) => tool_success(id, json!({ "message": format!("App '{}' restarted", slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

// ── secrets tools (HTTP) ────────────────────────────────────────────

fn tool_secrets_list(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    match state.secrets.list(app_slug) {
        Ok(keys) => tool_success(id, json!({ "app_slug": app_slug, "keys": keys })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn tool_secrets_get(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    match state.secrets.get(app_slug, key) {
        Ok(Some(value)) => tool_success(id, json!({ "app_slug": app_slug, "key": key, "value": value })),
        Ok(None) => tool_error(id, &format!("Secret '{}' not found for app '{}'", key, app_slug)),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn tool_secrets_set(id: Value, args: &Value, state: &McpState) -> Value {
    let perms = EnvPermissions::for_type(state.config.env_type());
    if !perms.env_vars_write {
        return tool_error(id, "Secrets writes not allowed in this environment type");
    }
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    let Some(value) = args.get("value").and_then(|v| v.as_str()) else {
        return tool_error(id, "value required");
    };
    match state.secrets.set(app_slug, key, value) {
        Ok(()) => tool_success(id, json!({ "message": format!("Secret '{}' set for app '{}'", key, app_slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn tool_secrets_delete(id: Value, args: &Value, state: &McpState) -> Value {
    let perms = EnvPermissions::for_type(state.config.env_type());
    if !perms.env_vars_write {
        return tool_error(id, "Secrets writes not allowed in this environment type");
    }
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    match state.secrets.delete(app_slug, key) {
        Ok(true) => tool_success(id, json!({ "message": format!("Secret '{}' deleted for app '{}'", key, app_slug) })),
        Ok(false) => tool_error(id, &format!("Secret '{}' not found for app '{}'", key, app_slug)),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

// ── studio tools (HTTP only) ────────────────────────────────────────

async fn tool_studio_refresh(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    match state.context.generate_for_app_by_slug(slug, &state.config.apps, &state.db).await {
        Ok(()) => tool_success(id, json!({ "message": format!("Context refreshed for '{}'", slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

async fn tool_studio_refresh_all(id: Value, state: &McpState) -> Value {
    match state.context.generate_all_with_db(&state.config.apps, &state.db).await {
        Ok(count) => tool_success(id, json!({ "message": format!("Context refreshed for {} apps", count) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

// ── Shared db tool implementation ───────────────────────────────────

async fn handle_db_tool_inner(
    id: Value,
    tool: &str,
    args: &Value,
    db: &DbManager,
    perms: &EnvPermissions,
) -> Value {
    use hr_db::query::*;

    let get_app_id = || -> std::result::Result<&str, Value> {
        args.get("app_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tool_error(id.clone(), "app_id required"))
    };

    match tool {
        "db.overview" => match db.overview().await {
            Ok(data) => tool_success(id, data),
            Err(e) => tool_error(id, &e),
        },

        "db.list_tables" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e, Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_schema() {
                Ok(schema) => {
                    let tables: Vec<Value> = schema.tables.iter().map(|t| {
                        let rows = engine.count_rows(&t.name).unwrap_or(0);
                        json!({ "name": t.name, "slug": t.slug, "columns": t.columns.len(), "rows": rows, "description": t.description })
                    }).collect();
                    tool_success(id, json!(tables))
                }
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.describe_table" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(n) => n, None => return tool_error(id, "table_name required"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e, Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_table(name) {
                Ok(Some(table)) => tool_success(id, serde_json::to_value(&table).unwrap_or(json!(null))),
                Ok(None) => tool_error(id, &format!("Table '{}' not found", name)),
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
                Ok(e) => e, Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.get_schema() {
                Ok(schema) => {
                    let mut total_rows: u64 = 0;
                    for t in &schema.tables {
                        total_rows += engine.count_rows(&t.name).unwrap_or(0);
                    }
                    let db_path = db.db_path_for(app_id);
                    tool_success(id, json!({
                        "tables": schema.tables.len(),
                        "relations": schema.relations.len(),
                        "total_rows": total_rows,
                        "schema_version": schema.version,
                        "db_size_bytes": hr_db::engine::DataverseEngine::db_size_bytes(&db_path),
                    }))
                }
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.query_data" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
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
                Ok(e) => e, Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match query_rows(engine.connection(), table, &filters, &pagination) {
                Ok(rows) => tool_success(id, json!(rows)),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.count_rows" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let engine = match db.get_engine(app_id).await {
                Ok(e) => e, Err(e) => return tool_error(id, &e),
            };
            let engine = engine.lock().await;
            match engine.count_rows(table) {
                Ok(count) => tool_success(id, json!({ "count": count })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.snapshot" => {
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            match db.snapshot_db(app_id).await {
                Ok(path) => tool_success(id, json!({ "message": format!("Snapshot created: {}", path.display()) })),
                Err(e) => tool_error(id, &e),
            }
        }

        // ── Schema write tools ──
        "db.create_table" => {
            if !perms.db_schema_write { return tool_error(id, "Schema writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(), None => return tool_error(id, "name required"),
            };
            let slug = match args.get("slug").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(), None => return tool_error(id, "slug required"),
            };
            let desc = args.get("description").and_then(|v| v.as_str()).map(String::from);
            let columns: Vec<ColumnDefinition> = match args.get("columns").map(|v| serde_json::from_value(v.clone())) {
                Some(Ok(c)) => c, Some(Err(e)) => return tool_error(id, &format!("Invalid columns: {}", e)),
                None => return tool_error(id, "columns required"),
            };
            let now = chrono::Utc::now();
            let table = TableDefinition { name: name.clone(), slug, columns, description: desc, created_at: now, updated_at: now };
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match engine.create_table(&table) {
                Ok(version) => tool_success(id, json!({ "message": format!("Table '{}' created (v{})", name, version) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.add_column" => {
            if !perms.db_schema_write { return tool_error(id, "Schema writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let name = match args.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(), None => return tool_error(id, "name required"),
            };
            let ft_str = match args.get("field_type").and_then(|v| v.as_str()) {
                Some(f) => f, None => return tool_error(id, "field_type required"),
            };
            let field_type: FieldType = match serde_json::from_str(&format!("\"{}\"", ft_str)) {
                Ok(ft) => ft, Err(_) => return tool_error(id, &format!("Invalid field_type: {}", ft_str)),
            };
            let col = ColumnDefinition {
                name: name.clone(), field_type,
                required: args.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                unique: args.get("unique").and_then(|v| v.as_bool()).unwrap_or(false),
                default_value: args.get("default_value").and_then(|v| v.as_str()).map(String::from),
                description: None, choices: vec![],
            };
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match engine.add_column(table, &col) {
                Ok(version) => tool_success(id, json!({ "message": format!("Column '{}' added to '{}' (v{})", name, table, version) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.remove_column" => {
            if !perms.db_schema_write { return tool_error(id, "Schema writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let col = match args.get("column_name").and_then(|v| v.as_str()) {
                Some(c) => c, None => return tool_error(id, "column_name required"),
            };
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match engine.remove_column(table, col) {
                Ok(version) => tool_success(id, json!({ "message": format!("Column '{}' removed from '{}' (v{})", col, table, version) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.drop_table" => {
            if !perms.db_schema_write { return tool_error(id, "Schema writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let name = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(n) => n, None => return tool_error(id, "table_name required"),
            };
            if !args.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false) {
                return tool_error(id, "Set confirm=true to confirm table deletion");
            }
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match engine.drop_table(name) {
                Ok(version) => tool_success(id, json!({ "message": format!("Table '{}' dropped (v{})", name, version) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.create_relation" => {
            if !perms.db_schema_write { return tool_error(id, "Schema writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let rel = RelationDefinition {
                from_table: match args.get("from_table").and_then(|v| v.as_str()) { Some(v) => v.to_string(), None => return tool_error(id, "from_table required") },
                from_column: match args.get("from_column").and_then(|v| v.as_str()) { Some(v) => v.to_string(), None => return tool_error(id, "from_column required") },
                to_table: match args.get("to_table").and_then(|v| v.as_str()) { Some(v) => v.to_string(), None => return tool_error(id, "to_table required") },
                to_column: match args.get("to_column").and_then(|v| v.as_str()) { Some(v) => v.to_string(), None => return tool_error(id, "to_column required") },
                relation_type: match args.get("relation_type").and_then(|v| v.as_str()) {
                    Some(rt) => match serde_json::from_str(&format!("\"{}\"", rt)) { Ok(r) => r, Err(e) => return tool_error(id, &format!("Invalid relation_type: {}", e)) },
                    None => return tool_error(id, "relation_type required"),
                },
                cascade: CascadeRules {
                    on_delete: args.get("on_delete").and_then(|v| v.as_str()).and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()).unwrap_or_default(),
                    on_update: args.get("on_update").and_then(|v| v.as_str()).and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()).unwrap_or_default(),
                },
            };
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match engine.create_relation(&rel) {
                Ok(version) => tool_success(id, json!({ "message": format!("Relation {}.{} -> {}.{} created (v{})", rel.from_table, rel.from_column, rel.to_table, rel.to_column, version) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        // ── Data write tools ──
        "db.insert_data" => {
            if !perms.db_data_write { return tool_error(id, "Data writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let rows: Vec<Value> = match args.get("rows").and_then(|v| v.as_array()) {
                Some(r) => r.clone(), None => return tool_error(id, "rows required (array)"),
            };
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match insert_rows(engine.connection(), table, &rows) {
                Ok(count) => tool_success(id, json!({ "message": format!("{} row(s) inserted into '{}'", count, table) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.update_data" => {
            if !perms.db_data_write { return tool_error(id, "Data writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let updates = match args.get("updates") { Some(u) => u, None => return tool_error(id, "updates required") };
            let filters: Vec<Filter> = args.get("filters").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match update_rows(engine.connection(), table, updates, &filters) {
                Ok(count) => tool_success(id, json!({ "message": format!("{} row(s) updated in '{}'", count, table) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        "db.delete_data" => {
            if !perms.db_data_write { return tool_error(id, "Data writes not allowed in this env"); }
            let app_id = match get_app_id() { Ok(v) => v, Err(e) => return e };
            let table = match args.get("table_name").and_then(|v| v.as_str()) {
                Some(t) => t, None => return tool_error(id, "table_name required"),
            };
            let filters: Vec<Filter> = args.get("filters").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            let engine = match db.get_engine(app_id).await { Ok(e) => e, Err(e) => return tool_error(id, &e) };
            let engine = engine.lock().await;
            match delete_rows(engine.connection(), table, &filters) {
                Ok(count) => tool_success(id, json!({ "message": format!("{} row(s) deleted from '{}'", count, table) })),
                Err(e) => tool_error(id, &e.to_string()),
            }
        }

        _ => {
            warn!(tool, "Unknown db tool");
            error_response(id, METHOD_NOT_FOUND, format!("Tool not found: {tool}"))
        }
    }
}

// ══════════════════════════════════════════════════════════════════════
// ── MCP stdio server (invoked via `env-agent mcp`) ──────────────────
// ══════════════════════════════════════════════════════════════════════

const STDIO_CONFIG_PATH: &str = "/etc/env-agent.toml";

/// Run the full MCP stdio server with all tools.
///
/// Loads config from /etc/env-agent.toml, creates its own DbManager,
/// and handles JSON-RPC requests on stdin/stdout.
pub async fn run_mcp_server() -> Result<()> {
    use std::io::{self, BufRead, Write};

    info!("Starting MCP stdio server");

    // ── Load config ──
    let config = hr_environment::config::EnvAgentConfig::load(STDIO_CONFIG_PATH)?;
    let env_type = config.env_type();
    let perms = EnvPermissions::for_type(env_type);

    // ── Create DbManager ──
    let db = DbManager::new(&PathBuf::from(&config.db_path));

    // ── Create SecretsManager ──
    let data_path = PathBuf::from(&config.db_path).parent().unwrap_or(std::path::Path::new("/var/lib/env-agent")).to_path_buf();
    let secrets = SecretsManager::new(&data_path.join("secrets"));

    // ── Build tool definitions (same as HTTP, but without studio.* tools) ──
    let tools = build_tool_definitions(&perms, false);

    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {}", e)}});
                writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
                continue;
            }
        };
        let id = request.id.clone().unwrap_or(Value::Null);

        let result = match request.method.as_str() {
            "initialize" => success_response(
                id.clone(),
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "env-agent", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": format!(
                        "env-agent MCP server for environment '{}' (type: {}).\n\
                         Tools: db.* (database), app.* (lifecycle), env.* (introspection).\n\
                         Permissions are enforced based on the environment type ({}).\n\
                         {} apps configured.",
                        config.env_slug, env_type, env_type, config.apps.len()
                    )
                }),
            ),
            "notifications/initialized" => continue,
            "tools/list" => success_response(id.clone(), json!({ "tools": tools })),
            "tools/call" => {
                let tool_name = request.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = request.params.get("arguments").cloned().unwrap_or(json!({}));
                stdio_handle_tool(id.clone(), tool_name, &arguments, &config, &db, &perms, &secrets).await
            }
            _ => error_response(id.clone(), METHOD_NOT_FOUND, format!("Method not found: {}", request.method)),
        };

        writeln!(&stdout, "{}", serde_json::to_string(&result)?)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

// ── stdio tool dispatch ─────────────────────────────────────────────

async fn stdio_handle_tool(
    id: Value,
    tool_name: &str,
    args: &Value,
    config: &hr_environment::config::EnvAgentConfig,
    db: &DbManager,
    perms: &EnvPermissions,
    secrets: &SecretsManager,
) -> Value {
    match tool_name {
        // ── env.* ──
        "env.info" => {
            tool_success(
                id,
                json!({
                    "env_slug": config.env_slug,
                    "env_type": config.env_type().to_string(),
                    "mcp_port": config.mcp_port,
                    "apps_path": config.apps_path,
                    "db_path": config.db_path,
                    "apps": config.apps.iter().map(|a| json!({
                        "slug": &a.slug,
                        "name": &a.name,
                        "port": a.port,
                        "stack": format!("{:?}", a.stack),
                        "has_db": a.has_db,
                    })).collect::<Vec<_>>(),
                    "permissions": serde_json::to_value(perms).unwrap_or(json!(null)),
                }),
            )
        }

        "env.permissions" => {
            tool_success(id, serde_json::to_value(perms).unwrap_or(json!(null)))
        }

        // ── app.* (via direct systemctl/journalctl since we have no supervisor instance) ──
        "app.list" => stdio_app_list(id, config).await,
        "app.status" => stdio_app_status(id, args, config).await,
        "app.start" => stdio_app_action(id, args, config, "start").await,
        "app.stop" => stdio_app_action(id, args, config, "stop").await,
        "app.restart" => stdio_app_action(id, args, config, "restart").await,
        "app.logs" => stdio_app_logs(id, args, config).await,

        // ── secrets.* ──
        "secrets.list" => stdio_secrets_list(id, args, secrets),
        "secrets.get" => stdio_secrets_get(id, args, secrets),
        "secrets.set" => stdio_secrets_set(id, args, secrets, perms),
        "secrets.delete" => stdio_secrets_delete(id, args, secrets, perms),

        // ── db.* ──
        t if t.starts_with("db.") => handle_db_tool_inner(id, t, args, db, perms).await,

        _ => {
            warn!(tool = tool_name, "Unknown tool");
            error_response(id, METHOD_NOT_FOUND, format!("Tool not found: {tool_name}"))
        }
    }
}

// ── stdio app tools (direct systemctl) ──────────────────────────────

/// Find an app in config by slug.
fn find_app_config<'a>(
    config: &'a hr_environment::config::EnvAgentConfig,
    slug: &str,
) -> std::result::Result<&'a hr_environment::config::EnvAgentAppConfig, String> {
    config
        .apps
        .iter()
        .find(|a| a.slug == slug)
        .ok_or_else(|| format!("App not found: {}", slug))
}

/// Get the systemctl active state for a service.
async fn systemctl_is_active(service: &str) -> String {
    use tokio::process::Command;
    let output = Command::new("systemctl")
        .args(["is-active", service])
        .output()
        .await;
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

async fn stdio_app_list(
    id: Value,
    config: &hr_environment::config::EnvAgentConfig,
) -> Value {
    let mut apps = Vec::new();
    for app in &config.apps {
        let svc = format!("{}.service", app.slug);
        let state = systemctl_is_active(&svc).await;
        let status = match state.as_str() {
            "active" => "running",
            "inactive" | "deactivating" => "stopped",
            "failed" => "failed",
            _ => "unknown",
        };
        apps.push(json!({
            "slug": app.slug,
            "name": app.name,
            "port": app.port,
            "status": status,
            "has_db": app.has_db,
        }));
    }
    tool_success(id, json!(apps))
}

async fn stdio_app_status(
    id: Value,
    args: &Value,
    config: &hr_environment::config::EnvAgentConfig,
) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    if let Err(e) = find_app_config(config, slug) {
        return tool_error(id, &e);
    }
    let svc = format!("{}.service", slug);
    let state = systemctl_is_active(&svc).await;
    let status = match state.as_str() {
        "active" => "running",
        "inactive" | "deactivating" => "stopped",
        "failed" => "failed",
        _ => "unknown",
    };
    tool_success(id, json!({ "slug": slug, "status": status }))
}

async fn stdio_app_action(
    id: Value,
    args: &Value,
    config: &hr_environment::config::EnvAgentConfig,
    action: &str,
) -> Value {
    use tokio::process::Command;

    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    if let Err(e) = find_app_config(config, slug) {
        return tool_error(id, &e);
    }
    let svc = format!("{}.service", slug);
    let output = Command::new("systemctl")
        .args([action, &svc])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let verb = match action {
                "start" => "started",
                "stop" => "stopped",
                "restart" => "restarted",
                _ => action,
            };
            tool_success(id, json!({ "message": format!("App '{}' {}", slug, verb) }))
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tool_error(id, &format!("systemctl {} failed: {}", action, stderr.trim()))
        }
        Err(e) => tool_error(id, &format!("Failed to run systemctl: {}", e)),
    }
}

async fn stdio_app_logs(
    id: Value,
    args: &Value,
    config: &hr_environment::config::EnvAgentConfig,
) -> Value {
    use tokio::process::Command;

    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "slug required");
    };
    if let Err(e) = find_app_config(config, slug) {
        return tool_error(id, &e);
    }
    let lines = args
        .get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(500);
    let svc = format!("{}.service", slug);
    let output = Command::new("journalctl")
        .args(["-u", &svc, "--no-pager", "-n", &lines.to_string()])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let logs = String::from_utf8_lossy(&o.stdout).to_string();
            tool_success(id, json!({ "slug": slug, "logs": logs }))
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tool_error(id, &format!("journalctl failed: {}", stderr.trim()))
        }
        Err(e) => tool_error(id, &format!("Failed to run journalctl: {}", e)),
    }
}

// ── stdio secrets tools ──────────────────────────────────────────────

fn stdio_secrets_list(id: Value, args: &Value, secrets: &SecretsManager) -> Value {
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    match secrets.list(app_slug) {
        Ok(keys) => tool_success(id, json!({ "app_slug": app_slug, "keys": keys })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn stdio_secrets_get(id: Value, args: &Value, secrets: &SecretsManager) -> Value {
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    match secrets.get(app_slug, key) {
        Ok(Some(value)) => tool_success(id, json!({ "app_slug": app_slug, "key": key, "value": value })),
        Ok(None) => tool_error(id, &format!("Secret '{}' not found for app '{}'", key, app_slug)),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn stdio_secrets_set(
    id: Value,
    args: &Value,
    secrets: &SecretsManager,
    perms: &EnvPermissions,
) -> Value {
    if !perms.env_vars_write {
        return tool_error(id, "Secrets writes not allowed in this environment type");
    }
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    let Some(value) = args.get("value").and_then(|v| v.as_str()) else {
        return tool_error(id, "value required");
    };
    match secrets.set(app_slug, key, value) {
        Ok(()) => tool_success(id, json!({ "message": format!("Secret '{}' set for app '{}'", key, app_slug) })),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

fn stdio_secrets_delete(
    id: Value,
    args: &Value,
    secrets: &SecretsManager,
    perms: &EnvPermissions,
) -> Value {
    if !perms.env_vars_write {
        return tool_error(id, "Secrets writes not allowed in this environment type");
    }
    let Some(app_slug) = args.get("app_slug").and_then(|v| v.as_str()) else {
        return tool_error(id, "app_slug required");
    };
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return tool_error(id, "key required");
    };
    match secrets.delete(app_slug, key) {
        Ok(true) => tool_success(id, json!({ "message": format!("Secret '{}' deleted for app '{}'", key, app_slug) })),
        Ok(false) => tool_error(id, &format!("Secret '{}' not found for app '{}'", key, app_slug)),
        Err(e) => tool_error(id, &e.to_string()),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_success(id: Value, data: Value) -> Value {
    success_response(id, json!({ "content": [{ "type": "text", "text": data.to_string() }] }))
}

fn tool_error(id: Value, message: &str) -> Value {
    success_response(id, json!({ "content": [{ "type": "text", "text": message }], "isError": true }))
}
