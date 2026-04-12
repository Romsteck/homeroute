use axum::{
    Json, Router,
    extract::Query,
    routing::{get, post},
};
use chrono::Timelike;
use hr_common::events::{CoreMetrics, EnergyMetricsEvent, EventBus};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::state::ApiState;

const ENERGY_SCHEDULE_PATH: &str = "/var/lib/server-dashboard/energy-schedule.json";
const ENERGY_MODE_DIR: &str = "/var/lib/server-dashboard";

pub struct EnergyHost {
    pub id: &'static str,
    pub name: &'static str,
    pub ip: Option<&'static str>,
}

pub const HOSTS: &[EnergyHost] = &[
    EnergyHost {
        id: "medion",
        name: "Medion (Routeur)",
        ip: None,
    },
    EnergyHost {
        id: "cloudmaster",
        name: "CloudMaster",
        ip: Some("10.0.0.10"),
    },
];

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/hosts", get(list_hosts))
        .route("/cpu", get(cpu_info))
        .route("/mode", get(current_mode))
        .route("/mode/{mode}", post(apply_mode))
        .route("/governor/all", post(set_governor_all))
        .route("/governor/{core}", post(set_governor_core))
        .route("/schedule", get(get_schedule).post(save_schedule))
        .route("/benchmark", get(benchmark_status))
        .route("/benchmark/start", post(start_benchmark))
        .route("/benchmark/stop", post(stop_benchmark))
}

// ─── Host query param ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HostQuery {
    #[serde(default = "default_host")]
    host: String,
}

fn default_host() -> String {
    "medion".to_string()
}

fn find_host(id: &str) -> Option<&'static EnergyHost> {
    HOSTS.iter().find(|h| h.id == id)
}

// ─── SSH helper ─────────────────────────────────────────────────────────────

async fn ssh_cmd(ip: &str, cmd: &str) -> Option<String> {
    let fut = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=3",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            &format!("root@{}", ip),
            cmd,
        ])
        .output();

    let output = match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
        Ok(Ok(o)) => o,
        Ok(Err(_)) | Err(_) => return None,
    };

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

// ─── Mode storage ───────────────────────────────────────────────────────────

async fn store_mode(host_id: &str, mode: &str) {
    let path = format!("{}/energy-mode-{}.txt", ENERGY_MODE_DIR, host_id);
    let _ = tokio::fs::write(&path, mode).await;
}

async fn read_stored_mode(host_id: &str) -> String {
    let path = format!("{}/energy-mode-{}.txt", ENERGY_MODE_DIR, host_id);
    tokio::fs::read_to_string(&path)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "economy".to_string())
}

// ─── Local sysfs reading ────────────────────────────────────────────────────

pub async fn read_local_temperature() -> Option<f64> {
    let hwmon_dir = "/sys/class/hwmon";
    let mut entries = tokio::fs::read_dir(hwmon_dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name_path = entry.path().join("name");
        if let Ok(name) = tokio::fs::read_to_string(&name_path).await {
            let name = name.trim();
            if name == "k10temp" || name == "coretemp" || name == "zenpower" {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(val) = tokio::fs::read_to_string(&temp_path).await {
                    if let Ok(millideg) = val.trim().parse::<f64>() {
                        return Some(millideg / 1000.0);
                    }
                }
            }
        }
    }
    None
}

pub async fn read_local_frequency() -> (f64, Option<f64>, Option<f64>, usize) {
    let mut freqs = Vec::new();
    let mut min_freq = None;
    let mut max_freq = None;

    for i in 0..128 {
        let cur = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
        match tokio::fs::read_to_string(&cur).await {
            Ok(val) => {
                if let Ok(khz) = val.trim().parse::<u64>() {
                    freqs.push(khz);
                }
            }
            Err(_) => break,
        }
        if i == 0 {
            let min_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
            let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_max_freq", i);
            min_freq = tokio::fs::read_to_string(&min_path)
                .await
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok());
            max_freq = tokio::fs::read_to_string(&max_path)
                .await
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok());
        }
    }

    let avg = if freqs.is_empty() {
        0
    } else {
        freqs.iter().sum::<u64>() / freqs.len() as u64
    };

    (
        avg as f64 / 1_000_000.0,
        min_freq.map(|f| f as f64 / 1_000_000.0),
        max_freq.map(|f| f as f64 / 1_000_000.0),
        freqs.len(),
    )
}

pub async fn read_local_governor() -> String {
    tokio::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

async fn read_local_model() -> String {
    if let Ok(content) = tokio::fs::read_to_string("/proc/cpuinfo").await {
        for line in content.lines() {
            if line.starts_with("model name") {
                if let Some((_k, v)) = line.split_once(':') {
                    return v.trim().to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}

async fn read_local_available_governors() -> Vec<String> {
    tokio::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors")
        .await
        .map(|s| s.trim().split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

pub async fn read_local_per_core() -> Vec<CoreMetrics> {
    let mut cores = Vec::new();
    for i in 0..128u32 {
        let gov_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
        let governor = match tokio::fs::read_to_string(&gov_path).await {
            Ok(s) => s.trim().to_string(),
            Err(_) => break,
        };
        let cur_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
        let frequency_mhz = tokio::fs::read_to_string(&cur_path)
            .await
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|khz| (khz / 1000) as u32)
            .unwrap_or(0);
        let min_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
        let min_freq_mhz = tokio::fs::read_to_string(&min_path)
            .await
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|khz| (khz / 1000) as u32)
            .unwrap_or(0);
        let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_max_freq", i);
        let max_freq_mhz = tokio::fs::read_to_string(&max_path)
            .await
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|khz| (khz / 1000) as u32)
            .unwrap_or(0);

        cores.push(CoreMetrics {
            core_id: i,
            frequency_mhz,
            governor,
            min_freq_mhz,
            max_freq_mhz,
        });
    }
    cores
}

pub async fn read_remote_per_core(ip: &str) -> Option<Vec<CoreMetrics>> {
    let script = concat!(
        "for cpu in /sys/devices/system/cpu/cpu*/cpufreq; do ",
        "id=$(basename $(dirname \"$cpu\") | sed 's/cpu//'); ",
        "freq=$(cat \"$cpu/scaling_cur_freq\" 2>/dev/null); ",
        "gov=$(cat \"$cpu/scaling_governor\" 2>/dev/null); ",
        "min=$(cat \"$cpu/scaling_min_freq\" 2>/dev/null); ",
        "max=$(cat \"$cpu/cpuinfo_max_freq\" 2>/dev/null); ",
        "echo \"CORE:$id:$freq:$gov:$min:$max\"; ",
        "done"
    );
    let output = ssh_cmd(ip, script).await?;
    let mut cores = Vec::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("CORE:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() >= 5 {
                let core_id = parts[0].parse::<u32>().unwrap_or(0);
                let freq_khz = parts[1].parse::<u64>().unwrap_or(0);
                let governor = parts[2].to_string();
                let min_khz = parts[3].parse::<u64>().unwrap_or(0);
                let max_khz = parts[4].parse::<u64>().unwrap_or(0);
                cores.push(CoreMetrics {
                    core_id,
                    frequency_mhz: (freq_khz / 1000) as u32,
                    governor,
                    min_freq_mhz: (min_khz / 1000) as u32,
                    max_freq_mhz: (max_khz / 1000) as u32,
                });
            }
        }
    }
    cores.sort_by_key(|c| c.core_id);
    if cores.is_empty() { None } else { Some(cores) }
}

pub fn parse_cpu_stat(line: &str) -> Option<(u64, u64)> {
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let idle = parts[3];
    let total: u64 = parts.iter().sum();
    Some((idle, total))
}

// ─── Remote host reading ────────────────────────────────────────────────────

pub struct RemoteMetrics {
    pub cpu_stat_line: String,
    pub temperature: Option<f64>,
    pub governor: String,
    pub frequency_khz: u64,
    pub min_freq_khz: Option<u64>,
    pub max_freq_khz: Option<u64>,
    pub cores: usize,
    pub model: String,
}

pub async fn read_remote_metrics(ip: &str) -> Option<RemoteMetrics> {
    let script = concat!(
        "echo \"STAT:$(head -1 /proc/stat)\";",
        "TEMP=''; for hw in /sys/class/hwmon/hwmon*; do ",
        "n=$(cat \"$hw/name\" 2>/dev/null); ",
        "if [ \"$n\" = coretemp ] || [ \"$n\" = k10temp ] || [ \"$n\" = zenpower ]; then ",
        "TEMP=$(cat \"$hw/temp1_input\" 2>/dev/null); break; fi; done; ",
        "echo \"TEMP:$TEMP\";",
        "echo \"GOV:$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null)\";",
        "echo \"FREQ:$(awk '{s+=$1;n++} END{if(n>0) printf \"%.0f\", s/n}' /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq 2>/dev/null)\";",
        "echo \"MIN:$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq 2>/dev/null)\";",
        "echo \"MAX:$(cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq 2>/dev/null)\";",
        "echo \"CORES:$(ls -d /sys/devices/system/cpu/cpu*/cpufreq 2>/dev/null | wc -l)\";",
        "echo \"MODEL:$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2 | xargs)\""
    );

    let output = ssh_cmd(ip, script).await?;
    let mut stat = String::new();
    let mut temp = None;
    let mut gov = String::from("unknown");
    let mut freq: u64 = 0;
    let mut min_f = None;
    let mut max_f = None;
    let mut cores: usize = 0;
    let mut model = String::from("Unknown");

    for line in output.lines() {
        if let Some(v) = line.strip_prefix("STAT:") {
            stat = v.to_string();
        } else if let Some(v) = line.strip_prefix("TEMP:") {
            temp = v.trim().parse::<f64>().ok().map(|t| t / 1000.0);
        } else if let Some(v) = line.strip_prefix("GOV:") {
            if !v.trim().is_empty() {
                gov = v.trim().to_string();
            }
        } else if let Some(v) = line.strip_prefix("FREQ:") {
            freq = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("MIN:") {
            min_f = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("MAX:") {
            max_f = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("CORES:") {
            cores = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("MODEL:") {
            if !v.trim().is_empty() {
                model = v.trim().to_string();
            }
        }
    }

    Some(RemoteMetrics {
        cpu_stat_line: stat,
        temperature: temp,
        governor: gov,
        frequency_khz: freq,
        min_freq_khz: min_f,
        max_freq_khz: max_f,
        cores,
        model,
    })
}

// ─── Route handlers ─────────────────────────────────────────────────────────

async fn list_hosts() -> Json<Value> {
    let hosts: Vec<Value> = HOSTS
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "name": h.name,
                "local": h.ip.is_none(),
            })
        })
        .collect();
    Json(json!({ "success": true, "hosts": hosts }))
}

async fn cpu_info(Query(q): Query<HostQuery>) -> Json<Value> {
    let host = match find_host(&q.host) {
        Some(h) => h,
        None => return Json(json!({"success": false, "error": "Host inconnu"})),
    };

    if let Some(ip) = host.ip {
        // Remote host
        match read_remote_metrics(ip).await {
            Some(m) => Json(json!({
                "success": true,
                "temperature": m.temperature,
                "frequency": {
                    "current": m.frequency_khz as f64 / 1_000_000.0,
                    "min": m.min_freq_khz.map(|f| f as f64 / 1_000_000.0),
                    "max": m.max_freq_khz.map(|f| f as f64 / 1_000_000.0),
                    "cores": m.cores
                },
                "model": m.model
            })),
            None => Json(json!({"success": false, "error": "Host injoignable"})),
        }
    } else {
        // Local
        let temp = read_local_temperature().await;
        let (cur, min, max, cores) = read_local_frequency().await;
        let model = read_local_model().await;
        Json(json!({
            "success": true,
            "temperature": temp,
            "frequency": { "current": cur, "min": min, "max": max, "cores": cores },
            "model": model
        }))
    }
}

async fn current_mode(Query(q): Query<HostQuery>) -> Json<Value> {
    let host = match find_host(&q.host) {
        Some(h) => h,
        None => return Json(json!({"success": false, "error": "Host inconnu"})),
    };

    let governor = if let Some(ip) = host.ip {
        ssh_cmd(
            ip,
            "cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null",
        )
        .await
        .unwrap_or_default()
    } else {
        read_local_governor().await
    };

    let mode = read_stored_mode(host.id).await;

    Json(json!({"success": true, "mode": mode, "governor": governor}))
}

async fn apply_mode(
    axum::extract::Path(mode): axum::extract::Path<String>,
    Query(q): Query<HostQuery>,
) -> Json<Value> {
    let host = match find_host(&q.host) {
        Some(h) => h,
        None => return Json(json!({"success": false, "error": "Host inconnu"})),
    };

    match apply_mode_on_host(host, &mode).await {
        Ok(()) => {
            store_mode(host.id, &mode).await;
            Json(json!({"success": true, "mode": mode}))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

async fn apply_mode_on_host(host: &EnergyHost, mode: &str) -> Result<(), String> {
    let (governor, epp, max_pct) = match mode {
        "economy" => ("powersave", Some("power"), 60u32),
        "auto" => {
            // Auto: try schedutil, then ondemand, then powersave+balance_power
            let available = if let Some(ip) = host.ip {
                ssh_cmd(ip, "cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors 2>/dev/null")
                    .await
                    .unwrap_or_default()
            } else {
                read_local_available_governors().await.join(" ")
            };

            if available.contains("schedutil") {
                ("schedutil", None, 100u32)
            } else if available.contains("ondemand") {
                ("ondemand", None, 100u32)
            } else {
                ("powersave", Some("balance_power"), 100u32)
            }
        }
        "performance" => ("performance", Some("performance"), 100),
        _ => return Err("Mode inconnu".to_string()),
    };

    if let Some(ip) = host.ip {
        // Remote via SSH
        let mut script = format!(
            "for cpu in /sys/devices/system/cpu/cpu*/cpufreq; do echo {} > \"$cpu/scaling_governor\" 2>/dev/null;",
            governor
        );
        if let Some(epp_val) = epp {
            script.push_str(&format!(
                " [ -f \"$cpu/energy_performance_preference\" ] && echo {} > \"$cpu/energy_performance_preference\" 2>/dev/null;",
                epp_val
            ));
        }
        if max_pct < 100 {
            script.push_str(&format!(
                " MAX=$(cat \"$cpu/cpuinfo_max_freq\" 2>/dev/null); if [ -n \"$MAX\" ]; then echo $(( MAX * {} / 100 )) > \"$cpu/scaling_max_freq\" 2>/dev/null; fi;",
                max_pct
            ));
        } else {
            script.push_str(
                " MAX=$(cat \"$cpu/cpuinfo_max_freq\" 2>/dev/null); if [ -n \"$MAX\" ]; then echo $MAX > \"$cpu/scaling_max_freq\" 2>/dev/null; fi;",
            );
        }
        script.push_str(" done");

        ssh_cmd(ip, &script)
            .await
            .ok_or_else(|| "SSH vers le host a echoue".to_string())?;
    } else {
        // Local sysfs writes
        for i in 0..128 {
            let gov_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            if tokio::fs::metadata(&gov_path).await.is_err() {
                break;
            }
            let _ = tokio::fs::write(&gov_path, governor).await;

            if let Some(epp_val) = epp {
                let epp_path = format!(
                    "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
                    i
                );
                if tokio::fs::metadata(&epp_path).await.is_ok() {
                    let _ = tokio::fs::write(&epp_path, epp_val).await;
                }
            }

            let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_max_freq", i);
            if let Ok(max_str) = tokio::fs::read_to_string(&max_path).await {
                if let Ok(max_khz) = max_str.trim().parse::<u64>() {
                    let target = max_khz * max_pct as u64 / 100;
                    let scaling_max =
                        format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);
                    let _ = tokio::fs::write(&scaling_max, target.to_string()).await;
                }
            }
        }
    }

    Ok(())
}

// ─── Per-core governor ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct GovernorBody {
    governor: String,
}

const ALLOWED_GOVERNORS: &[&str] = &["powersave", "performance", "schedutil"];

async fn set_governor_core(
    axum::extract::Path(core): axum::extract::Path<u32>,
    Query(q): Query<HostQuery>,
    Json(body): Json<GovernorBody>,
) -> Json<Value> {
    let host = match find_host(&q.host) {
        Some(h) => h,
        None => return Json(json!({"success": false, "error": "Host inconnu"})),
    };

    if !ALLOWED_GOVERNORS.contains(&body.governor.as_str()) {
        return Json(json!({"success": false, "error": "Governor invalide"}));
    }

    let result = if let Some(ip) = host.ip {
        let script = format!(
            "echo {} > /sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor 2>/dev/null && echo OK",
            body.governor, core
        );
        ssh_cmd(ip, &script).await
    } else {
        let path = format!(
            "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor",
            core
        );
        match tokio::fs::write(&path, &body.governor).await {
            Ok(_) => Some("OK".to_string()),
            Err(e) => {
                return Json(json!({"success": false, "error": e.to_string()}));
            }
        }
    };

    match result {
        Some(out) if out.contains("OK") => {
            info!(
                "Governor set to {} on core {} of {}",
                body.governor, core, host.id
            );
            Json(json!({"success": true, "core": core, "governor": body.governor}))
        }
        _ => Json(json!({"success": false, "error": "Echec ecriture governor"})),
    }
}

async fn set_governor_all(
    Query(q): Query<HostQuery>,
    Json(body): Json<GovernorBody>,
) -> Json<Value> {
    let host = match find_host(&q.host) {
        Some(h) => h,
        None => return Json(json!({"success": false, "error": "Host inconnu"})),
    };

    if !ALLOWED_GOVERNORS.contains(&body.governor.as_str()) {
        return Json(json!({"success": false, "error": "Governor invalide"}));
    }

    if let Some(ip) = host.ip {
        let script = format!(
            "for cpu in /sys/devices/system/cpu/cpu*/cpufreq; do echo {} > \"$cpu/scaling_governor\" 2>/dev/null; done && echo OK",
            body.governor
        );
        match ssh_cmd(ip, &script).await {
            Some(out) if out.contains("OK") => {
                info!(
                    "Governor set to {} on all cores of {}",
                    body.governor, host.id
                );
                Json(json!({"success": true, "governor": body.governor}))
            }
            _ => Json(json!({"success": false, "error": "SSH failed"})),
        }
    } else {
        for i in 0..128u32 {
            let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            if tokio::fs::metadata(&path).await.is_err() {
                break;
            }
            let _ = tokio::fs::write(&path, &body.governor).await;
        }
        info!(
            "Governor set to {} on all cores of {}",
            body.governor, host.id
        );
        Json(json!({"success": true, "governor": body.governor}))
    }
}

// ─── Schedule ───────────────────────────────────────────────────────────────

async fn get_schedule() -> Json<Value> {
    let default = json!({"enabled": false, "nightStart": "23:00", "nightEnd": "07:00"});
    match tokio::fs::read_to_string(ENERGY_SCHEDULE_PATH).await {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(config) => Json(json!({"success": true, "config": config})),
            Err(_) => Json(json!({"success": true, "config": default})),
        },
        Err(_) => Json(json!({"success": true, "config": default})),
    }
}

async fn save_schedule(Json(body): Json<Value>) -> Json<Value> {
    match serde_json::to_string_pretty(&body) {
        Ok(content) => {
            if let Err(e) = tokio::fs::write(ENERGY_SCHEDULE_PATH, &content).await {
                return Json(json!({"success": false, "error": e.to_string()}));
            }
            Json(json!({"success": true}))
        }
        Err(e) => Json(json!({"success": false, "error": e.to_string()})),
    }
}

// ─── Benchmark (local only) ─────────────────────────────────────────────────

async fn benchmark_status() -> Json<Value> {
    let output = tokio::process::Command::new("pgrep")
        .args(["-f", "stress-ng|yes"])
        .output()
        .await;
    let running = output.map(|o| o.status.success()).unwrap_or(false);
    Json(json!({"success": true, "running": running}))
}

#[derive(Deserialize)]
struct BenchmarkRequest {
    #[serde(default = "default_duration")]
    duration: u64,
}

fn default_duration() -> u64 {
    60
}

async fn start_benchmark(Json(body): Json<BenchmarkRequest>) -> Json<Value> {
    let duration = body.duration.min(600);
    let result = tokio::process::Command::new("stress-ng")
        .args(["--cpu", "0", "--timeout", &format!("{}s", duration)])
        .spawn();

    match result {
        Ok(_) => Json(json!({"success": true, "tool": "stress-ng", "duration": duration})),
        Err(_) => {
            let num = num_cpus().await;
            for _ in 0..num {
                let _ = tokio::process::Command::new("sh")
                    .args(["-c", &format!("timeout {} yes > /dev/null", duration)])
                    .spawn();
            }
            Json(json!({"success": true, "tool": "yes", "duration": duration, "cores": num}))
        }
    }
}

async fn num_cpus() -> usize {
    if let Ok(content) = tokio::fs::read_to_string("/proc/cpuinfo").await {
        content
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count()
    } else {
        1
    }
}

async fn stop_benchmark() -> Json<Value> {
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "stress-ng"])
        .output()
        .await;
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "yes"])
        .output()
        .await;
    Json(json!({"success": true}))
}

// ─── Background: energy metrics poller ──────────────────────────────────────

pub async fn energy_metrics_poller(events: Arc<EventBus>) {
    let mut prev_stats: HashMap<String, (u64, u64)> = HashMap::new();
    let mut cached_models: HashMap<String, String> = HashMap::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));

    loop {
        interval.tick().await;

        // Local host
        {
            let host = &HOSTS[0]; // medion
            let temp = read_local_temperature().await;
            let (freq, min, max, cores) = read_local_frequency().await;
            let governor = read_local_governor().await;
            let mode = read_stored_mode(host.id).await;

            // CPU usage from /proc/stat
            let cpu_percent = if let Ok(content) = tokio::fs::read_to_string("/proc/stat").await {
                if let Some(line) = content.lines().next() {
                    if let Some((idle, total)) = parse_cpu_stat(line) {
                        let key = host.id.to_string();
                        let pct = if let Some(&(prev_idle, prev_total)) = prev_stats.get(&key) {
                            let di = idle.saturating_sub(prev_idle) as f64;
                            let dt = total.saturating_sub(prev_total) as f64;
                            if dt > 0.0 {
                                ((1.0 - di / dt) * 1000.0).round() / 10.0
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        prev_stats.insert(key, (idle, total));
                        pct
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let model = if let Some(m) = cached_models.get(host.id) {
                m.clone()
            } else {
                let m = read_local_model().await;
                cached_models.insert(host.id.to_string(), m.clone());
                m
            };

            let per_core = {
                let cores = read_local_per_core().await;
                if cores.is_empty() { None } else { Some(cores) }
            };

            let _ = events.energy_metrics.send(EnergyMetricsEvent {
                host_id: host.id.to_string(),
                host_name: host.name.to_string(),
                online: true,
                temperature: temp,
                cpu_percent: cpu_percent as f32,
                frequency_ghz: freq,
                frequency_min_ghz: min,
                frequency_max_ghz: max,
                governor,
                mode,
                cores,
                model,
                per_core,
            });
        }

        // Remote hosts
        for host in HOSTS.iter().skip(1) {
            if let Some(ip) = host.ip {
                match read_remote_metrics(ip).await {
                    Some(m) => {
                        let key = host.id.to_string();
                        let cpu_percent = if let Some((idle, total)) =
                            parse_cpu_stat(&m.cpu_stat_line)
                        {
                            let pct = if let Some(&(prev_idle, prev_total)) = prev_stats.get(&key) {
                                let di = idle.saturating_sub(prev_idle) as f64;
                                let dt = total.saturating_sub(prev_total) as f64;
                                if dt > 0.0 {
                                    ((1.0 - di / dt) * 1000.0).round() / 10.0
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            };
                            prev_stats.insert(key, (idle, total));
                            pct
                        } else {
                            0.0
                        };

                        let model = if let Some(cached) = cached_models.get(host.id) {
                            cached.clone()
                        } else {
                            cached_models.insert(host.id.to_string(), m.model.clone());
                            m.model.clone()
                        };

                        let mode = read_stored_mode(host.id).await;
                        let per_core = read_remote_per_core(ip).await;

                        let _ = events.energy_metrics.send(EnergyMetricsEvent {
                            host_id: host.id.to_string(),
                            host_name: host.name.to_string(),
                            online: true,
                            temperature: m.temperature,
                            cpu_percent: cpu_percent as f32,
                            frequency_ghz: m.frequency_khz as f64 / 1_000_000.0,
                            frequency_min_ghz: m.min_freq_khz.map(|f| f as f64 / 1_000_000.0),
                            frequency_max_ghz: m.max_freq_khz.map(|f| f as f64 / 1_000_000.0),
                            governor: m.governor,
                            mode,
                            cores: m.cores,
                            model,
                            per_core,
                        });
                    }
                    None => {
                        // Host unreachable — send offline event so frontend knows
                        let mode = read_stored_mode(host.id).await;
                        let _ = events.energy_metrics.send(EnergyMetricsEvent {
                            host_id: host.id.to_string(),
                            host_name: host.name.to_string(),
                            online: false,
                            temperature: None,
                            cpu_percent: 0.0,
                            frequency_ghz: 0.0,
                            frequency_min_ghz: None,
                            frequency_max_ghz: None,
                            governor: String::new(),
                            mode,
                            cores: 0,
                            model: String::new(),
                            per_core: None,
                        });
                        warn!(
                            "Energy metrics: host {} ({}) unreachable via SSH",
                            host.name, ip
                        );
                    }
                }
            }
        }
    }
}

// ─── Background: schedule enforcer ──────────────────────────────────────────

pub async fn energy_schedule_enforcer(_events: Arc<EventBus>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    let mut night_active = false;

    loop {
        interval.tick().await;

        // Read schedule config
        let config = match tokio::fs::read_to_string(ENERGY_SCHEDULE_PATH).await {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(c) => c,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let enabled = config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            if night_active {
                // Schedule was disabled while night was active — restore modes
                info!("Energy schedule disabled, restoring modes");
                for host in HOSTS {
                    let mode = read_stored_mode(host.id).await;
                    if mode != "economy" {
                        let _ = apply_mode_on_host(host, &mode).await;
                    }
                }
                night_active = false;
            }
            continue;
        }

        let night_start = config
            .get("nightStart")
            .and_then(|v| v.as_str())
            .unwrap_or("23:00");
        let night_end = config
            .get("nightEnd")
            .and_then(|v| v.as_str())
            .unwrap_or("07:00");

        let now = chrono::Local::now();
        let current_minutes = now.hour() * 60 + now.minute();

        let start_minutes = parse_time_minutes(night_start).unwrap_or(23 * 60);
        let end_minutes = parse_time_minutes(night_end).unwrap_or(7 * 60);

        let in_night = if start_minutes <= end_minutes {
            // Same day (e.g., 01:00 - 06:00)
            current_minutes >= start_minutes && current_minutes < end_minutes
        } else {
            // Crosses midnight (e.g., 23:00 - 07:00)
            current_minutes >= start_minutes || current_minutes < end_minutes
        };

        if in_night && !night_active {
            info!(
                "Energy night mode activating ({}–{})",
                night_start, night_end
            );
            for host in HOSTS {
                let _ = apply_mode_on_host(host, "economy").await;
            }
            night_active = true;
        } else if !in_night && night_active {
            info!("Energy night mode ending, restoring modes");
            for host in HOSTS {
                let mode = read_stored_mode(host.id).await;
                let _ = apply_mode_on_host(host, &mode).await;
            }
            night_active = false;
        }
    }
}

fn parse_time_minutes(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        let h = parts[0].parse::<u32>().ok()?;
        let m = parts[1].parse::<u32>().ok()?;
        Some(h * 60 + m)
    } else {
        None
    }
}
