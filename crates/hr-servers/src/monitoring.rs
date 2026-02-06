use hr_common::events::HostStatusEvent;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

const HOSTS_FILE: &str = "/data/hosts.json";
const SERVERS_FILE: &str = "/data/servers.json";
const MONITOR_INTERVAL_SECS: u64 = 30;

/// Run the host monitoring loop.
/// Pings all hosts every 30 seconds and updates their status in hosts.json.
/// Also updates legacy servers.json for backward compat during transition.
/// Emits HostStatusEvent on the event bus for each status change.
pub async fn run_monitoring(
    host_events: Arc<broadcast::Sender<HostStatusEvent>>,
) {
    info!("Host monitoring started (interval: {}s)", MONITOR_INTERVAL_SECS);

    loop {
        if let Err(e) = monitor_all_hosts(&host_events).await {
            error!("Monitoring cycle error: {}", e);
        }
        tokio::time::sleep(std::time::Duration::from_secs(MONITOR_INTERVAL_SECS)).await;
    }
}

async fn monitor_all_hosts(
    host_events: &broadcast::Sender<HostStatusEvent>,
) -> Result<(), String> {
    // Prefer hosts.json, fall back to servers.json
    let (file_path, key) = if tokio::fs::metadata(HOSTS_FILE).await.is_ok() {
        (HOSTS_FILE, "hosts")
    } else {
        (SERVERS_FILE, "servers")
    };

    let content = match tokio::fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let mut data: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let entries = match data.get_mut(key).and_then(|s| s.as_array_mut()) {
        Some(s) => s,
        None => return Ok(()),
    };

    if entries.is_empty() {
        return Ok(());
    }

    // Ping all hosts in parallel
    let mut join_set = tokio::task::JoinSet::new();
    for entry in entries.iter() {
        let host = entry
            .get("host")
            .and_then(|h| h.as_str())
            .unwrap_or("")
            .to_string();
        let id = entry
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        if host.is_empty() || id.is_empty() {
            continue;
        }

        join_set.spawn(async move {
            let (status, latency) = ping_host(&host).await;
            (id, status, latency)
        });
    }

    let mut results: Vec<(String, String, Option<u64>)> = Vec::new();
    while let Some(Ok(result)) = join_set.join_next().await {
        results.push(result);
    }

    let now = chrono::Utc::now().to_rfc3339();

    for (id, new_status, latency) in &results {
        if let Some(entry) = entries.iter_mut().find(|s| {
            s.get("id").and_then(|i| i.as_str()) == Some(id)
        }) {
            let old_status = entry
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");

            if old_status != new_status {
                debug!("{}: {} -> {}", id, old_status, new_status);
            }

            entry["status"] = serde_json::json!(new_status);
            entry["latency"] = serde_json::json!(latency.unwrap_or(0));
            entry["lastSeen"] = serde_json::json!(&now);

            // Emit host status event
            let _ = host_events.send(HostStatusEvent {
                host_id: id.clone(),
                status: new_status.clone(),
                latency_ms: *latency,
            });
        }
    }

    if !results.is_empty() {
        let content = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        let tmp = format!("{}.tmp", file_path);
        tokio::fs::write(&tmp, &content)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, file_path)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Ping a host and return (status, latency_ms).
async fn ping_host(host: &str) -> (String, Option<u64>) {
    let start = std::time::Instant::now();
    let output = tokio::process::Command::new("ping")
        .args(["-c", "1", "-W", "5", host])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let latency = start.elapsed().as_millis() as u64;
            ("online".to_string(), Some(latency))
        }
        _ => ("offline".to_string(), None),
    }
}
