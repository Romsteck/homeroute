use serde_json::{json, Value};
use std::path::Path;
use tokio::process::Command;

use crate::mcp::{tool_error, tool_success, McpState, INVALID_PARAMS};

pub async fn tool_status(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let dev_exists = Path::new(&project.dev_path).exists();

    let git_info = if dev_exists {
        get_git_info(&project.dev_path).await
    } else {
        json!({ "error": "DEV directory not found" })
    };

    let ssh = crate::ssh::SshClient::from_config(&state.config);
    let service_status = match ssh
        .exec_in_container(
            &project.prod.container_name,
            &format!("systemctl is-active {}", project.prod.service),
        )
        .await
    {
        Ok(r) => r.stdout.trim().to_string(),
        Err(e) => format!("error: {e}"),
    };

    tool_success(
        id,
        json!({
            "slug": project.slug,
            "name": project.name,
            "stack": project.stack.as_str(),
            "dev_path": project.dev_path,
            "dev_exists": dev_exists,
            "prod": {
                "container": project.prod.container_name,
                "ip": project.prod.ip,
                "service_status": service_status,
            },
            "git": git_info,
            "domain": project.domain,
            "last_deployed_at": project.last_deployed_at,
            "last_deploy_commit": project.last_deploy_commit,
        }),
    )
}

async fn get_git_info(dev_path: &str) -> Value {
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(["-C", dev_path]).args(args);
        cmd
    };

    let status = git(&["status", "--porcelain"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let dirty = !status.trim().is_empty();

    let last_commit = git(&["log", "-1", "--format=%H|%s|%ai"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let (hash, subject, date) = if let Some((h, rest)) = last_commit.split_once('|') {
        if let Some((s, d)) = rest.split_once('|') {
            (h.to_string(), s.to_string(), d.to_string())
        } else {
            (h.to_string(), rest.to_string(), String::new())
        }
    } else {
        (String::new(), String::new(), String::new())
    };

    let ahead_behind = git(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let (ahead, behind) = if let Some((a, b)) = ahead_behind.split_once('\t') {
        (
            a.parse::<i64>().unwrap_or(0),
            b.parse::<i64>().unwrap_or(0),
        )
    } else {
        (0, 0)
    };

    json!({
        "dirty": dirty,
        "ahead": ahead,
        "behind": behind,
        "last_commit": { "hash": hash, "subject": subject, "date": date }
    })
}
