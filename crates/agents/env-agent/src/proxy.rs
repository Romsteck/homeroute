use serde_json::Value;
use tracing::{debug, warn};

/// Build a JSON-RPC 2.0 request for MCP tools/call.
fn build_jsonrpc_request(tool_name: &str, arguments: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        }
    })
}

/// Extract the result from a JSON-RPC response.
///
/// On success, parses `result.content[0].text` — if the text is valid JSON,
/// returns it parsed; otherwise returns it as a string Value.
/// On error (JSON-RPC error or `isError: true` in result), returns Err.
fn extract_response(body: Value) -> Result<Value, String> {
    // Check for JSON-RPC error
    if let Some(error) = body.get("error") {
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown JSON-RPC error");
        return Err(message.to_string());
    }

    let result = body
        .get("result")
        .ok_or_else(|| "Missing 'result' in JSON-RPC response".to_string())?;

    // Check for isError flag in result
    if result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
        let text = result
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or("Tool returned an error");
        return Err(text.to_string());
    }

    let text = result
        .pointer("/content/0/text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Missing content[0].text in response".to_string())?;

    // Try to parse as JSON, fall back to string Value
    match serde_json::from_str::<Value>(text) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Ok(Value::String(text.to_string())),
    }
}

/// Proxy an MCP tool call to the orchestrator MCP server.
/// URL: http://{address}:{port}/mcp
/// Auth: Bearer token
pub async fn proxy_to_orchestrator(
    client: &reqwest::Client,
    address: &str,
    port: u16,
    token: &str,
    tool_name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let url = format!("http://{}:{}/mcp", address, port);
    let payload = build_jsonrpc_request(tool_name, arguments);

    debug!(url, tool_name, "Proxying to orchestrator");

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .timeout(std::time::Duration::from_secs(15))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Orchestrator request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        warn!(status = %status, "Orchestrator returned error");
        return Err(format!("Orchestrator HTTP {status}: {text}"));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse orchestrator response: {e}"))?;

    extract_response(body)
}

/// Proxy an MCP tool call to the hub MCP server.
/// URL: http://{hub_url}/api/mcp
/// No auth required.
pub async fn proxy_to_hub(
    client: &reqwest::Client,
    hub_url: &str,
    tool_name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let url = format!("{}/api/mcp", hub_url.trim_end_matches('/'));
    let payload = build_jsonrpc_request(tool_name, arguments);

    debug!(url, tool_name, "Proxying to hub");

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Hub request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        warn!(status = %status, "Hub returned error");
        return Err(format!("Hub HTTP {status}: {text}"));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse hub response: {e}"))?;

    extract_response(body)
}
