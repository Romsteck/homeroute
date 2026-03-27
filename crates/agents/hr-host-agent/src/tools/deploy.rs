use serde_json::{json, Value};
use std::path::Path;
use tokio::process::Command;
use tracing::{info, error};

use crate::mcp::{tool_error, tool_success, McpState, INVALID_PARAMS};
use crate::registry::ProjectStack;
use crate::ssh::SshClient;

pub async fn tool_deploy(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    if !Path::new(&project.dev_path).exists() {
        return tool_error(id, &format!("DEV directory not found: {}", project.dev_path));
    }

    // Validate service name
    if project.prod.service.is_empty() || !project.prod.service.contains('.') {
        return tool_error(id, &format!(
            "Invalid service name '{}' for '{}'. Expected format: 'name.service'",
            project.prod.service, slug
        ));
    }

    info!(slug, stack = project.stack.as_str(), "Starting deploy");

    let ssh = SshClient::from_config(&state.config);
    let mut steps: Vec<Value> = Vec::new();

    let result = match project.stack {
        ProjectStack::AxumViteReact => {
            deploy_axum_vite_react(&project, &ssh, &mut steps).await
        }
        ProjectStack::NextJs => {
            deploy_nextjs(&project, &ssh, &mut steps).await
        }
        ProjectStack::AxumFlutter => {
            deploy_axum_flutter(&project, &ssh, &mut steps).await
        }
    };

    match result {
        Ok(()) => {
            // Update last deploy timestamp
            let now = chrono::Utc::now().to_rfc3339();
            let _ = state.registry.update(slug, |p| {
                p.last_deployed_at = Some(now.clone());
            }).await;

            info!(slug, "Deploy completed successfully");
            tool_success(id, json!({
                "slug": slug,
                "status": "success",
                "steps": steps,
            }))
        }
        Err(e) => {
            error!(slug, error = %e, "Deploy failed");
            steps.push(json!({ "step": "error", "error": e }));
            tool_error(id, &format!("Deploy failed: {e}\nSteps: {}", serde_json::to_string_pretty(&steps).unwrap_or_default()))
        }
    }
}

async fn deploy_axum_vite_react(
    project: &crate::registry::Project,
    ssh: &SshClient,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let cargo_dir = project.cargo_dir();
    let container = &project.prod.container_name;
    let binary = project.prod.binary.as_deref().unwrap_or("/opt/app/app");
    let static_dir = project.prod.static_dir.as_deref().unwrap_or("/opt/app/dist");

    // 1. Cargo build
    steps.push(json!({ "step": format!("cargo build --release (in {cargo_dir})") }));
    let output = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&cargo_dir)
        .output()
        .await
        .map_err(|e| format!("cargo build failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo build failed:\n{stderr}"));
    }
    steps.push(json!({ "step": "cargo build done" }));

    // Find the binary name from Cargo.toml
    let binary_name = find_binary_name(&cargo_dir).await?;
    let local_binary = format!("{cargo_dir}/target/release/{binary_name}");

    // 2. Build frontend
    if let Some(web_dir) = project.web_dir() {
        steps.push(json!({ "step": format!("pnpm build (frontend in {web_dir})") }));
        let output = Command::new("pnpm")
            .args(["build"])
            .current_dir(&web_dir)
            .output()
            .await
            .map_err(|e| format!("pnpm build failed: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("pnpm build failed:\n{stderr}"));
        }
        steps.push(json!({ "step": "frontend build done" }));
    }

    // 3. SCP binary to prod
    let tmp_binary = format!("/tmp/{binary_name}");
    steps.push(json!({ "step": format!("scp binary to prod ({tmp_binary})") }));
    ssh.scp(&local_binary, &tmp_binary).await?;

    // 4. Copy binary into container
    steps.push(json!({ "step": "copy binary into container" }));
    ssh.copy_to_container(container, &tmp_binary, binary).await?;

    // 5. Copy frontend dist if built
    if let Some(web_dir) = project.web_dir() {
        let local_dist = format!("{web_dir}/dist/");
        if Path::new(&local_dist).exists() {
            let tmp_dist = format!("/tmp/{}-dist", project.slug);
            steps.push(json!({ "step": "rsync dist to prod" }));
            ssh.rsync(&local_dist, &tmp_dist).await?;

            steps.push(json!({ "step": "copy dist into container" }));
            ssh.copy_to_container(container, &tmp_dist, static_dir).await?;
        }
    }

    // 6. Restart service
    steps.push(json!({ "step": "restart app.service" }));
    let r = ssh.exec_in_container(container, &format!("systemctl restart {}", project.prod.service)).await?;
    if !r.success {
        return Err(format!("restart failed: {}", r.stderr.trim()));
    }

    // 7. Health check
    steps.push(json!({ "step": "health check" }));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if let Some(ip) = &project.prod.ip {
        let r = ssh.exec(&format!("curl -sf http://{}:3000/api/health || curl -sf http://{}:3000/", ip, ip)).await?;
        steps.push(json!({ "step": "health check result", "success": r.success }));
        if !r.success {
            return Err("Health check failed after restart".into());
        }
    }

    Ok(())
}

async fn deploy_nextjs(
    project: &crate::registry::Project,
    ssh: &SshClient,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let dev = &project.dev_path;
    let container = &project.prod.container_name;

    // 1. pnpm build
    steps.push(json!({ "step": "pnpm build" }));
    let output = Command::new("pnpm")
        .args(["build"])
        .current_dir(dev)
        .output()
        .await
        .map_err(|e| format!("pnpm build failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pnpm build failed:\n{stderr}"));
    }
    steps.push(json!({ "step": "pnpm build done" }));

    // 2. rsync .next/, public/, package.json, next.config.* to prod
    let tmp_dir = format!("/tmp/{}-next", project.slug);
    steps.push(json!({ "step": format!("rsync to {tmp_dir}") }));

    // rsync the essential Next.js files
    let output = Command::new("rsync")
        .args([
            "-az", "--delete",
            "--include=.next/***",
            "--include=public/***",
            "--include=package.json",
            "--include=next.config.*",
            "--include=node_modules/***",
            "--exclude=*",
            &format!("{dev}/"),
            &format!("{}:{tmp_dir}/", ssh.target()),
        ])
        .output()
        .await
        .map_err(|e| format!("rsync failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("rsync failed:\n{stderr}"));
    }
    steps.push(json!({ "step": "rsync done" }));

    // 3. Copy into container
    steps.push(json!({ "step": "copy into container" }));
    ssh.copy_to_container(container, &tmp_dir, "/opt/app").await?;

    // 4. Restart
    steps.push(json!({ "step": "restart app.service" }));
    let r = ssh.exec_in_container(container, &format!("systemctl restart {}", project.prod.service)).await?;
    if !r.success {
        return Err(format!("restart failed: {}", r.stderr.trim()));
    }

    // 5. Health check
    steps.push(json!({ "step": "health check" }));
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    if let Some(ip) = &project.prod.ip {
        let r = ssh.exec(&format!("curl -sf http://{}:3000/", ip)).await?;
        steps.push(json!({ "step": "health check result", "success": r.success }));
        if !r.success {
            return Err("Health check failed after restart".into());
        }
    }

    Ok(())
}

async fn deploy_axum_flutter(
    project: &crate::registry::Project,
    ssh: &SshClient,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let cargo_dir = project.cargo_dir();
    let container = &project.prod.container_name;
    let binary = project.prod.binary.as_deref().unwrap_or("/opt/app/app");

    // 1. Cargo build
    steps.push(json!({ "step": format!("cargo build --release (in {cargo_dir})") }));
    let output = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&cargo_dir)
        .output()
        .await
        .map_err(|e| format!("cargo build failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo build failed:\n{stderr}"));
    }
    steps.push(json!({ "step": "cargo build done" }));

    let binary_name = find_binary_name(&cargo_dir).await?;
    let local_binary = format!("{cargo_dir}/target/release/{binary_name}");

    // 2. SCP to prod
    let tmp_binary = format!("/tmp/{binary_name}");
    steps.push(json!({ "step": format!("scp binary to prod ({tmp_binary})") }));
    ssh.scp(&local_binary, &tmp_binary).await?;

    // 3. Copy into container
    steps.push(json!({ "step": "copy binary into container" }));
    ssh.copy_to_container(container, &tmp_binary, binary).await?;

    // 4. Restart
    steps.push(json!({ "step": "restart app.service" }));
    let r = ssh.exec_in_container(container, &format!("systemctl restart {}", project.prod.service)).await?;
    if !r.success {
        return Err(format!("restart failed: {}", r.stderr.trim()));
    }

    // 5. Health check
    steps.push(json!({ "step": "health check" }));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if let Some(ip) = &project.prod.ip {
        let r = ssh.exec(&format!("curl -sf http://{}:3000/api/health || curl -sf http://{}:3000/", ip, ip)).await?;
        steps.push(json!({ "step": "health check result", "success": r.success }));
    }

    Ok(())
}

async fn find_binary_name(dev_path: &str) -> Result<String, String> {
    // Parse Cargo.toml to find [[bin]] name, or use package name
    let cargo_toml = format!("{dev_path}/Cargo.toml");
    let content = tokio::fs::read_to_string(&cargo_toml)
        .await
        .map_err(|e| format!("Failed to read {cargo_toml}: {e}"))?;

    // Quick parse: look for name in [[bin]] section or [package]
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") && trimmed.contains('=') {
            if let Some(name) = trimmed.split('=').nth(1) {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() {
                    return Ok(name.to_string());
                }
            }
        }
    }
    Err("Could not determine binary name from Cargo.toml".into())
}

pub async fn tool_deploy_status(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let ssh = SshClient::from_config(&state.config);
    let cmd = format!("systemctl status {}", project.prod.service);
    match ssh.exec_in_container(&project.prod.container_name, &cmd).await {
        Ok(r) => tool_success(id, json!({
            "slug": slug,
            "container": project.prod.container_name,
            "output": r.stdout,
        })),
        Err(e) => tool_error(id, &format!("Failed to get status: {e}")),
    }
}

pub async fn tool_deploy_logs(id: Value, args: &Value, state: &McpState) -> Value {
    let Some(slug) = args.get("slug").and_then(|v| v.as_str()) else {
        return crate::mcp::error_response(id, INVALID_PARAMS, "Missing slug".into());
    };

    let lines = args
        .get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(50);

    let Some(project) = state.registry.get(slug).await else {
        return tool_error(id, &format!("Project '{slug}' not found"));
    };

    let ssh = SshClient::from_config(&state.config);
    let cmd = format!("journalctl -u {} -n {} --no-pager", project.prod.service, lines);
    match ssh.exec_in_container(&project.prod.container_name, &cmd).await {
        Ok(r) => tool_success(id, json!({
            "slug": slug,
            "container": project.prod.container_name,
            "lines": lines,
            "logs": r.stdout,
        })),
        Err(e) => tool_error(id, &format!("Failed to get logs: {e}")),
    }
}
