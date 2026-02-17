//! MCP (Model Context Protocol) stdio server for Dataverse operations.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::info;

use hr_dataverse::engine::DataverseEngine;
use hr_dataverse::query::*;
use hr_dataverse::schema::*;
use hr_registry::protocol::{AgentMessage, AppSchemaOverview};

use hr_registry::types::Environment;

use crate::dataverse::LocalDataverse;

/// Shared map for pending schema query responses.
/// The MCP tool registers a oneshot sender here before sending the request,
/// and the main WebSocket loop resolves it when the response arrives.
pub type SchemaQuerySignals =
    Arc<RwLock<HashMap<String, oneshot::Sender<Vec<AppSchemaOverview>>>>>;

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Context for deploy tools (only available in Development environments).
#[derive(Clone)]
pub struct DeployContext {
    pub app_id: String,
    pub api_base_url: String,
    pub environment: Environment,
}

/// Context for store tools (available in all environments).
#[derive(Clone)]
pub struct StoreContext {
    pub app_id: String,
    pub api_base_url: String,
}

/// Run the MCP stdio server for Dataverse tools.
///
/// When `outbound_tx` and `schema_signals` are provided, the server can
/// send requests to the registry via the WebSocket and wait for responses
/// (used by the `list_other_apps_schemas` tool).
pub async fn run_mcp_server_with_registry(
    outbound_tx: Option<mpsc::Sender<AgentMessage>>,
    schema_signals: Option<SchemaQuerySignals>,
) -> Result<()> {
    info!("Starting MCP Dataverse server");

    let dataverse = LocalDataverse::open()?;
    let engine = dataverse.engine().clone();

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
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
                continue;
            }
        };

        let id = request.id.clone().unwrap_or(Value::Null);

        let result = match request.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "hr-dataverse",
                    "version": "0.1.0"
                },
                "instructions": include_str!("mcp_instructions.txt")
            })),
            "notifications/initialized" => {
                // No response needed for notifications
                continue;
            }
            "tools/list" => {
                let tools = get_tool_definitions();
                Ok(json!({ "tools": tools }))
            },
            "tools/call" => {
                let tool_name = request
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));

                // Registry-backed tools (async, no engine lock needed)
                if tool_name == "list_other_apps_schemas" {
                    handle_list_other_apps_schemas(
                        outbound_tx.as_ref(),
                        schema_signals.as_ref(),
                    )
                    .await
                } else {
                    // Local Dataverse tools (need engine lock)
                    let engine_guard = engine.lock().await;
                    let res = handle_tool_call(&engine_guard, tool_name, &arguments);
                    drop(engine_guard);
                    res
                }
            }
            _ => Err(format!("Method not found: {}", request.method)),
        };

        let resp = match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(value),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                }),
            },
        };

        writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

/// Run the MCP stdio server without registry communication (standalone mode).
pub async fn run_mcp_server() -> Result<()> {
    run_mcp_server_with_registry(None, None).await
}

/// Run the Deploy MCP stdio server (separate from Dataverse).
/// Only exposes deploy, deploy_status, and prod_logs tools.
pub async fn run_deploy_mcp_server(ctx: DeployContext) -> Result<()> {
    info!("Starting MCP Deploy server");

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
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
                continue;
            }
        };

        let id = request.id.clone().unwrap_or(Value::Null);

        let result = match request.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "hr-deploy",
                    "version": "0.1.0"
                },
                "instructions": "Deploy tools for pushing builds from development to production containers."
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                let tools = get_deploy_tool_definitions();
                Ok(json!({ "tools": tools }))
            },
            "tools/call" => {
                let tool_name = request
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                handle_deploy_tool_call(Some(&ctx), tool_name, &arguments).await
            }
            _ => Err(format!("Method not found: {}", request.method)),
        };

        let resp = match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(value),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                }),
            },
        };

        writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

/// Run the Store MCP stdio server (separate from Dataverse and Deploy).
/// Exposes tools for browsing, publishing, and updating store applications.
pub async fn run_store_mcp_server(ctx: StoreContext) -> Result<()> {
    info!("Starting MCP Store server");

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
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
                continue;
            }
        };

        let id = request.id.clone().unwrap_or(Value::Null);

        let result = match request.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "hr-store",
                    "version": "0.1.0"
                },
                "instructions": "Store tools for browsing, publishing, and updating HomeRoute applications."
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                let tools = get_store_tool_definitions();
                Ok(json!({ "tools": tools }))
            },
            "tools/call" => {
                let tool_name = request
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                handle_store_tool_call(Some(&ctx), tool_name, &arguments).await
            }
            _ => Err(format!("Method not found: {}", request.method)),
        };

        let resp = match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(value),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                }),
            },
        };

        writeln!(&stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

fn get_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_tables",
            "description": "List all tables in the Dataverse database with their column counts and row counts.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "describe_table",
            "description": "Get the full schema of a table including all columns and their types.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string", "description": "Name of the table to describe" }
                },
                "required": ["table_name"]
            }
        }),
        json!({
            "name": "create_table",
            "description": "Create a new table with the specified columns. Each table automatically gets id, created_at, and updated_at columns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Table name (alphanumeric + underscore)" },
                    "slug": { "type": "string", "description": "URL-friendly slug for the table" },
                    "description": { "type": "string", "description": "Optional table description" },
                    "columns": {
                        "type": "array",
                        "description": "Column definitions",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "field_type": { "type": "string", "enum": ["text", "number", "decimal", "boolean", "date_time", "date", "time", "email", "url", "phone", "currency", "percent", "duration", "json", "uuid", "auto_increment", "choice", "multi_choice", "lookup", "formula"] },
                                "required": { "type": "boolean", "default": false },
                                "unique": { "type": "boolean", "default": false },
                                "default_value": { "type": "string" },
                                "description": { "type": "string" },
                                "choices": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["name", "field_type"]
                        }
                    }
                },
                "required": ["name", "slug", "columns"]
            }
        }),
        json!({
            "name": "add_column",
            "description": "Add a new column to an existing table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "name": { "type": "string" },
                    "field_type": { "type": "string" },
                    "required": { "type": "boolean", "default": false },
                    "unique": { "type": "boolean", "default": false },
                    "default_value": { "type": "string" }
                },
                "required": ["table_name", "name", "field_type"]
            }
        }),
        json!({
            "name": "remove_column",
            "description": "Remove a column from a table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "column_name": { "type": "string" }
                },
                "required": ["table_name", "column_name"]
            }
        }),
        json!({
            "name": "drop_table",
            "description": "Drop (delete) a table and all its data. This action is irreversible.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "confirm": { "type": "boolean", "description": "Must be true to confirm deletion" }
                },
                "required": ["table_name", "confirm"]
            }
        }),
        json!({
            "name": "query_data",
            "description": "Query rows from a table with optional filters and pagination.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "filters": { "type": "array", "items": { "type": "object", "properties": { "column": {"type":"string"}, "op": {"type":"string","enum":["eq","ne","gt","lt","gte","lte","like","in","is_null","is_not_null"]}, "value": {} } } },
                    "limit": { "type": "integer", "default": 100 },
                    "offset": { "type": "integer", "default": 0 },
                    "order_by": { "type": "string" },
                    "order_desc": { "type": "boolean", "default": false }
                },
                "required": ["table_name"]
            }
        }),
        json!({
            "name": "insert_data",
            "description": "Insert one or more rows into a table.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "rows": { "type": "array", "items": { "type": "object" }, "description": "Array of row objects (key=column, value=data)" }
                },
                "required": ["table_name", "rows"]
            }
        }),
        json!({
            "name": "update_data",
            "description": "Update rows in a table matching the given filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "updates": { "type": "object", "description": "Column-value pairs to update" },
                    "filters": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["table_name", "updates", "filters"]
            }
        }),
        json!({
            "name": "delete_data",
            "description": "Delete rows from a table matching the given filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "filters": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["table_name", "filters"]
            }
        }),
        json!({
            "name": "get_schema",
            "description": "Get the full database schema as JSON, including all tables, columns, and relations.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "get_db_info",
            "description": "Get database statistics: file size, table count, total row count.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "create_relation",
            "description": "Create a relation between two tables.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from_table": {"type":"string"}, "from_column": {"type":"string"},
                    "to_table": {"type":"string"}, "to_column": {"type":"string"},
                    "relation_type": {"type":"string","enum":["one_to_many","many_to_many","self_referential"]},
                    "on_delete": {"type":"string","enum":["cascade","set_null","restrict"],"default":"restrict"},
                    "on_update": {"type":"string","enum":["cascade","set_null","restrict"],"default":"cascade"}
                },
                "required": ["from_table","from_column","to_table","to_column","relation_type"]
            }
        }),
        json!({
            "name": "count_rows",
            "description": "Count rows in a table, optionally with filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table_name": { "type": "string" },
                    "filters": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["table_name"]
            }
        }),
        json!({
            "name": "list_other_apps_schemas",
            "description": "List the database schemas (tables, columns, relations) of all other applications in the HomeRoute network. Useful for understanding what data other apps have and how to integrate with them.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

fn handle_tool_call(engine: &DataverseEngine, tool: &str, args: &Value) -> Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    match tool {
        "list_tables" => {
            let schema = engine.get_schema().map_err(|e| e.to_string())?;
            let mut tables_info = Vec::new();
            for t in &schema.tables {
                let rows = engine.count_rows(&t.name).unwrap_or(0);
                tables_info.push(json!({
                    "name": t.name,
                    "slug": t.slug,
                    "columns": t.columns.len(),
                    "rows": rows,
                    "description": t.description,
                }));
            }
            Ok(text_result(
                serde_json::to_string_pretty(&tables_info).unwrap(),
            ))
        }

        "describe_table" => {
            let name = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let table = engine
                .get_table(name)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Table '{}' not found", name))?;
            Ok(text_result(serde_json::to_string_pretty(&table).unwrap()))
        }

        "create_table" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("name required")?
                .to_string();
            let slug = args
                .get("slug")
                .and_then(|v| v.as_str())
                .ok_or("slug required")?
                .to_string();
            let desc = args
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from);
            let cols_val = args.get("columns").ok_or("columns required")?;
            let columns: Vec<ColumnDefinition> = serde_json::from_value(cols_val.clone())
                .map_err(|e| format!("Invalid columns: {}", e))?;

            let now = chrono::Utc::now();
            let table = TableDefinition {
                name: name.clone(),
                slug,
                columns,
                description: desc,
                created_at: now,
                updated_at: now,
            };
            let version = engine.create_table(&table).map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "Table '{}' created (schema version {})",
                name, version
            )))
        }

        "add_column" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("name required")?
                .to_string();
            let ft_str = args
                .get("field_type")
                .and_then(|v| v.as_str())
                .ok_or("field_type required")?;
            let field_type: FieldType = serde_json::from_str(&format!("\"{}\"", ft_str))
                .map_err(|_| format!("Invalid field_type: {}", ft_str))?;
            let required = args
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let unique = args
                .get("unique")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let default_value = args
                .get("default_value")
                .and_then(|v| v.as_str())
                .map(String::from);

            let col = ColumnDefinition {
                name: name.clone(),
                field_type,
                required,
                unique,
                default_value,
                description: None,
                choices: vec![],
            };
            let version = engine.add_column(table, &col).map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "Column '{}' added to '{}' (schema version {})",
                name, table, version
            )))
        }

        "remove_column" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let col = args
                .get("column_name")
                .and_then(|v| v.as_str())
                .ok_or("column_name required")?;
            let version = engine
                .remove_column(table, col)
                .map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "Column '{}' removed from '{}' (schema version {})",
                col, table, version
            )))
        }

        "drop_table" => {
            let name = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let confirm = args
                .get("confirm")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !confirm {
                return Err("Set confirm=true to confirm table deletion".to_string());
            }
            let version = engine.drop_table(name).map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "Table '{}' dropped (schema version {})",
                name, version
            )))
        }

        "query_data" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let filters: Vec<Filter> = args
                .get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let pagination = Pagination {
                limit: args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100),
                offset: args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0),
                order_by: args
                    .get("order_by")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                order_desc: args
                    .get("order_desc")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };
            let rows = query_rows(engine.connection(), table, &filters, &pagination)
                .map_err(|e| e.to_string())?;
            Ok(text_result(serde_json::to_string_pretty(&rows).unwrap()))
        }

        "insert_data" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let rows: Vec<Value> = args
                .get("rows")
                .and_then(|v| v.as_array())
                .cloned()
                .ok_or("rows required (array)")?;
            let count =
                insert_rows(engine.connection(), table, &rows).map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "{} row(s) inserted into '{}'",
                count, table
            )))
        }

        "update_data" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let updates = args.get("updates").ok_or("updates required")?;
            let filters: Vec<Filter> = args
                .get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let count = update_rows(engine.connection(), table, updates, &filters)
                .map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "{} row(s) updated in '{}'",
                count, table
            )))
        }

        "delete_data" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let filters: Vec<Filter> = args
                .get("filters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let count = delete_rows(engine.connection(), table, &filters)
                .map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "{} row(s) deleted from '{}'",
                count, table
            )))
        }

        "get_schema" => {
            let schema = engine.get_schema().map_err(|e| e.to_string())?;
            Ok(text_result(serde_json::to_string_pretty(&schema).unwrap()))
        }

        "get_db_info" => {
            let schema = engine.get_schema().map_err(|e| e.to_string())?;
            let mut total_rows: u64 = 0;
            for t in &schema.tables {
                total_rows += engine.count_rows(&t.name).unwrap_or(0);
            }
            let info = json!({
                "tables": schema.tables.len(),
                "relations": schema.relations.len(),
                "total_rows": total_rows,
                "schema_version": schema.version,
            });
            Ok(text_result(serde_json::to_string_pretty(&info).unwrap()))
        }

        "create_relation" => {
            let rel = RelationDefinition {
                from_table: args
                    .get("from_table")
                    .and_then(|v| v.as_str())
                    .ok_or("from_table required")?
                    .to_string(),
                from_column: args
                    .get("from_column")
                    .and_then(|v| v.as_str())
                    .ok_or("from_column required")?
                    .to_string(),
                to_table: args
                    .get("to_table")
                    .and_then(|v| v.as_str())
                    .ok_or("to_table required")?
                    .to_string(),
                to_column: args
                    .get("to_column")
                    .and_then(|v| v.as_str())
                    .ok_or("to_column required")?
                    .to_string(),
                relation_type: serde_json::from_str(&format!(
                    "\"{}\"",
                    args.get("relation_type")
                        .and_then(|v| v.as_str())
                        .ok_or("relation_type required")?
                ))
                .map_err(|e| format!("Invalid relation_type: {}", e))?,
                cascade: CascadeRules {
                    on_delete: args
                        .get("on_delete")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or_default(),
                    on_update: args
                        .get("on_update")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or_default(),
                },
            };
            let version = engine
                .create_relation(&rel)
                .map_err(|e| e.to_string())?;
            Ok(text_result(format!(
                "Relation created: {}.{} -> {}.{} (schema version {})",
                rel.from_table, rel.from_column, rel.to_table, rel.to_column, version
            )))
        }

        "count_rows" => {
            let table = args
                .get("table_name")
                .and_then(|v| v.as_str())
                .ok_or("table_name required")?;
            let count = engine.count_rows(table).map_err(|e| e.to_string())?;
            Ok(text_result(format!("{}", count)))
        }

        // list_other_apps_schemas is handled separately in the async path above
        _ => Err(format!("Unknown tool: {}", tool)),
    }
}

/// Handle the `list_other_apps_schemas` tool call by sending a request to the
/// registry via the WebSocket and waiting for the response.
async fn handle_list_other_apps_schemas(
    outbound_tx: Option<&mpsc::Sender<AgentMessage>>,
    schema_signals: Option<&SchemaQuerySignals>,
) -> Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    let outbound_tx = outbound_tx
        .ok_or_else(|| "Registry connection not available (running in standalone MCP mode)".to_string())?;
    let schema_signals = schema_signals
        .ok_or_else(|| "Schema signals not available".to_string())?;

    let request_id = uuid::Uuid::new_v4().to_string();

    // Register a oneshot channel to receive the response
    let (tx, rx) = oneshot::channel();
    {
        let mut signals = schema_signals.write().await;
        signals.insert(request_id.clone(), tx);
    }

    // Send the request to the registry
    outbound_tx
        .send(AgentMessage::GetDataverseSchemas {
            request_id: request_id.clone(),
        })
        .await
        .map_err(|_| "Failed to send request to registry (connection closed)".to_string())?;

    // Wait for the response with a 10s timeout
    match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
        Ok(Ok(schemas)) => {
            let json_output = serde_json::to_string_pretty(&schemas)
                .map_err(|e| format!("Failed to serialize schemas: {}", e))?;
            Ok(text_result(json_output))
        }
        Ok(Err(_)) => {
            // Oneshot sender was dropped (e.g., connection lost)
            Err("Registry connection lost while waiting for schemas".to_string())
        }
        Err(_) => {
            // Timeout — clean up the signal
            let mut signals = schema_signals.write().await;
            signals.remove(&request_id);
            Err("Timeout waiting for schemas from registry (10s)".to_string())
        }
    }
}

// ── Deploy tools (Development environment only) ──────────────

fn get_deploy_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "deploy",
            "description": "Deploy a compiled Rust binary to the linked production container. Copies the binary to /opt/app/app on prod, creates the app.service systemd unit if needed, and (re)starts the service. This tool does NOT build — run `cargo build --release` first, then pass the binary path. The binary manages its own configuration (e.g. read from /opt/app/config.toml or environment variables). The deploy is synchronous and blocks until the service is restarted.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "binary_path": {
                        "type": "string",
                        "description": "Absolute path to the compiled Rust binary (e.g. /root/workspace/target/release/my-app)"
                    }
                },
                "required": ["binary_path"]
            }
        }),
        json!({
            "name": "prod_status",
            "description": "Check the status of the linked production container's app.service and deployed binary. Returns whether the service is active, its uptime, and metadata about the binary at /opt/app/app (size, modification date).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "prod_logs",
            "description": "Get recent logs from the linked production container's app.service (journalctl output).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lines": {
                        "type": "integer",
                        "description": "Number of log lines to retrieve (default: 50)",
                        "default": 50
                    }
                }
            }
        }),
        json!({
            "name": "prod_exec",
            "description": "Execute a shell command on the linked production container. Useful for creating directories, checking files, installing packages, inspecting the prod environment, etc. The command runs as root inside the prod container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (e.g. 'ls -la /opt/app/', 'mkdir -p /opt/app/data')"
                    }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "prod_push",
            "description": "Copy a local file or directory to the linked production container. For directories, the contents are archived and extracted at the destination. Use this to push config files (.env), static assets, or any other files needed on prod. WARNING: Pushing .dataverse/ to prod will OVERWRITE the entire production database including all data. Use `migrate_schema` instead for safe schema-only migrations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "local_path": {
                        "type": "string",
                        "description": "Absolute path to the local file or directory to copy (e.g. /root/workspace/.env, /root/workspace/frontend/dist)"
                    },
                    "remote_path": {
                        "type": "string",
                        "description": "Absolute destination path on the prod container (e.g. /opt/app/.env, /opt/app/frontend/dist). IMPORTANT: the backend reads static assets from /opt/app/frontend/dist — always push frontend builds there, NOT /opt/app/dist."
                    }
                },
                "required": ["local_path", "remote_path"]
            }
        }),
        json!({
            "name": "prod_schema",
            "description": "Get the full SQLite schema of the PROD database, including CREATE TABLE statements and a list of user tables.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "schema_diff",
            "description": "Compare the DEV database schema against the PROD database schema. Shows new tables, new columns, removed columns, and type changes.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "migrate_schema",
            "description": "Apply schema migrations from DEV to PROD. Generates CREATE TABLE / ALTER TABLE ADD COLUMN statements based on the schema diff. Use dry_run=true to preview without applying.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dry_run": {
                        "type": "boolean",
                        "description": "If true, only show the SQL that would be executed without applying it (default: true)",
                        "default": true
                    }
                }
            }
        }),
        json!({
            "name": "deploy_app",
            "description": "Full deployment pipeline: build the Rust binary, optionally build frontend, migrate schema, push frontend assets, deploy the binary, and run a health check.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skip_frontend": {
                        "type": "boolean",
                        "description": "Skip the frontend build step (default: false)",
                        "default": false
                    },
                    "skip_schema": {
                        "type": "boolean",
                        "description": "Skip the schema migration step (default: false)",
                        "default": false
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Preview what would happen without executing (default: false)",
                        "default": false
                    }
                }
            }
        }),
        json!({
            "name": "dev_health_check",
            "description": "Check the status of all DEV services (code-server, vite-dev, cargo-dev) and their ports (13337, 5173, 3000).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "dev_test_endpoint",
            "description": "Test an HTTP endpoint locally from the DEV container. Returns status code, headers, and truncated body.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to test (e.g. http://localhost:3000/api/health)"
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method (default: GET)",
                        "enum": ["GET", "POST", "PUT", "DELETE"],
                        "default": "GET"
                    },
                    "body": {
                        "type": "string",
                        "description": "Request body (for POST/PUT)"
                    },
                    "expected_status": {
                        "type": "integer",
                        "description": "Expected HTTP status code. If provided, result includes PASS/FAIL."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "dev_test_browser",
            "description": "Capture a screenshot of a web page using headless Chromium. Installs Chromium on first use if not present. Returns the screenshot as a base64-encoded PNG image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to capture (e.g. http://localhost:3000)"
                    },
                    "width": {
                        "type": "integer",
                        "default": 1280,
                        "description": "Viewport width in pixels (default: 1280)"
                    },
                    "height": {
                        "type": "integer",
                        "default": 720,
                        "description": "Viewport height in pixels (default: 720)"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "default": 2000,
                        "description": "Wait time in ms after page load before capture (default: 2000)"
                    }
                },
                "required": ["url"]
            }
        }),
    ]
}

// ── Store tools (all environments) ──────────────

fn get_store_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_store_apps",
            "description": "List all applications available in the HomeRoute Store. Returns app names, slugs, categories, and latest version info.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "get_app_info",
            "description": "Get detailed information about a specific app in the HomeRoute Store, including all available versions and changelogs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "App slug identifier" }
                },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "check_updates",
            "description": "Check for available updates for installed apps. Pass a list of currently installed app versions to see which have newer releases available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "installed": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "slug": { "type": "string" },
                                "version": { "type": "string" }
                            },
                            "required": ["slug", "version"]
                        }
                    }
                },
                "required": ["installed"]
            }
        }),
        json!({
            "name": "publish_release",
            "description": "Publish a new release (APK) to the HomeRoute Store. The APK file must be built first (e.g. via `eas build` or local Gradle build). Pass the path to the APK file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "apk_path": { "type": "string", "description": "Absolute path to the APK file" },
                    "slug": { "type": "string", "description": "App slug for the store" },
                    "version": { "type": "string", "description": "Version string (e.g. 1.0.0)" },
                    "name": { "type": "string", "description": "App display name (required for first publish)" },
                    "description": { "type": "string" },
                    "changelog": { "type": "string" },
                    "category": { "type": "string" }
                },
                "required": ["apk_path", "slug", "version"]
            }
        }),
    ]
}

async fn handle_store_tool_call(
    store_ctx: Option<&StoreContext>,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    let ctx = store_ctx
        .ok_or_else(|| "Store tools not available (not connected)".to_string())?;

    match tool {
        "list_store_apps" => {
            let url = format!("{}/api/store/apps", ctx.api_base_url);
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await
                .map_err(|e| format!("Failed to query store apps: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
        }

        "get_app_info" => {
            let slug = args
                .get("slug")
                .and_then(|v| v.as_str())
                .ok_or("slug required")?;

            let url = format!("{}/api/store/apps/{}", ctx.api_base_url, slug);
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await
                .map_err(|e| format!("Failed to query app info: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
        }

        "check_updates" => {
            let installed = args
                .get("installed")
                .and_then(|v| v.as_array())
                .ok_or("installed required (array)")?;

            let installed_param: Vec<String> = installed.iter().filter_map(|item| {
                let slug = item.get("slug").and_then(|v| v.as_str())?;
                let version = item.get("version").and_then(|v| v.as_str())?;
                Some(format!("{}:{}", slug, version))
            }).collect();

            let url = format!(
                "{}/api/store/updates?installed={}",
                ctx.api_base_url,
                installed_param.join(",")
            );
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await
                .map_err(|e| format!("Failed to check updates: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
        }

        "publish_release" => {
            let apk_path = args
                .get("apk_path")
                .and_then(|v| v.as_str())
                .ok_or("apk_path required")?;
            let slug = args
                .get("slug")
                .and_then(|v| v.as_str())
                .ok_or("slug required")?;
            let version = args
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or("version required")?;

            // Validate APK exists
            let metadata = tokio::fs::metadata(apk_path)
                .await
                .map_err(|e| format!("Cannot access APK at '{}': {}", apk_path, e))?;
            let apk_size = metadata.len();
            if apk_size == 0 {
                return Err("APK file is empty".to_string());
            }

            info!("Publishing APK: {} ({} bytes) as {}@{}", apk_path, apk_size, slug, version);

            // Read the APK
            let apk_data = tokio::fs::read(apk_path)
                .await
                .map_err(|e| format!("Failed to read APK: {e}"))?;

            let url = format!("{}/api/store/apps/{}/releases", ctx.api_base_url, slug);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

            let mut req = client
                .post(&url)
                .header("Content-Type", "application/octet-stream")
                .header("X-Version", version)
                .header("X-Publisher-App-Id", &ctx.app_id);

            if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                req = req.header("X-App-Name", name);
            }
            if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
                req = req.header("X-App-Description", description);
            }
            if let Some(changelog) = args.get("changelog").and_then(|v| v.as_str()) {
                req = req.header("X-Changelog", changelog);
            }
            if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
                req = req.header("X-Category", category);
            }

            let resp = req
                .body(apk_data)
                .send()
                .await
                .map_err(|e| format!("Failed to send publish request: {e}"))?;

            let status = resp.status();
            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse publish response: {e}"))?;

            if status.is_success() && body.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("Release published");
                Ok(text_result(format!(
                    "Published successfully!\n\nAPK: {} ({} bytes)\nSlug: {}\nVersion: {}\n{}",
                    apk_path, apk_size, slug, version, message
                )))
            } else {
                let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                Ok(text_result(format!("Publish failed: {}", error)))
            }
        }

        _ => Err(format!("Unknown store tool: {}", tool)),
    }
}

/// Generate the `.mcp.json` content with all tools listed in `autoApprove`.
/// When `is_dev` is true, includes the deploy MCP server.
pub fn generate_mcp_json(is_dev: bool) -> String {
    let dataverse_tools: Vec<String> = get_tool_definitions()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    let mut servers = serde_json::Map::new();
    servers.insert(
        "dataverse".to_string(),
        json!({
            "command": "/usr/local/bin/hr-agent",
            "args": ["mcp"],
            "autoApprove": dataverse_tools
        }),
    );

    if is_dev {
        let deploy_tools: Vec<String> = get_deploy_tool_definitions()
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
            .collect();
        servers.insert(
            "deploy".to_string(),
            json!({
                "command": "/usr/local/bin/hr-agent",
                "args": ["mcp-deploy"],
                "autoApprove": deploy_tools
            }),
        );
    }

    let store_tools: Vec<String> = get_store_tool_definitions()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    servers.insert(
        "store".to_string(),
        json!({
            "command": "/usr/local/bin/hr-agent",
            "args": ["mcp-store"],
            "autoApprove": store_tools
        }),
    );

    serde_json::to_string_pretty(&json!({ "mcpServers": servers })).unwrap()
}

/// Helper to execute a shell command on the linked production container via the API.
async fn exec_on_prod(ctx: &DeployContext, command: &str) -> Result<String, String> {
    let url = format!(
        "{}/api/applications/{}/prod/exec",
        ctx.api_base_url, ctx.app_id
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let resp = client
        .post(&url)
        .json(&json!({"command": command}))
        .send()
        .await
        .map_err(|e| format!("Failed to send exec request: {e}"))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;
    let success = body
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let stdout = body
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let stderr = body
        .get("stderr")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !success {
        return Err(format!(
            "Command failed:\nSTDOUT: {stdout}\nSTDERR: {stderr}"
        ));
    }
    Ok(if stdout.is_empty() { stderr } else { stdout })
}

/// Parse CREATE TABLE statements from raw SQLite .schema output.
/// Returns a map of table_name -> Vec<(column_name, column_type)>.
fn parse_schema_tables(schema_text: &str) -> HashMap<String, Vec<(String, String)>> {
    let mut tables: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let text = schema_text;

    // Simple approach: find "CREATE TABLE" then extract table name and columns
    for segment in text.split("CREATE TABLE") {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // Skip internal/system tables
        if segment.starts_with("IF NOT EXISTS") {
            // "IF NOT EXISTS tablename (...)"
            let rest = segment.trim_start_matches("IF NOT EXISTS").trim();
            if rest.starts_with("_dv_") || rest.starts_with("sqlite_") {
                continue;
            }
        }

        // Extract table name: everything before the first '('
        let paren_pos = match segment.find('(') {
            Some(p) => p,
            None => continue,
        };
        let table_name = segment[..paren_pos]
            .trim()
            .trim_matches('"')
            .trim_matches('`')
            .trim_start_matches("IF NOT EXISTS ")
            .trim()
            .trim_matches('"')
            .trim_matches('`')
            .to_string();

        if table_name.starts_with("_dv_") || table_name.starts_with("sqlite_") {
            continue;
        }

        // Find matching closing paren
        let body_start = paren_pos + 1;
        let mut depth = 1;
        let mut body_end = body_start;
        let segment_bytes = segment.as_bytes();
        for j in body_start..segment.len() {
            if segment_bytes[j] == b'(' {
                depth += 1;
            } else if segment_bytes[j] == b')' {
                depth -= 1;
                if depth == 0 {
                    body_end = j;
                    break;
                }
            }
        }

        let body = &segment[body_start..body_end];
        let mut columns = Vec::new();

        for col_def in body.split(',') {
            let col_def = col_def.trim();
            if col_def.is_empty() {
                continue;
            }
            // Skip constraints like PRIMARY KEY, UNIQUE, FOREIGN KEY, CHECK
            let upper = col_def.to_uppercase();
            if upper.starts_with("PRIMARY KEY")
                || upper.starts_with("UNIQUE")
                || upper.starts_with("FOREIGN KEY")
                || upper.starts_with("CHECK")
                || upper.starts_with("CONSTRAINT")
            {
                continue;
            }

            // Column definition: "name TYPE ..."
            let parts: Vec<&str> = col_def.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let col_name = parts[0].trim_matches('"').trim_matches('`').to_string();
            let col_type = if parts.len() > 1 {
                parts[1].trim_matches('"').trim_matches('`').to_uppercase()
            } else {
                "TEXT".to_string()
            };

            columns.push((col_name, col_type));
        }

        if !table_name.is_empty() {
            tables.insert(table_name, columns);
        }
    }

    tables
}

/// Compute schema diff between dev and prod tables.
struct SchemaDiff {
    new_tables: Vec<(String, Vec<(String, String)>)>,
    new_columns: Vec<(String, String, String)>,   // (table, col_name, col_type)
    removed_columns: Vec<(String, String)>,         // (table, col_name)
    type_changes: Vec<(String, String, String, String)>, // (table, col, old_type, new_type)
}

fn compute_schema_diff(
    dev_tables: &HashMap<String, Vec<(String, String)>>,
    prod_tables: &HashMap<String, Vec<(String, String)>>,
) -> SchemaDiff {
    let mut new_tables = Vec::new();
    let mut new_columns = Vec::new();
    let mut removed_columns = Vec::new();
    let mut type_changes = Vec::new();

    for (table_name, dev_cols) in dev_tables {
        match prod_tables.get(table_name) {
            None => {
                // Entire table is new
                new_tables.push((table_name.clone(), dev_cols.clone()));
            }
            Some(prod_cols) => {
                let prod_map: HashMap<&str, &str> = prod_cols
                    .iter()
                    .map(|(n, t)| (n.as_str(), t.as_str()))
                    .collect();
                let dev_map: HashMap<&str, &str> = dev_cols
                    .iter()
                    .map(|(n, t)| (n.as_str(), t.as_str()))
                    .collect();

                // New columns in dev
                for (col_name, col_type) in dev_cols {
                    match prod_map.get(col_name.as_str()) {
                        None => {
                            new_columns.push((
                                table_name.clone(),
                                col_name.clone(),
                                col_type.clone(),
                            ));
                        }
                        Some(&prod_type) => {
                            if prod_type != col_type.as_str() {
                                type_changes.push((
                                    table_name.clone(),
                                    col_name.clone(),
                                    prod_type.to_string(),
                                    col_type.clone(),
                                ));
                            }
                        }
                    }
                }

                // Removed columns (in prod but not in dev)
                for (col_name, _) in prod_cols {
                    if !dev_map.contains_key(col_name.as_str()) {
                        removed_columns.push((table_name.clone(), col_name.clone()));
                    }
                }
            }
        }
    }

    SchemaDiff {
        new_tables,
        new_columns,
        removed_columns,
        type_changes,
    }
}

fn format_schema_diff(diff: &SchemaDiff) -> String {
    let mut output = String::new();

    if diff.new_tables.is_empty()
        && diff.new_columns.is_empty()
        && diff.removed_columns.is_empty()
        && diff.type_changes.is_empty()
    {
        return "Schemas are identical. No differences found.".to_string();
    }

    if !diff.new_tables.is_empty() {
        output.push_str("## New Tables\n\n");
        for (name, cols) in &diff.new_tables {
            output.push_str(&format!("- **{}** ({} columns)\n", name, cols.len()));
            for (col_name, col_type) in cols {
                output.push_str(&format!("  - {} {}\n", col_name, col_type));
            }
        }
        output.push('\n');
    }

    if !diff.new_columns.is_empty() {
        output.push_str("## New Columns\n\n");
        for (table, col, typ) in &diff.new_columns {
            output.push_str(&format!("- **{}**.{} ({})\n", table, col, typ));
        }
        output.push('\n');
    }

    if !diff.removed_columns.is_empty() {
        output.push_str("## Removed Columns (in PROD but not in DEV)\n\n");
        for (table, col) in &diff.removed_columns {
            output.push_str(&format!("- **{}**.{}\n", table, col));
        }
        output.push_str("\n> Note: SQLite does not support DROP COLUMN in older versions. These columns will be left in place.\n\n");
    }

    if !diff.type_changes.is_empty() {
        output.push_str("## Type Changes\n\n");
        for (table, col, old_type, new_type) in &diff.type_changes {
            output.push_str(&format!(
                "- **{}**.{}: {} → {}\n",
                table, col, old_type, new_type
            ));
        }
        output.push_str("\n> Note: SQLite does not support ALTER COLUMN. Type changes require manual migration.\n\n");
    }

    output
}

fn generate_migration_sql(diff: &SchemaDiff) -> Vec<String> {
    let mut statements = Vec::new();

    for (table_name, columns) in &diff.new_tables {
        let cols_sql: Vec<String> = columns
            .iter()
            .map(|(name, typ)| format!("\"{}\" {}", name, typ))
            .collect();
        statements.push(format!(
            "CREATE TABLE IF NOT EXISTS \"{}\" ({});",
            table_name,
            cols_sql.join(", ")
        ));
    }

    for (table, col_name, col_type) in &diff.new_columns {
        statements.push(format!(
            "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {};",
            table, col_name, col_type
        ));
    }

    statements
}

async fn handle_deploy_tool_call(
    deploy_ctx: Option<&DeployContext>,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    let ctx = deploy_ctx
        .ok_or_else(|| "Deploy tools not available (not a development environment or not connected)".to_string())?;

    if ctx.environment != Environment::Development {
        return Err("Deploy tools are only available in development environments".to_string());
    }

    match tool {
        "deploy" => {
            let binary_path = args
                .get("binary_path")
                .and_then(|v| v.as_str())
                .ok_or("binary_path required")?;

            // Validate binary exists
            let metadata = tokio::fs::metadata(binary_path)
                .await
                .map_err(|e| format!("Cannot access binary at '{}': {}", binary_path, e))?;
            let binary_size = metadata.len();
            if binary_size == 0 {
                return Err("Binary file is empty".to_string());
            }

            info!("Deploying binary: {} ({} bytes)", binary_path, binary_size);

            // Read the binary
            let binary_data = tokio::fs::read(binary_path)
                .await
                .map_err(|e| format!("Failed to read binary: {e}"))?;

            // POST to deploy endpoint as raw binary
            let url = format!("{}/api/applications/{}/deploy", ctx.api_base_url, ctx.app_id);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

            let resp = client
                .post(&url)
                .header("Content-Type", "application/octet-stream")
                .body(binary_data)
                .send()
                .await
                .map_err(|e| format!("Failed to send deploy request: {e}"))?;

            let status = resp.status();
            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse deploy response: {e}"))?;

            if status.is_success() && body.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("Deploy completed");
                Ok(text_result(format!(
                    "Deploy successful!\n\nBinary: {} ({} bytes)\n{}",
                    binary_path, binary_size, message
                )))
            } else {
                let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                Ok(text_result(format!("Deploy failed: {}", error)))
            }
        }

        "prod_status" => {
            let url = format!("{}/api/applications/{}/prod/status", ctx.api_base_url, ctx.app_id);
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await
                .map_err(|e| format!("Failed to query prod status: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
        }

        "prod_logs" => {
            let lines = args
                .get("lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(50);

            let url = format!(
                "{}/api/applications/{}/prod/logs?lines={}",
                ctx.api_base_url, ctx.app_id, lines
            );
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await
                .map_err(|e| format!("Failed to query prod logs: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            if let Some(logs) = body.get("logs").and_then(|v| v.as_str()) {
                Ok(text_result(logs.to_string()))
            } else {
                Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
            }
        }

        "prod_exec" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or("command required")?;

            let url = format!("{}/api/applications/{}/prod/exec", ctx.api_base_url, ctx.app_id);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

            let resp = client.post(&url)
                .json(&json!({"command": command}))
                .send()
                .await
                .map_err(|e| format!("Failed to send exec request: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            if let Some(stdout) = body.get("stdout").and_then(|v| v.as_str()) {
                let stderr = body.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                let success = body.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let mut output = String::new();
                if !success {
                    output.push_str("Command failed!\n\n");
                }
                if !stdout.is_empty() {
                    output.push_str(&format!("STDOUT:\n{}\n", stdout));
                }
                if !stderr.is_empty() {
                    output.push_str(&format!("STDERR:\n{}\n", stderr));
                }
                if output.is_empty() {
                    output = "Command completed (no output)".to_string();
                }
                Ok(text_result(output))
            } else {
                Ok(text_result(serde_json::to_string_pretty(&body).unwrap()))
            }
        }

        "prod_schema" => {
            let schema_output = exec_on_prod(ctx, "sqlite3 /opt/app/.dataverse/app.db '.schema'").await?;
            let tables_output = exec_on_prod(
                ctx,
                "sqlite3 /opt/app/.dataverse/app.db \"SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_dv_%'\"",
            ).await.unwrap_or_else(|_| "(no tables)".to_string());

            let output = format!(
                "## PROD Database Schema\n\n### Tables\n```\n{}\n```\n\n### Full Schema\n```sql\n{}\n```",
                tables_output.trim(),
                schema_output.trim()
            );
            Ok(text_result(output))
        }

        "schema_diff" => {
            // Get local DEV schema
            let dev_output = tokio::process::Command::new("sqlite3")
                .arg("/root/workspace/.dataverse/app.db")
                .arg(".schema")
                .output()
                .await
                .map_err(|e| format!("Failed to read DEV schema: {e}"))?;
            if !dev_output.status.success() {
                return Err(format!(
                    "Failed to read DEV schema: {}",
                    String::from_utf8_lossy(&dev_output.stderr)
                ));
            }
            let dev_schema = String::from_utf8_lossy(&dev_output.stdout).to_string();

            // Get PROD schema
            let prod_schema =
                exec_on_prod(ctx, "sqlite3 /opt/app/.dataverse/app.db '.schema'").await?;

            let dev_tables = parse_schema_tables(&dev_schema);
            let prod_tables = parse_schema_tables(&prod_schema);

            let diff = compute_schema_diff(&dev_tables, &prod_tables);
            let output = format!(
                "# Schema Diff: DEV vs PROD\n\nDEV tables: {}\nPROD tables: {}\n\n{}",
                dev_tables.len(),
                prod_tables.len(),
                format_schema_diff(&diff)
            );
            Ok(text_result(output))
        }

        "migrate_schema" => {
            let dry_run = args
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            // Get local DEV schema
            let dev_output = tokio::process::Command::new("sqlite3")
                .arg("/root/workspace/.dataverse/app.db")
                .arg(".schema")
                .output()
                .await
                .map_err(|e| format!("Failed to read DEV schema: {e}"))?;
            if !dev_output.status.success() {
                return Err(format!(
                    "Failed to read DEV schema: {}",
                    String::from_utf8_lossy(&dev_output.stderr)
                ));
            }
            let dev_schema = String::from_utf8_lossy(&dev_output.stdout).to_string();

            // Get PROD schema
            let prod_schema =
                exec_on_prod(ctx, "sqlite3 /opt/app/.dataverse/app.db '.schema'").await?;

            let dev_tables = parse_schema_tables(&dev_schema);
            let prod_tables = parse_schema_tables(&prod_schema);

            let diff = compute_schema_diff(&dev_tables, &prod_tables);
            let statements = generate_migration_sql(&diff);

            if statements.is_empty() {
                return Ok(text_result(
                    "No migrations needed. DEV and PROD schemas are compatible.".to_string(),
                ));
            }

            if dry_run {
                let sql_preview = statements.join("\n");
                return Ok(text_result(format!(
                    "## Dry Run — {} statement(s) to execute:\n\n```sql\n{}\n```\n\nRun with dry_run=false to apply.",
                    statements.len(),
                    sql_preview
                )));
            }

            // Execute each statement on prod
            let mut results = Vec::new();
            for stmt in &statements {
                let cmd = format!(
                    "sqlite3 /opt/app/.dataverse/app.db \"{}\"",
                    stmt.replace('"', "\\\"")
                );
                match exec_on_prod(ctx, &cmd).await {
                    Ok(_) => results.push(format!("OK: {}", stmt)),
                    Err(e) => results.push(format!("FAIL: {}\n  Error: {}", stmt, e)),
                }
            }

            let report = format!(
                "## Migration Report\n\n{} statement(s) executed:\n\n{}",
                statements.len(),
                results.join("\n")
            );
            Ok(text_result(report))
        }

        "deploy_app" => {
            let skip_frontend = args
                .get("skip_frontend")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let skip_schema = args
                .get("skip_schema")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let dry_run = args
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut report = Vec::new();
            if dry_run {
                report.push("# DRY RUN MODE — no changes will be made\n".to_string());
            }

            // Step 1: cargo build --release
            report.push("## Step 1: Building Rust binary...".to_string());
            // Ensure cargo is in PATH (rustup installs to ~/.cargo/bin, not in systemd PATH)
            let path_env = format!(
                "/root/.cargo/bin:{}",
                std::env::var("PATH").unwrap_or_default()
            );
            if dry_run {
                report.push("DRY RUN: Would run `cargo build --release` in /root/workspace".to_string());
            } else {
                let build_output = tokio::process::Command::new("cargo")
                    .args(["build", "--release"])
                    .current_dir("/root/workspace")
                    .env("PATH", &path_env)
                    .output()
                    .await
                    .map_err(|e| format!("Failed to run cargo build: {e}"))?;
                if !build_output.status.success() {
                    let stderr = String::from_utf8_lossy(&build_output.stderr);
                    report.push(format!("FAILED: cargo build --release\n{}", stderr));
                    return Ok(text_result(report.join("\n\n")));
                }
                report.push("OK: cargo build --release".to_string());
            }

            // Step 2: Frontend build (if frontend/ exists and not skipped)
            let frontend_dir = std::path::Path::new("/root/workspace/frontend");
            if frontend_dir.exists() && !skip_frontend {
                report.push("## Step 2: Building frontend...".to_string());
                if dry_run {
                    report.push("DRY RUN: Would run `npm run build` in /root/workspace/frontend".to_string());
                } else {
                    let npm_output = tokio::process::Command::new("npm")
                        .args(["run", "build"])
                        .current_dir("/root/workspace/frontend")
                        .output()
                        .await
                        .map_err(|e| format!("Failed to run npm build: {e}"))?;
                    if !npm_output.status.success() {
                        let stderr = String::from_utf8_lossy(&npm_output.stderr);
                        report.push(format!("FAILED: npm run build\n{}", stderr));
                        return Ok(text_result(report.join("\n\n")));
                    }
                    report.push("OK: npm run build".to_string());
                }
            } else {
                report.push("## Step 2: Frontend build SKIPPED".to_string());
            }

            // Step 3: Schema migration (if .dataverse/ exists and not skipped)
            let dataverse_dir = std::path::Path::new("/root/workspace/.dataverse");
            if dataverse_dir.exists() && !skip_schema {
                report.push("## Step 3: Schema migration...".to_string());
                let dev_output = tokio::process::Command::new("sqlite3")
                    .arg("/root/workspace/.dataverse/app.db")
                    .arg(".schema")
                    .output()
                    .await
                    .map_err(|e| format!("Failed to read DEV schema: {e}"))?;
                if dev_output.status.success() {
                    let dev_schema = String::from_utf8_lossy(&dev_output.stdout).to_string();
                    match exec_on_prod(ctx, "sqlite3 /opt/app/.dataverse/app.db '.schema'").await {
                        Ok(prod_schema) => {
                            let dev_tables = parse_schema_tables(&dev_schema);
                            let prod_tables = parse_schema_tables(&prod_schema);
                            let diff = compute_schema_diff(&dev_tables, &prod_tables);
                            let statements = generate_migration_sql(&diff);
                            if statements.is_empty() {
                                report.push("OK: No schema changes needed".to_string());
                            } else if dry_run {
                                report.push(format!(
                                    "DRY RUN: Would execute {} migration statement(s):\n{}",
                                    statements.len(),
                                    statements.iter().map(|s| format!("  {}", s)).collect::<Vec<_>>().join("\n")
                                ));
                            } else {
                                let mut migration_results = Vec::new();
                                for stmt in &statements {
                                    let cmd = format!(
                                        "sqlite3 /opt/app/.dataverse/app.db \"{}\"",
                                        stmt.replace('"', "\\\"")
                                    );
                                    match exec_on_prod(ctx, &cmd).await {
                                        Ok(_) => migration_results.push(format!("  OK: {}", stmt)),
                                        Err(e) => migration_results
                                            .push(format!("  FAIL: {} ({})", stmt, e)),
                                    }
                                }
                                report.push(format!(
                                    "Migrated {} statement(s):\n{}",
                                    statements.len(),
                                    migration_results.join("\n")
                                ));
                            }
                        }
                        Err(e) => {
                            report.push(format!("WARNING: Could not read PROD schema: {}", e));
                        }
                    }
                } else {
                    report.push("WARNING: Could not read DEV schema".to_string());
                }
            } else {
                report.push("## Step 3: Schema migration SKIPPED".to_string());
            }

            // Step 4: Push frontend assets if frontend/dist exists
            let frontend_dist = std::path::Path::new("/root/workspace/frontend/dist");
            if frontend_dist.exists() {
                report.push("## Step 4: Pushing frontend assets to prod...".to_string());
                if dry_run {
                    // Count files in frontend/dist for the dry-run report
                    let count_output = tokio::process::Command::new("find")
                        .args(["/root/workspace/frontend/dist", "-type", "f"])
                        .output()
                        .await;
                    let file_count = count_output
                        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
                        .unwrap_or(0);
                    report.push(format!(
                        "DRY RUN: Would push frontend/dist ({} files) to /opt/app/frontend/dist",
                        file_count
                    ));
                } else {
                    // Create tarball
                    let tar_path = "/tmp/deploy-frontend.tar.gz";
                    let tar_output = tokio::process::Command::new("tar")
                        .args(["czf", tar_path, "-C", "/root/workspace/frontend/dist", "."])
                        .output()
                        .await
                        .map_err(|e| format!("Failed to create frontend tarball: {e}"))?;
                    if tar_output.status.success() {
                        let archive_data = tokio::fs::read(tar_path)
                            .await
                            .map_err(|e| format!("Failed to read frontend tarball: {e}"))?;
                        let _ = tokio::fs::remove_file(tar_path).await;

                        let push_url = format!(
                            "{}/api/applications/{}/prod/push",
                            ctx.api_base_url, ctx.app_id
                        );
                        let push_client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(120))
                            .build()
                            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

                        let push_resp = push_client
                            .post(&push_url)
                            .header("Content-Type", "application/octet-stream")
                            .header("X-Remote-Path", "/opt/app/frontend/dist")
                            .header("X-Is-Directory", "true")
                            .body(archive_data)
                            .send()
                            .await
                            .map_err(|e| format!("Failed to push frontend: {e}"))?;

                        let push_body: Value = push_resp
                            .json()
                            .await
                            .map_err(|e| format!("Failed to parse push response: {e}"))?;
                        if push_body
                            .get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            report.push("OK: Frontend assets pushed to prod".to_string());
                        } else {
                            let err = push_body
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            report.push(format!("WARNING: Frontend push failed: {}", err));
                        }
                    } else {
                        report.push("WARNING: Failed to create frontend tarball".to_string());
                    }
                }
            } else {
                report.push("## Step 4: Frontend push SKIPPED (no dist/)".to_string());
            }

            // Step 5: Deploy the binary
            report.push("## Step 5: Deploying binary...".to_string());
            // Find the binary in target/release/
            let mut binary_path = None;
            if let Ok(mut entries) =
                tokio::fs::read_dir("/root/workspace/target/release").await
            {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_file() {
                        if let Ok(meta) = tokio::fs::metadata(&path).await {
                            // Check if it's executable and not a .d or .so file
                            let name = entry.file_name().to_string_lossy().to_string();
                            if !name.contains('.')
                                && meta.len() > 0
                                && meta.permissions().mode() & 0o111 != 0
                            {
                                binary_path = Some((path, meta.len()));
                                break;
                            }
                        }
                    }
                }
            }

            if dry_run {
                match binary_path {
                    Some((path, size)) => {
                        report.push(format!(
                            "DRY RUN: Would deploy {} ({} bytes)",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            size
                        ));
                    }
                    None => {
                        report.push("DRY RUN: No binary found in target/release/ (would need cargo build first)".to_string());
                    }
                }
            } else {
                let binary_path = match binary_path {
                    Some((p, _)) => p,
                    None => {
                        report.push("FAILED: No executable binary found in target/release/".to_string());
                        return Ok(text_result(report.join("\n\n")));
                    }
                };

                let binary_data = tokio::fs::read(&binary_path)
                    .await
                    .map_err(|e| format!("Failed to read binary: {e}"))?;
                let binary_size = binary_data.len();

                let deploy_url = format!(
                    "{}/api/applications/{}/deploy",
                    ctx.api_base_url, ctx.app_id
                );
                let deploy_client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(120))
                    .build()
                    .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

                let deploy_resp = deploy_client
                    .post(&deploy_url)
                    .header("Content-Type", "application/octet-stream")
                    .body(binary_data)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to send deploy request: {e}"))?;

                let deploy_body: Value = deploy_resp
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse deploy response: {e}"))?;
                if deploy_body
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    report.push(format!(
                        "OK: Binary deployed ({} bytes) from {}",
                        binary_size,
                        binary_path.display()
                    ));
                } else {
                    let err = deploy_body
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    report.push(format!("FAILED: Deploy error: {}", err));
                    return Ok(text_result(report.join("\n\n")));
                }
            }

            // Step 6: Health check (3 retries, 2s backoff)
            report.push("## Step 6: Health check...".to_string());
            if dry_run {
                report.push("DRY RUN: Would run health check on PROD".to_string());
                report.push("\n## Summary\n\nDRY RUN completed — no changes were made.".to_string());
            } else {
                let mut health_ok = false;
                for attempt in 1..=3 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    match exec_on_prod(ctx, "curl -sf http://localhost:3000/api/health || curl -sf http://localhost:8080/health || echo 'HEALTH_OK'").await {
                        Ok(output) => {
                            health_ok = true;
                            report.push(format!("OK: Health check passed (attempt {}): {}", attempt, output.trim()));
                            break;
                        }
                        Err(e) => {
                            if attempt < 3 {
                                report.push(format!("Attempt {}/3 failed: {}", attempt, e));
                            } else {
                                report.push(format!("WARNING: Health check failed after 3 attempts: {}", e));
                            }
                        }
                    }
                }

                report.push(format!(
                    "\n## Summary\n\nDeployment {}.",
                    if health_ok { "completed successfully" } else { "completed with warnings" }
                ));
            }
            Ok(text_result(report.join("\n\n")))
        }

        "dev_health_check" => {
            let mut results = Vec::new();

            // Check systemd services
            let services = ["code-server.service", "vite-dev.service", "cargo-dev.service"];
            for service in &services {
                let output = tokio::process::Command::new("systemctl")
                    .args(["is-active", service])
                    .output()
                    .await;
                let status = match output {
                    Ok(o) => {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        s.to_uppercase()
                    }
                    Err(_) => "UNKNOWN".to_string(),
                };
                results.push(format!("- {} : {}", service, status));
            }

            results.push(String::new());
            results.push("Ports:".to_string());

            // Check ports
            let ports = [(13337, "code-server"), (5173, "vite-dev"), (3000, "app")];
            for (port, label) in &ports {
                let addr = format!("127.0.0.1:{}", port);
                let status =
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        tokio::net::TcpStream::connect(&addr),
                    )
                    .await
                    {
                        Ok(Ok(_)) => "OPEN",
                        _ => "CLOSED",
                    };
                results.push(format!("- :{} ({}) : {}", port, label, status));
            }

            Ok(text_result(format!(
                "## DEV Health Check\n\nServices:\n{}\n",
                results.join("\n")
            )))
        }

        "dev_test_endpoint" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("url required")?;
            let method = args
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET")
                .to_uppercase();
            let body = args.get("body").and_then(|v| v.as_str());
            let expected_status = args
                .get("expected_status")
                .and_then(|v| v.as_u64())
                .map(|v| v as u16);

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

            let mut req = match method.as_str() {
                "POST" => client.post(url),
                "PUT" => client.put(url),
                "DELETE" => client.delete(url),
                _ => client.get(url),
            };

            if let Some(body_str) = body {
                req = req.header("Content-Type", "application/json").body(body_str.to_string());
            }

            let resp = req
                .send()
                .await
                .map_err(|e| format!("Request failed: {e}"))?;

            let status_code = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            let content_length = resp
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();

            let resp_body = resp.text().await.unwrap_or_default();
            let truncated_body = if resp_body.len() > 2000 {
                format!("{}... (truncated, {} total bytes)", &resp_body[..2000], resp_body.len())
            } else {
                resp_body
            };

            let mut output = format!(
                "## {} {}\n\nStatus: {}\nContent-Type: {}\nContent-Length: {}\n\n### Body\n```\n{}\n```",
                method, url, status_code, content_type, content_length, truncated_body
            );

            if let Some(expected) = expected_status {
                let pass = status_code == expected;
                output.push_str(&format!(
                    "\n\n### Assertion: expected status {}\nResult: **{}**",
                    expected,
                    if pass { "PASS" } else { "FAIL" }
                ));
            }

            Ok(text_result(output))
        }

        "dev_test_browser" => {
            use base64::Engine;

            let url = args.get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: url")?;
            let width = args.get("width")
                .and_then(|v| v.as_u64())
                .unwrap_or(1280);
            let height = args.get("height")
                .and_then(|v| v.as_u64())
                .unwrap_or(720);
            let wait_ms = args.get("wait_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000);

            // Check if chromium is installed, install if not
            let chromium_check = tokio::process::Command::new("which")
                .arg("chromium-browser")
                .output()
                .await;

            let chromium_bin = if chromium_check.map(|o| o.status.success()).unwrap_or(false) {
                "chromium-browser".to_string()
            } else {
                // Try 'chromium' as alternative name
                let alt_check = tokio::process::Command::new("which")
                    .arg("chromium")
                    .output()
                    .await;
                if alt_check.map(|o| o.status.success()).unwrap_or(false) {
                    "chromium".to_string()
                } else {
                    // Install chromium
                    let install = tokio::process::Command::new("bash")
                        .args(["-c", "apt-get update -qq 2>/dev/null && apt-get install -y -qq chromium-browser 2>/dev/null || apt-get install -y -qq chromium 2>/dev/null"])
                        .output()
                        .await
                        .map_err(|e| format!("Failed to install chromium: {e}"))?;
                    if !install.status.success() {
                        return Ok(text_result("ERROR: Failed to install Chromium. Please install manually: apt-get install chromium-browser".to_string()));
                    }
                    // Determine which binary was installed
                    let check_again = tokio::process::Command::new("which")
                        .arg("chromium-browser")
                        .output()
                        .await;
                    if check_again.map(|o| o.status.success()).unwrap_or(false) {
                        "chromium-browser".to_string()
                    } else {
                        "chromium".to_string()
                    }
                }
            };

            let screenshot_path = "/tmp/screenshot.png";
            let window_size = format!("--window-size={},{}", width, height);

            // Run chromium headless to take screenshot
            let output = tokio::process::Command::new(&chromium_bin)
                .args([
                    "--headless",
                    "--no-sandbox",
                    "--disable-gpu",
                    "--disable-software-rasterizer",
                    &window_size,
                    &format!("--screenshot={}", screenshot_path),
                    &format!("--virtual-time-budget={}", wait_ms),
                    url,
                ])
                .output()
                .await
                .map_err(|e| format!("Failed to run chromium: {e}"))?;

            if !std::path::Path::new(screenshot_path).exists() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(text_result(format!(
                    "ERROR: Screenshot failed.\nChromium stderr: {}",
                    stderr.chars().take(1000).collect::<String>()
                )));
            }

            // Read screenshot and encode as base64
            let screenshot_data = tokio::fs::read(screenshot_path)
                .await
                .map_err(|e| format!("Failed to read screenshot: {e}"))?;
            let _ = tokio::fs::remove_file(screenshot_path).await;

            let base64_data = base64::engine::general_purpose::STANDARD.encode(&screenshot_data);

            // Return as image content per MCP protocol
            Ok(json!({
                "content": [{
                    "type": "image",
                    "data": base64_data,
                    "mimeType": "image/png"
                }, {
                    "type": "text",
                    "text": format!("Screenshot captured: {}x{} pixels, {} bytes, URL: {}", width, height, screenshot_data.len(), url)
                }]
            }))
        }

        "prod_push" => {
            let local_path = args
                .get("local_path")
                .and_then(|v| v.as_str())
                .ok_or("local_path required")?;
            let remote_path = args
                .get("remote_path")
                .and_then(|v| v.as_str())
                .ok_or("remote_path required")?;

            let metadata = tokio::fs::metadata(local_path)
                .await
                .map_err(|e| format!("Cannot access '{}': {}", local_path, e))?;

            let is_dir = metadata.is_dir();

            // Create a tarball of the file/directory
            let tar_path = "/tmp/prod-push-artifact.tar.gz";
            let tar_args = if is_dir {
                vec!["czf", tar_path, "-C", local_path, "."]
            } else {
                // For a single file, tar it from its parent dir with just the filename
                let parent = std::path::Path::new(local_path)
                    .parent()
                    .map(|p| p.to_str().unwrap_or("/"))
                    .unwrap_or("/");
                let filename = std::path::Path::new(local_path)
                    .file_name()
                    .map(|f| f.to_str().unwrap_or("file"))
                    .unwrap_or("file");
                vec!["czf", tar_path, "-C", parent, filename]
            };

            let tar_output = tokio::process::Command::new("tar")
                .args(&tar_args)
                .output()
                .await
                .map_err(|e| format!("Failed to create tarball: {e}"))?;

            if !tar_output.status.success() {
                let stderr = String::from_utf8_lossy(&tar_output.stderr);
                return Err(format!("Failed to create tarball: {stderr}"));
            }

            let archive_data = tokio::fs::read(tar_path)
                .await
                .map_err(|e| format!("Failed to read tarball: {e}"))?;
            let archive_size = archive_data.len();
            let _ = tokio::fs::remove_file(tar_path).await;

            info!("Pushing {} to prod:{} ({} bytes archive, is_dir={})",
                local_path, remote_path, archive_size, is_dir);

            let url = format!("{}/api/applications/{}/prod/push", ctx.api_base_url, ctx.app_id);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

            let resp = client.post(&url)
                .header("Content-Type", "application/octet-stream")
                .header("X-Remote-Path", remote_path)
                .header("X-Is-Directory", if is_dir { "true" } else { "false" })
                .body(archive_data)
                .send()
                .await
                .map_err(|e| format!("Failed to send push request: {e}"))?;

            let body: Value = resp.json().await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            if body.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                Ok(text_result(format!(
                    "Pushed {} → prod:{}\nArchive size: {} bytes\nType: {}",
                    local_path, remote_path, archive_size,
                    if is_dir { "directory" } else { "file" }
                )))
            } else {
                let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                Ok(text_result(format!("Push failed: {}", error)))
            }
        }

        _ => Err(format!("Unknown deploy tool: {}", tool)),
    }
}
