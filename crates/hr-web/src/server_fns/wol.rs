use leptos::prelude::*;

use crate::types::{WolData, WolSchedule};
#[cfg(feature = "ssr")]
use crate::types::WolServer;

#[cfg(feature = "ssr")]
const SERVERS_FILE: &str = "/data/servers.json";
#[cfg(feature = "ssr")]
const SCHEDULES_FILE: &str = "/data/wol-schedules.json";

#[server]
pub async fn get_wol_data() -> Result<WolData, ServerFnError> {
    let (servers, schedules) = tokio::join!(fetch_servers(), fetch_schedules());

    Ok(WolData {
        servers,
        schedules,
    })
}

#[cfg(feature = "ssr")]
async fn fetch_servers() -> Vec<WolServer> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .unwrap_or_else(|_| r#"{"servers":[]}"#.to_string());

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    json.get("servers")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    Some(WolServer {
                        id: s.get("id")?.as_str()?.to_string(),
                        name: s.get("name")?.as_str()?.to_string(),
                        host: s.get("host")?.as_str()?.to_string(),
                        mac: s.get("mac").and_then(|m| m.as_str()).map(String::from),
                        groups: s
                            .get("groups")
                            .and_then(|g| g.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(feature = "ssr")]
async fn fetch_schedules() -> Vec<WolSchedule> {
    let content = tokio::fs::read_to_string(SCHEDULES_FILE)
        .await
        .unwrap_or_else(|_| r#"{"schedules":[]}"#.to_string());

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    json.get("schedules")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    Some(WolSchedule {
                        id: s.get("id")?.as_str()?.to_string(),
                        server_id: s.get("serverId")?.as_str()?.to_string(),
                        server_name: s
                            .get("serverName")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string(),
                        action: s.get("action")?.as_str()?.to_string(),
                        cron: s.get("cron")?.as_str()?.to_string(),
                        description: s
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        enabled: s
                            .get("enabled")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false),
                        last_run: s
                            .get("lastRun")
                            .and_then(|l| l.as_str())
                            .map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── Power actions ──────────────────────────────────────────────────

#[server]
pub async fn wake_server(id: String) -> Result<(), ServerFnError> {
    let server = find_server(&id).await?;
    let mac = server["mac"]
        .as_str()
        .ok_or_else(|| ServerFnError::new("Ce serveur n'a pas d'adresse MAC"))?;
    send_wol_packet(mac).await?;
    leptos_axum::redirect("/wol?msg=Signal+WoL+envoy%C3%A9");
    Ok(())
}

#[server]
pub async fn shutdown_server(id: String) -> Result<(), ServerFnError> {
    let server = find_server(&id).await?;
    ssh_command(&server, "poweroff || shutdown -h now").await?;
    leptos_axum::redirect("/wol?msg=Arr%C3%AAt+envoy%C3%A9");
    Ok(())
}

#[server]
pub async fn reboot_server(id: String) -> Result<(), ServerFnError> {
    let server = find_server(&id).await?;
    ssh_command(&server, "reboot").await?;
    leptos_axum::redirect("/wol?msg=Red%C3%A9marrage+envoy%C3%A9");
    Ok(())
}

#[server]
pub async fn bulk_wake(server_ids: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;
    let servers = json["servers"].as_array().ok_or_else(|| ServerFnError::new("Format invalide"))?;

    let ids: Vec<&str> = server_ids.split(',').map(|s| s.trim()).collect();
    for id in &ids {
        if let Some(server) = servers.iter().find(|s| s["id"].as_str() == Some(id)) {
            if let Some(mac) = server["mac"].as_str() {
                let _ = send_wol_packet(mac).await;
            }
        }
    }
    leptos_axum::redirect("/wol?msg=Signaux+WoL+envoy%C3%A9s");
    Ok(())
}

#[cfg(feature = "ssr")]
async fn find_server(id: &str) -> Result<serde_json::Value, ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;
    json["servers"]
        .as_array()
        .and_then(|arr| arr.iter().find(|s| s["id"].as_str() == Some(id)))
        .cloned()
        .ok_or_else(|| ServerFnError::new("Serveur introuvable"))
}

#[cfg(feature = "ssr")]
async fn send_wol_packet(mac: &str) -> Result<(), ServerFnError> {
    let mac_bytes: Vec<u8> = mac
        .split(':')
        .filter_map(|s| u8::from_str_radix(s, 16).ok())
        .collect();
    if mac_bytes.len() != 6 {
        return Err(ServerFnError::new("Adresse MAC invalide"));
    }
    let mut packet = vec![0xFF_u8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    socket
        .set_broadcast(true)
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let _ = socket.send_to(&packet, "255.255.255.255:9").await;
    let _ = socket.send_to(&packet, "10.0.0.255:9").await;
    Ok(())
}

#[cfg(feature = "ssr")]
async fn ssh_command(server: &serde_json::Value, cmd: &str) -> Result<(), ServerFnError> {
    let host = server["host"].as_str().unwrap_or("");
    let port = server["port"].as_u64().unwrap_or(22);
    let username = server["username"].as_str().unwrap_or("root");

    let output = tokio::process::Command::new("ssh")
        .args([
            "-o", "BatchMode=yes",
            "-o", "ConnectTimeout=5",
            "-o", "StrictHostKeyChecking=no",
            "-i", "/data/ssh/id_rsa",
            "-p", &port.to_string(),
            &format!("{username}@{host}"),
            cmd,
        ])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServerFnError::new(format!("SSH : {}", stderr.trim())));
    }
    Ok(())
}

// ── Schedule CRUD ──────────────────────────────────────────────────

#[server]
pub async fn create_wol_schedule(
    server_id: String,
    action: String,
    cron: String,
    description: String,
) -> Result<(), ServerFnError> {
    // Validate server exists and get name
    let server = find_server(&server_id).await?;
    let server_name = server["name"].as_str().unwrap_or("").to_string();

    let content = tokio::fs::read_to_string(SCHEDULES_FILE)
        .await
        .unwrap_or_else(|_| r#"{"schedules":[]}"#.to_string());
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let id = {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    };

    let obj = serde_json::json!({
        "id": id,
        "serverId": server_id,
        "serverName": server_name,
        "action": action,
        "cron": cron,
        "description": description,
        "enabled": true,
        "lastRun": null,
    });

    json["schedules"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format invalide"))?
        .push(obj);

    let tmp = format!("{SCHEDULES_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/wol?msg=Planification+cr%C3%A9%C3%A9e");
    Ok(())
}

#[server]
pub async fn delete_wol_schedule(id: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let arr = json["schedules"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format invalide"))?;

    let before = arr.len();
    arr.retain(|s| s["id"].as_str() != Some(&id));
    if arr.len() == before {
        return Err(ServerFnError::new("Planification introuvable"));
    }

    let tmp = format!("{SCHEDULES_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/wol?msg=Planification+supprim%C3%A9e");
    Ok(())
}

#[server]
pub async fn toggle_wol_schedule(id: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let arr = json["schedules"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format invalide"))?;

    let entry = arr
        .iter_mut()
        .find(|s| s["id"].as_str() == Some(&id))
        .ok_or_else(|| ServerFnError::new("Planification introuvable"))?;

    let current = entry["enabled"].as_bool().unwrap_or(false);
    entry["enabled"] = serde_json::Value::Bool(!current);

    let tmp = format!("{SCHEDULES_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/wol?msg=Planification+modifi%C3%A9e");
    Ok(())
}

#[server]
pub async fn execute_wol_schedule(id: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SCHEDULES_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let schedule = json["schedules"]
        .as_array()
        .and_then(|arr| arr.iter().find(|s| s["id"].as_str() == Some(&id)))
        .ok_or_else(|| ServerFnError::new("Planification introuvable"))?;

    let server_id = schedule["serverId"]
        .as_str()
        .ok_or_else(|| ServerFnError::new("serverId manquant"))?
        .to_string();
    let action = schedule["action"]
        .as_str()
        .ok_or_else(|| ServerFnError::new("action manquante"))?;

    let server = find_server(&server_id).await?;
    match action {
        "wake" => {
            let mac = server["mac"]
                .as_str()
                .ok_or_else(|| ServerFnError::new("Pas d'adresse MAC"))?;
            send_wol_packet(mac).await?;
        }
        "shutdown" => {
            ssh_command(&server, "poweroff || shutdown -h now").await?;
        }
        "reboot" => {
            ssh_command(&server, "reboot").await?;
        }
        _ => return Err(ServerFnError::new(format!("Action inconnue : {action}"))),
    }

    leptos_axum::redirect("/wol?msg=Planification+ex%C3%A9cut%C3%A9e");
    Ok(())
}
