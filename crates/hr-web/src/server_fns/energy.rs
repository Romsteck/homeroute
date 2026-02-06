use leptos::prelude::*;

use crate::types::EnergyPageData;

#[server]
pub async fn get_energy_data() -> Result<EnergyPageData, ServerFnError> {
    // CPU model
    let cpu_model = tokio::fs::read_to_string("/proc/cpuinfo")
        .await
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "CPU".to_string());

    // Temperature
    let temperature = read_temperature().await;

    // Frequency
    let freq_current = read_sysfs_f64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .await
        .map(|f| f / 1_000_000.0);
    let freq_min = read_sysfs_f64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq")
        .await
        .map(|f| f / 1_000_000.0);
    let freq_max = read_sysfs_f64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .await
        .map(|f| f / 1_000_000.0);

    // CPU usage
    let cpu_usage = read_cpu_usage().await;

    // Current energy mode
    let governor = tokio::fs::read_to_string(
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
    )
    .await
    .unwrap_or_default()
    .trim()
    .to_string();

    let current_mode = match governor.as_str() {
        "powersave" => "economy",
        "schedutil" | "ondemand" | "conservative" => "auto",
        "performance" => "performance",
        _ => "unknown",
    }
    .to_string();

    // Schedule
    let schedule_json =
        tokio::fs::read_to_string("/var/lib/server-dashboard/energy-schedule.json")
            .await
            .unwrap_or_else(|_| "{}".to_string());
    let schedule: serde_json::Value =
        serde_json::from_str(&schedule_json).unwrap_or(serde_json::json!({}));

    let schedule_enabled = schedule
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let schedule_night_start = schedule
        .get("nightStart")
        .and_then(|v| v.as_str())
        .unwrap_or("00:00")
        .to_string();
    let schedule_night_end = schedule
        .get("nightEnd")
        .and_then(|v| v.as_str())
        .unwrap_or("08:00")
        .to_string();

    // Auto-select
    let autoselect_json =
        tokio::fs::read_to_string("/var/lib/server-dashboard/energy-autoselect.json")
            .await
            .unwrap_or_else(|_| "{}".to_string());
    let autoselect: serde_json::Value =
        serde_json::from_str(&autoselect_json).unwrap_or(serde_json::json!({}));

    let auto_select_enabled = autoselect
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let auto_select_interface = autoselect
        .get("networkInterface")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(EnergyPageData {
        cpu_model,
        temperature,
        frequency_current: freq_current,
        frequency_min: freq_min,
        frequency_max: freq_max,
        cpu_usage,
        current_mode,
        schedule_enabled,
        schedule_night_start,
        schedule_night_end,
        auto_select_enabled,
        auto_select_interface,
    })
}

#[server]
pub async fn set_energy_mode(mode: String) -> Result<(), ServerFnError> {
    let (governor, epp, max_pct) = match mode.as_str() {
        "economy" => ("powersave", "power", 60u32),
        "auto" => ("powersave", "balance_power", 85),
        "performance" => ("performance", "performance", 100),
        _ => return Err(ServerFnError::new("Mode inconnu")),
    };

    for i in 0..128u32 {
        let gov_path = format!(
            "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor",
            i
        );
        if tokio::fs::metadata(&gov_path).await.is_err() {
            break;
        }
        let _ = tokio::fs::write(&gov_path, governor).await;

        let epp_path = format!(
            "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
            i
        );
        if tokio::fs::metadata(&epp_path).await.is_ok() {
            let _ = tokio::fs::write(&epp_path, epp).await;
        }

        let max_path = format!(
            "/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_max_freq",
            i
        );
        if let Ok(max_str) = tokio::fs::read_to_string(&max_path).await {
            if let Ok(max_khz) = max_str.trim().parse::<u64>() {
                let target = max_khz * max_pct as u64 / 100;
                let scaling_max = format!(
                    "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq",
                    i
                );
                let _ = tokio::fs::write(&scaling_max, target.to_string()).await;
            }
        }
    }

    leptos_axum::redirect("/energy?msg=Mode+appliqu%C3%A9");
    Ok(())
}

#[cfg(feature = "ssr")]
async fn read_sysfs_f64(path: &str) -> Option<f64> {
    tokio::fs::read_to_string(path)
        .await
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
}

#[cfg(feature = "ssr")]
async fn read_temperature() -> Option<f64> {
    // Try hwmon thermal zones
    for i in 0..10 {
        let path = format!("/sys/class/hwmon/hwmon{i}/temp1_input");
        if let Some(temp) = read_sysfs_f64(&path).await {
            return Some(temp / 1000.0);
        }
    }
    // Fallback: thermal zone
    read_sysfs_f64("/sys/class/thermal/thermal_zone0/temp")
        .await
        .map(|t| t / 1000.0)
}

#[cfg(feature = "ssr")]
async fn read_cpu_usage() -> Option<f64> {
    use tokio::time::{sleep, Duration};

    let s1 = tokio::fs::read_to_string("/proc/stat").await.ok()?;
    let (idle1, total1) = parse_cpu_line(&s1)?;
    sleep(Duration::from_millis(200)).await;
    let s2 = tokio::fs::read_to_string("/proc/stat").await.ok()?;
    let (idle2, total2) = parse_cpu_line(&s2)?;

    let idle_delta = idle2 - idle1;
    let total_delta = total2 - total1;
    if total_delta == 0 {
        return Some(0.0);
    }
    Some(((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0)
}

#[cfg(feature = "ssr")]
fn parse_cpu_line(content: &str) -> Option<(u64, u64)> {
    let line = content.lines().next()?;
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1) // skip "cpu"
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let idle = parts[3];
    let total: u64 = parts.iter().sum();
    Some((idle, total))
}
