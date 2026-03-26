use serde_json::{json, Value};

use crate::mcp::{tool_error, tool_success, McpState, INVALID_PARAMS};
use crate::ssh::SshClient;

pub async fn tool_db_tables(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let Some(db_path) = &project.prod.db_path else {
        return tool_error(id, &format!("No database configured for project '{slug}'"));
    };

    let ssh = SshClient::from_config(&state.config);
    let cmd = format!(
        r#"sqlite3 {db_path} "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;" | while read table; do count=$(sqlite3 {db_path} "SELECT COUNT(*) FROM \"$table\";"); echo "$table|$count"; done"#,
    );

    match ssh.exec_in_container(&project.prod.container_name, &cmd).await {
        Ok(r) => {
            let tables: Vec<Value> = r
                .stdout
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.splitn(2, '|').collect();
                    json!({
                        "name": parts.first().unwrap_or(&""),
                        "rows": parts.get(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
                    })
                })
                .collect();

            tool_success(id, json!({
                "slug": slug,
                "db_path": db_path,
                "tables": tables,
            }))
        }
        Err(e) => tool_error(id, &format!("Failed to list tables: {e}")),
    }
}

pub async fn tool_db_schema(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let Some(db_path) = &project.prod.db_path else {
        return tool_error(id, &format!("No database configured for project '{slug}'"));
    };

    let ssh = SshClient::from_config(&state.config);
    let cmd = format!("sqlite3 {db_path} .schema");

    match ssh.exec_in_container(&project.prod.container_name, &cmd).await {
        Ok(r) => tool_success(id, json!({
            "slug": slug,
            "db_path": db_path,
            "schema": r.stdout,
        })),
        Err(e) => tool_error(id, &format!("Failed to get schema: {e}")),
    }
}

pub async fn tool_db_query(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(sql) = args.get("sql").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing sql".into());
    };

    let trimmed = sql.trim().to_uppercase();
    if !trimmed.starts_with("SELECT") && !trimmed.starts_with("PRAGMA") {
        return tool_error(id, "Only SELECT and PRAGMA statements are allowed");
    }

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let Some(db_path) = &project.prod.db_path else {
        return tool_error(id, &format!("No database configured for project '{slug}'"));
    };

    let ssh = SshClient::from_config(&state.config);
    let cmd = format!("sqlite3 -json {db_path} {}", crate::ssh::shell_quote(sql));

    match ssh.exec_in_container(&project.prod.container_name, &cmd).await {
        Ok(r) => {
            let data: Value = serde_json::from_str(&r.stdout).unwrap_or(Value::String(r.stdout));
            tool_success(id, json!({
                "slug": slug,
                "sql": sql,
                "data": data,
            }))
        }
        Err(e) => tool_error(id, &format!("Query failed: {e}")),
    }
}
