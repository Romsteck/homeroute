use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};
use std::time::Duration;

use hr_ipc::orchestrator::OrchestratorRequest;

use crate::state::ApiState;

pub fn router() -> Router<ApiState> {
    Router::new().route("/", get(dashboard))
}

/// GET /api/dashboard
///
/// Aggregates system stats in a single call for the Dashboard page.
/// Each data source has a 2s internal timeout — if it fails, the field
/// returns null instead of blocking the whole response.
async fn dashboard(State(state): State<ApiState>) -> Json<Value> {
    // Fire all data sources concurrently with individual timeouts
    let (uptime, cpu_ram, containers, apps, updates, leases, adblock, services) = tokio::join!(
        fetch_uptime(),
        fetch_cpu_ram(),
        fetch_containers(&state),
        fetch_apps(&state),
        fetch_updates_count(&state),
        fetch_dhcp_leases_count(&state),
        fetch_adblock_stats(&state),
        fetch_services(&state),
    );

    Json(json!({
        "success": true,
        "uptime_secs": uptime,
        "cpu_percent": cpu_ram.as_ref().map(|(c, _)| *c),
        "ram_percent": cpu_ram.as_ref().map(|(_, r)| *r),
        "containers_running": containers.as_ref().map(|(r, _)| *r),
        "containers_total": containers.as_ref().map(|(_, t)| *t),
        "apps_running": apps.as_ref().map(|(r, _)| *r),
        "apps_total": apps.as_ref().map(|(_, t)| *t),
        "updates_available": updates,
        "dhcp_leases": leases,
        "adblock_domains": adblock.as_ref().map(|(d, _)| *d),
        "adblock_enabled": adblock.as_ref().map(|(_, e)| *e),
        "services": services,
    }))
}

/// Read /proc/uptime — returns uptime in seconds.
async fn fetch_uptime() -> Option<u64> {
    let content = tokio::fs::read_to_string("/proc/uptime").await.ok()?;
    let secs: f64 = content.split_whitespace().next()?.parse().ok()?;
    Some(secs as u64)
}

/// Read a single CPU sample from /proc/stat.
async fn read_cpu_sample() -> Option<(u64, u64)> {
    let content = tokio::fs::read_to_string("/proc/stat").await.ok()?;
    let line = content.lines().next()?;
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 4 {
        let idle = parts[3];
        let total: u64 = parts.iter().sum();
        Some((idle, total))
    } else {
        None
    }
}

/// Read /proc/stat and /proc/meminfo for CPU% and RAM%.
async fn fetch_cpu_ram() -> Option<(f64, f64)> {
    let timeout = Duration::from_secs(2);

    let result = tokio::time::timeout(timeout, async {
        // CPU: sample two readings 200ms apart from /proc/stat
        let cpu = async {
            let (idle1, total1) = read_cpu_sample().await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            let (idle2, total2) = read_cpu_sample().await?;

            let idle_delta = idle2.saturating_sub(idle1) as f64;
            let total_delta = total2.saturating_sub(total1) as f64;
            if total_delta > 0.0 {
                Some(((1.0 - idle_delta / total_delta) * 100.0 * 10.0).round() / 10.0)
            } else {
                Some(0.0)
            }
        };

        // RAM from /proc/meminfo
        let ram = async {
            let content = tokio::fs::read_to_string("/proc/meminfo").await.ok()?;
            let mut total_kb = 0u64;
            let mut available_kb = 0u64;
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total_kb = line.split_whitespace().nth(1)?.parse().ok()?;
                } else if line.starts_with("MemAvailable:") {
                    available_kb = line.split_whitespace().nth(1)?.parse().ok()?;
                }
            }
            if total_kb > 0 {
                let pct = ((total_kb - available_kb) as f64 / total_kb as f64 * 100.0 * 10.0).round() / 10.0;
                Some(pct)
            } else {
                None
            }
        };

        let (cpu_pct, ram_pct) = tokio::join!(cpu, ram);
        match (cpu_pct, ram_pct) {
            (Some(c), Some(r)) => Some((c, r)),
            _ => None,
        }
    })
    .await;

    result.ok().flatten()
}

/// Legacy containers have been removed. Return None for backward compat.
async fn fetch_containers(_state: &ApiState) -> Option<(usize, usize)> {
    None
}

/// Count running/total applications via IPC orchestrator.
async fn fetch_apps(state: &ApiState) -> Option<(usize, usize)> {
    let timeout = Duration::from_secs(2);
    let resp = tokio::time::timeout(
        timeout,
        state.orchestrator.request(&OrchestratorRequest::ListApplications),
    )
    .await
    .ok()?
    .ok()?;

    if !resp.ok {
        return None;
    }

    let data = resp.data?;
    let apps = data.as_array()?;
    let total = apps.len();
    let running = apps
        .iter()
        .filter(|a| {
            a.get("agent_connected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .count();
    Some((running, total))
}

/// Get the number of pending updates from last scan results.
async fn fetch_updates_count(state: &ApiState) -> Option<usize> {
    let timeout = Duration::from_secs(2);
    let resp = tokio::time::timeout(
        timeout,
        state
            .orchestrator
            .request(&OrchestratorRequest::GetScanResults),
    )
    .await
    .ok()?
    .ok()?;

    if !resp.ok {
        return None;
    }

    let data = resp.data?;
    let targets = data.as_object()?;

    // Count targets that have at least one upgradable package
    let mut total_upgradable = 0usize;
    for (_id, target) in targets {
        if let Some(apt) = target.get("apt_upgradable").and_then(|v| v.as_u64()) {
            total_upgradable += apt as usize;
        }
        if let Some(snap) = target.get("snap_upgradable").and_then(|v| v.as_u64()) {
            total_upgradable += snap as usize;
        }
        // Agent version mismatch counts as 1 update
        let av = target
            .get("agent_version")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let al = target
            .get("agent_version_latest")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !av.is_empty() && !al.is_empty() && av != al {
            total_upgradable += 1;
        }
    }
    Some(total_upgradable)
}

/// Get DHCP leases count from hr-netcore.
async fn fetch_dhcp_leases_count(state: &ApiState) -> Option<usize> {
    let timeout = Duration::from_secs(2);
    match tokio::time::timeout(timeout, state.netcore.dhcp_leases()).await {
        Ok(Ok(leases)) => Some(leases.len()),
        Ok(Err(_)) => None,
        Err(_) => None,
    }
}

/// Get adblock stats from hr-netcore.
async fn fetch_adblock_stats(state: &ApiState) -> Option<(usize, bool)> {
    let timeout = Duration::from_secs(2);
    match tokio::time::timeout(timeout, state.netcore.adblock_stats()).await {
        Ok(Ok(stats)) => Some((stats.domain_count, stats.enabled)),
        Ok(Err(_)) => None,
        Err(_) => None,
    }
}

/// Get services status summary.
async fn fetch_services(state: &ApiState) -> Option<Value> {
    let timeout = Duration::from_secs(2);
    let registry = state.service_registry.read().await;
    let mut services: Vec<Value> = registry
        .values()
        .map(|s| {
            json!({
                "name": s.name,
                "state": format!("{:?}", s.state).to_lowercase(),
            })
        })
        .collect();

    // Merge netcore services
    if let Ok(Ok(netcore_svcs)) =
        tokio::time::timeout(timeout, state.netcore.service_status()).await
    {
        for entry in netcore_svcs {
            services.push(json!({
                "name": entry.name,
                "state": entry.state,
            }));
        }
    }

    Some(json!(services))
}
