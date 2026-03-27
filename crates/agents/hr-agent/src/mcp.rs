//! MCP (Model Context Protocol) stdio servers for Store, Studio, and Docs operations.

use std::io::{self, BufRead, Write};
use std::os::unix::fs::PermissionsExt;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

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


/// Context for store tools (available in all environments).
#[derive(Clone)]
pub struct StoreContext {
    pub app_id: String,
    pub api_base_url: String,
}

/// Run a minimal MCP stdio server (legacy "mcp" subcommand, now a no-op).
/// Database operations are now handled by the orchestrator's homeroute MCP server.
pub async fn run_mcp_server() -> Result<()> {
    info!("Starting MCP server (legacy — no local Dataverse tools)");

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
                    "name": "hr-agent",
                    "version": "0.1.0"
                },
                "instructions": include_str!("mcp_instructions.txt")
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                Ok(json!({ "tools": [] }))
            },
            "tools/call" => {
                Err("No tools available. Database operations are now handled by the homeroute MCP server.".to_string())
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
pub fn generate_mcp_json(token: &str) -> String {
    let mut servers = serde_json::Map::new();

    // Orchestrator MCP server (database + infrastructure tools)
    let db_tools: Vec<String> = vec![
        "db_overview", "db_list_tables", "db_describe_table", "db_create_table",
        "db_add_column", "db_remove_column", "db_drop_table", "db_create_relation",
        "db_get_schema", "db_get_db_info", "db_query_data", "db_insert_data",
        "db_update_data", "db_delete_data", "db_count_rows",
    ].into_iter().map(String::from).collect();
    servers.insert(
        "homeroute".to_string(),
        json!({
            "type": "http",
            "url": "http://10.0.0.254:4001/mcp",
            "headers": { "Authorization": format!("Bearer {}", token) },
            "autoApprove": db_tools
        }),
    );

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

    let studio_tools: Vec<String> = get_studio_tool_definitions()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    servers.insert(
        "studio".to_string(),
        json!({
            "command": "/usr/local/bin/hr-agent",
            "args": ["mcp-studio"],
            "autoApprove": studio_tools
        }),
    );

    let docs_tools: Vec<String> = get_docs_tool_definitions()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    servers.insert(
        "docs".to_string(),
        json!({
            "command": "/usr/local/bin/hr-agent",
            "args": ["mcp-docs"],
            "autoApprove": docs_tools
        }),
    );

    serde_json::to_string_pretty(&json!({ "mcpServers": servers })).unwrap()
}

/// Generate `settings.json` with all MCP tools listed in `permissions.allow`.
/// This ensures MCP tools are auto-authorized in plan mode where they would otherwise be denied.
pub fn generate_settings_json() -> String {
    let mut allow: Vec<String> = Vec::new();

    // Orchestrator database tools (homeroute MCP server)
    let db_tools = [
        "db_overview", "db_list_tables", "db_describe_table", "db_create_table",
        "db_add_column", "db_remove_column", "db_drop_table", "db_create_relation",
        "db_get_schema", "db_get_db_info", "db_query_data", "db_insert_data",
        "db_update_data", "db_delete_data", "db_count_rows",
    ];
    for name in &db_tools {
        allow.push(format!("mcp__homeroute__{name}"));
    }

    // Store tools
    for t in get_store_tool_definitions() {
        if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
            allow.push(format!("mcp__store__{name}"));
        }
    }

    // Studio tools
    for t in get_studio_tool_definitions() {
        if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
            allow.push(format!("mcp__studio__{name}"));
        }
    }

    // Docs tools
    for t in get_docs_tool_definitions() {
        if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
            allow.push(format!("mcp__docs__{name}"));
        }
    }

    serde_json::to_string_pretty(&json!({
        "permissions": {
            "allow": allow
        }
    }))
    .unwrap()
}

// ── Studio MCP Server ──────────────────────────────────────────────────

pub async fn run_studio_mcp_server() -> Result<()> {
    info!("Starting MCP Studio server");

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = io::stdout();

    use tokio::io::AsyncBufReadExt;
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
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
                    "name": "hr-studio",
                    "version": "0.1.0"
                },
                "instructions": "Studio workflow tools. Use todo_save after each TodoWrite call, and todo_load when resuming work."
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                let tools = get_studio_tool_definitions();
                Ok(json!({ "tools": tools }))
            }
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
                handle_studio_tool_call(tool_name, &arguments).await
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

fn get_studio_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "todo_save",
            "description": "Save the current todo list to persistent storage. Call this after every TodoWrite. Supports flat todos or phased structure (use one or the other).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "status": { "type": "string" },
                                "activeForm": { "type": "string" }
                            }
                        },
                        "description": "Flat todo list (simple activities without phases)"
                    },
                    "phases": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "status": { "type": "string" },
                                "todos": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "content": { "type": "string" },
                                            "status": { "type": "string" },
                                            "activeForm": { "type": "string" }
                                        }
                                    }
                                }
                            }
                        },
                        "description": "Phased todo list (complex activities). Each phase has a name, status, and its own todo list."
                    }
                }
            }
        }),
        json!({
            "name": "todo_load",
            "description": "Load the previously saved todo list. Returns either a flat JSON array (simple) or a JSON object with a 'phases' key (phased structure).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "browser_screenshot",
            "description": "Capture a screenshot of the dev site using headless Chromium. Returns the screenshot as a base64-encoded PNG image. Installs Chrome on first use if not present.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to capture (default: http://localhost:5173)"
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
                }
            }
        }),
        json!({
            "name": "browser_console_logs",
            "description": "Read browser console logs captured from the dev preview iframe. Logs are intercepted via injected script and stored server-side.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "level": {
                        "type": "string",
                        "description": "Filter by log level (log, warn, error, info, debug)"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 100,
                        "description": "Maximum number of log entries to return (default: 100)"
                    },
                    "since": {
                        "type": "integer",
                        "description": "Only return logs after this timestamp (ms since epoch)"
                    }
                }
            }
        }),
        json!({
            "name": "browser_console_clear",
            "description": "Clear all stored browser console logs.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}

/// Take a screenshot using puppeteer-core via Node.js CDP protocol.
/// Falls back to installing Chrome + puppeteer-core if not present.
async fn puppeteer_screenshot(
    url: &str,
    screenshot_path: &str,
    width: u64,
    height: u64,
    wait_ms: u64,
) -> Result<Value, String> {
    use base64::Engine;

    // Find Chrome binary (installed at container startup by hr-agent root process)
    // Resolve Chrome binary to absolute path (puppeteer requires it)
    let candidates = ["google-chrome-stable", "chromium-browser", "chromium"];
    let mut chrome_bin = String::new();
    for candidate in &candidates {
        let check = tokio::process::Command::new("which")
            .arg(candidate)
            .output()
            .await;
        if let Ok(output) = check {
            if output.status.success() {
                chrome_bin = String::from_utf8_lossy(&output.stdout).trim().to_string();
                break;
            }
        }
    }

    if chrome_bin.is_empty() {
        return Ok(json!({
            "content": [{ "type": "text", "text": "ERROR: Chrome is not installed. It should be auto-installed at container startup. Try restarting the hr-agent service." }]
        }));
    }

    // Write Node.js script to temp file — parameters passed via process.argv
    let script_path = "/tmp/_puppeteer_screenshot.cjs";
    let node_script = r#"const puppeteer = require('puppeteer-core');
const [,, chromeBin, url, outPath, w, h, waitMs] = process.argv;
(async () => {
    const browser = await puppeteer.launch({
        executablePath: chromeBin,
        headless: 'new',
        args: ['--no-sandbox', '--disable-gpu', '--disable-software-rasterizer', '--disable-dev-shm-usage']
    });
    const page = await browser.newPage();
    await page.setViewport({ width: parseInt(w), height: parseInt(h) });
    await page.goto(url, { waitUntil: 'networkidle2', timeout: Math.max(parseInt(waitMs) + 10000, 15000) });
    await new Promise(r => setTimeout(r, Math.min(parseInt(waitMs), 5000)));
    await page.screenshot({ path: outPath });
    await browser.close();
})().catch(e => { console.error(e.message); process.exit(1); });
"#;

    tokio::fs::write(script_path, node_script)
        .await
        .map_err(|e| format!("Failed to write screenshot script: {e}"))?;

    // Get NODE_PATH for global modules
    let npm_root = tokio::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
        .await
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        } else { None })
        .unwrap_or_default();

    let output = tokio::process::Command::new("timeout")
        .args(["30", "node", script_path,
            &chrome_bin, url, screenshot_path,
            &width.to_string(), &height.to_string(), &wait_ms.to_string()])
        .env("NODE_PATH", &npm_root)
        .output()
        .await
        .map_err(|e| format!("Failed to run puppeteer screenshot: {e}"))?;

    let _ = tokio::fs::remove_file(script_path).await;

    if !std::path::Path::new(screenshot_path).exists() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(json!({
            "content": [{ "type": "text", "text": format!(
                "ERROR: Screenshot failed.\nstdout: {}\nstderr: {}",
                stdout.chars().take(500).collect::<String>(),
                stderr.chars().take(500).collect::<String>()
            ) }]
        }));
    }

    let screenshot_data = tokio::fs::read(screenshot_path)
        .await
        .map_err(|e| format!("Failed to read screenshot: {e}"))?;
    let _ = tokio::fs::remove_file(screenshot_path).await;

    let base64_data = base64::engine::general_purpose::STANDARD.encode(&screenshot_data);

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

async fn handle_studio_tool_call(tool: &str, args: &Value) -> Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    match tool {
        "todo_save" => {
            let path = std::path::Path::new("/root/workspace/.studio-todos.json");
            let (content, msg) = if let Some(phases) = args.get("phases") {
                let data = json!({"phases": phases});
                let serialized = serde_json::to_string_pretty(&data)
                    .map_err(|e| format!("Failed to serialize phases: {e}"))?;
                let count = phases.as_array().map(|a| a.len()).unwrap_or(0);
                (serialized, format!("Saved {} phase(s) to persistent storage.", count))
            } else {
                let empty = json!([]);
                let todos = args.get("todos").unwrap_or(&empty);
                let serialized = serde_json::to_string_pretty(todos)
                    .map_err(|e| format!("Failed to serialize todos: {e}"))?;
                let count = todos.as_array().map(|a| a.len()).unwrap_or(0);
                (serialized, format!("Saved {} todo(s) to persistent storage.", count))
            };
            std::fs::write(path, &content)
                .map_err(|e| format!("Failed to write todos file: {e}"))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
                .map_err(|e| format!("Failed to set permissions: {e}"))?;

            // Notify the todo_watcher by writing to /tmp/studio-todos/{session_id}.json
            // Try env var first, then detect from tmux session name (studio-{uuid})
            let session_id = std::env::var("STUDIO_SESSION_ID").ok().or_else(|| {
                std::process::Command::new("tmux")
                    .args(["display-message", "-p", "#{session_name}"])
                    .output()
                    .ok()
                    .and_then(|o| {
                        let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        name.strip_prefix("studio-").map(|s| s.to_string())
                    })
            });
            if let Some(sid) = session_id {
                let notify_dir = std::path::Path::new("/tmp/studio-todos");
                let _ = std::fs::create_dir_all(notify_dir);
                let notify_path = notify_dir.join(format!("{}.json", sid));
                let _ = std::fs::write(&notify_path, &content);
            }

            Ok(json!({
                "content": [{ "type": "text", "text": msg }]
            }))
        }
        "todo_load" => {
            let path = std::path::Path::new("/root/workspace/.studio-todos.json");
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => "[]".to_string(),
            };
            Ok(json!({
                "content": [{ "type": "text", "text": content }]
            }))
        }

        "browser_screenshot" => {
            let url = args.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("http://localhost:5173");
            let width = args.get("width")
                .and_then(|v| v.as_u64())
                .unwrap_or(1280);
            let height = args.get("height")
                .and_then(|v| v.as_u64())
                .unwrap_or(720);
            let wait_ms = args.get("wait_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000);

            return puppeteer_screenshot(url, "/tmp/studio-screenshot.png", width, height, wait_ms).await;
        }

        "browser_console_logs" => {
            let level_filter = args.get("level").and_then(|v| v.as_str());
            let limit = args.get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as usize;
            let since = args.get("since").and_then(|v| v.as_u64());

            let log_path = "/tmp/studio-console-logs.json";
            let logs: Vec<Value> = match tokio::fs::read_to_string(log_path).await {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Vec::new(),
            };

            if logs.is_empty() {
                return Ok(text_result("No console logs recorded.".to_string()));
            }

            // Filter and limit
            let filtered: Vec<&Value> = logs.iter()
                .filter(|entry| {
                    if let Some(level) = level_filter {
                        entry.get("level").and_then(|l| l.as_str()) == Some(level)
                    } else {
                        true
                    }
                })
                .filter(|entry| {
                    if let Some(ts) = since {
                        entry.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0) > ts
                    } else {
                        true
                    }
                })
                .collect();

            let total = filtered.len();
            let display: Vec<&Value> = filtered.into_iter().rev().take(limit).collect::<Vec<_>>().into_iter().rev().collect();

            let mut output = format!("Console logs ({} shown, {} total):\n\n", display.len(), total);
            for entry in &display {
                let level = entry.get("level").and_then(|l| l.as_str()).unwrap_or("log");
                let message = entry.get("message").and_then(|m| m.as_str()).unwrap_or("");
                let ts = entry.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0);
                let icon = match level {
                    "error" => "[ERROR]",
                    "warn" => "[WARN]",
                    "info" => "[INFO]",
                    "debug" => "[DEBUG]",
                    _ => "[LOG]",
                };
                output.push_str(&format!("{} {} (ts: {})\n", icon, message, ts));
            }

            Ok(text_result(output))
        }

        "browser_console_clear" => {
            let log_path = "/tmp/studio-console-logs.json";
            let _ = tokio::fs::remove_file(log_path).await;
            Ok(text_result("Console logs cleared.".to_string()))
        }

        _ => Err(format!("Unknown studio tool: {}", tool)),
    }
}


// ── Docs MCP Server ────────────────────────────────────────────────────

pub async fn run_docs_mcp_server() -> Result<()> {
    info!("Starting MCP Docs server");

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = io::stdout();

    use tokio::io::AsyncBufReadExt;
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
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
                    "name": "hr-docs",
                    "version": "0.1.0"
                },
                "instructions": "Documentation tools for app screens and flows. Always call get_docs before modifying documentation to understand the current state."
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                let tools = get_docs_tool_definitions();
                Ok(json!({ "tools": tools }))
            }
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
                handle_docs_tool_call(tool_name, &arguments).await
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

fn get_docs_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_docs",
            "description": "Read the app documentation. Returns all docs or a specific section.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "section": {
                        "type": "string",
                        "enum": ["all", "app", "screens", "flows"],
                        "description": "Section to retrieve (default: all)"
                    }
                }
            }
        }),
        json!({
            "name": "update_app_info",
            "description": "Update the app overview information. Only provided fields are updated.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "App name" },
                    "description": { "type": "string", "description": "Short tagline (1 sentence)" },
                    "business_context": { "type": "string", "description": "Paragraph explaining the problem solved and value" },
                    "target_users": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of target user personas"
                    }
                }
            }
        }),
        json!({
            "name": "upsert_screen",
            "description": "Create or update a screen. If a screen with the given id exists, only provided fields are updated. Otherwise a new screen is created.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique screen identifier" },
                    "name": { "type": "string", "description": "Screen display name" },
                    "path": { "type": "string", "description": "Route path (e.g. /dashboard)" },
                    "description": { "type": "string", "description": "User-oriented description of the screen" },
                    "features": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of user-facing features"
                    },
                    "related_tables": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Dataverse tables used by this screen"
                    },
                    "related_flows": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Flow IDs related to this screen"
                    }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "delete_screen",
            "description": "Delete a screen by id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Screen id to delete" }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "upsert_flow",
            "description": "Create or update a user flow. Metadata (name, description) is merged; steps are replaced entirely if provided.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique flow identifier" },
                    "name": { "type": "string", "description": "Flow display name" },
                    "description": { "type": "string", "description": "Flow description" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "label": { "type": "string" },
                                "type": {
                                    "type": "string",
                                    "enum": ["state", "action", "decision"]
                                },
                                "next": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Next step IDs (for state/action)"
                                },
                                "outcomes": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string" },
                                            "next": { "type": "string" }
                                        }
                                    },
                                    "description": "Decision outcomes (for decision type)"
                                }
                            }
                        },
                        "description": "Flow steps (replaces all existing steps if provided)"
                    }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "delete_flow",
            "description": "Delete a flow by id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Flow id to delete" }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "list_screens",
            "description": "List all screens (summary: id, name, path).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "list_flows",
            "description": "List all flows (summary: id, name, step_count).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}

const DOCS_PATH: &str = "/root/workspace/docs.json";

fn load_docs() -> Value {
    match std::fs::read_to_string(DOCS_PATH) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| default_docs()),
        Err(_) => default_docs(),
    }
}

fn default_docs() -> Value {
    json!({
        "version": 0,
        "updated_at": "",
        "app": {
            "name": "",
            "description": "",
            "business_context": "",
            "target_users": []
        },
        "screens": [],
        "flows": []
    })
}

/// Decode literal `\uXXXX` sequences in strings within a JSON Value tree.
/// Claude Code sometimes sends unicode escapes as literal backslash sequences
/// (e.g. `\\u00e9` instead of `é`), which end up double-escaped in the file.
fn decode_unicode_escapes(val: &mut Value) {
    match val {
        Value::String(s) => {
            if s.contains("\\u") {
                let mut result = String::with_capacity(s.len());
                let mut chars = s.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        // Peek at next char
                        let mut tmp = chars.clone();
                        if tmp.next() == Some('u') {
                            let hex: String = tmp.by_ref().take(4).collect();
                            if hex.len() == 4 && hex.chars().all(|h| h.is_ascii_hexdigit()) {
                                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(code) {
                                        result.push(ch);
                                        // Advance the real iterator past 'u' + 4 hex digits
                                        chars.next(); // 'u'
                                        for _ in 0..4 { chars.next(); }
                                        continue;
                                    }
                                }
                            }
                        }
                        result.push(c);
                    } else {
                        result.push(c);
                    }
                }
                *s = result;
            }
        }
        Value::Array(arr) => arr.iter_mut().for_each(decode_unicode_escapes),
        Value::Object(map) => map.values_mut().for_each(decode_unicode_escapes),
        _ => {}
    }
}

fn save_docs(docs: &mut Value) -> std::result::Result<(), String> {
    // Decode any literal \uXXXX sequences before saving
    decode_unicode_escapes(docs);

    // Increment version
    let version = docs.get("version").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
    docs["version"] = json!(version);

    // Set updated_at with ISO timestamp
    let now = chrono::Utc::now().to_rfc3339();
    docs["updated_at"] = json!(now);

    let content = serde_json::to_string_pretty(docs).map_err(|e| e.to_string())?;

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(DOCS_PATH).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    std::fs::write(DOCS_PATH, &content).map_err(|e| e.to_string())?;
    std::fs::set_permissions(DOCS_PATH, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn handle_docs_tool_call(tool: &str, args: &Value) -> std::result::Result<Value, String> {
    let text_result = |text: String| -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    };

    match tool {
        "get_docs" => {
            let docs = load_docs();
            let section = args.get("section").and_then(|v| v.as_str()).unwrap_or("all");
            let result = match section {
                "app" => json!({ "app": docs["app"] }),
                "screens" => json!({ "screens": docs["screens"] }),
                "flows" => json!({ "flows": docs["flows"] }),
                _ => docs,
            };
            Ok(text_result(serde_json::to_string_pretty(&result).unwrap()))
        }

        "update_app_info" => {
            let mut docs = load_docs();
            if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                docs["app"]["name"] = json!(name);
            }
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                docs["app"]["description"] = json!(desc);
            }
            if let Some(ctx) = args.get("business_context").and_then(|v| v.as_str()) {
                docs["app"]["business_context"] = json!(ctx);
            }
            if let Some(users) = args.get("target_users").and_then(|v| v.as_array()) {
                docs["app"]["target_users"] = json!(users);
            }
            save_docs(&mut docs)?;
            Ok(text_result("App info updated.".to_string()))
        }

        "upsert_screen" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or("Missing required field: id")?;
            let mut docs = load_docs();
            let screens = docs["screens"].as_array_mut()
                .ok_or("Invalid screens array")?;

            if let Some(existing) = screens.iter_mut().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(id)) {
                // Merge: only overwrite provided fields
                if let Some(v) = args.get("name") { existing["name"] = v.clone(); }
                if let Some(v) = args.get("path") { existing["path"] = v.clone(); }
                if let Some(v) = args.get("description") { existing["description"] = v.clone(); }
                if let Some(v) = args.get("features") { existing["features"] = v.clone(); }
                if let Some(v) = args.get("related_tables") { existing["related_tables"] = v.clone(); }
                if let Some(v) = args.get("related_flows") { existing["related_flows"] = v.clone(); }
            } else {
                // Create new screen
                screens.push(json!({
                    "id": id,
                    "name": args.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "path": args.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": args.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "features": args.get("features").cloned().unwrap_or(json!([])),
                    "related_tables": args.get("related_tables").cloned().unwrap_or(json!([])),
                    "related_flows": args.get("related_flows").cloned().unwrap_or(json!([]))
                }));
            }
            save_docs(&mut docs)?;
            Ok(text_result(format!("Screen '{}' upserted.", id)))
        }

        "delete_screen" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or("Missing required field: id")?;
            let mut docs = load_docs();
            if let Some(screens) = docs["screens"].as_array_mut() {
                screens.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(id));
            }
            save_docs(&mut docs)?;
            Ok(text_result(format!("Screen '{}' deleted.", id)))
        }

        "upsert_flow" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or("Missing required field: id")?;
            let mut docs = load_docs();
            let flows = docs["flows"].as_array_mut()
                .ok_or("Invalid flows array")?;

            if let Some(existing) = flows.iter_mut().find(|f| f.get("id").and_then(|v| v.as_str()) == Some(id)) {
                // Merge metadata
                if let Some(v) = args.get("name") { existing["name"] = v.clone(); }
                if let Some(v) = args.get("description") { existing["description"] = v.clone(); }
                // Replace steps entirely if provided
                if let Some(v) = args.get("steps") { existing["steps"] = v.clone(); }
            } else {
                // Create new flow
                flows.push(json!({
                    "id": id,
                    "name": args.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": args.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "steps": args.get("steps").cloned().unwrap_or(json!([]))
                }));
            }
            save_docs(&mut docs)?;
            Ok(text_result(format!("Flow '{}' upserted.", id)))
        }

        "delete_flow" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or("Missing required field: id")?;
            let mut docs = load_docs();
            if let Some(flows) = docs["flows"].as_array_mut() {
                flows.retain(|f| f.get("id").and_then(|v| v.as_str()) != Some(id));
            }
            save_docs(&mut docs)?;
            Ok(text_result(format!("Flow '{}' deleted.", id)))
        }

        "list_screens" => {
            let docs = load_docs();
            let screens = docs["screens"].as_array()
                .map(|arr| arr.iter().map(|s| json!({
                    "id": s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": s.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "path": s.get("path").and_then(|v| v.as_str()).unwrap_or("")
                })).collect::<Vec<_>>())
                .unwrap_or_default();
            Ok(text_result(serde_json::to_string_pretty(&screens).unwrap()))
        }

        "list_flows" => {
            let docs = load_docs();
            let flows = docs["flows"].as_array()
                .map(|arr| arr.iter().map(|f| json!({
                    "id": f.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "step_count": f.get("steps").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
                })).collect::<Vec<_>>())
                .unwrap_or_default();
            Ok(text_result(serde_json::to_string_pretty(&flows).unwrap()))
        }

        _ => Err(format!("Unknown docs tool: {}", tool)),
    }
}
