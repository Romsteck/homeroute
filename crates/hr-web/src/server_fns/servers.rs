use leptos::prelude::*;

use crate::types::ServersData;
#[cfg(feature = "ssr")]
use crate::types::ServerEntry;

#[cfg(feature = "ssr")]
const SERVERS_FILE: &str = "/data/servers.json";

#[server]
pub async fn get_servers_data() -> Result<ServersData, ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .unwrap_or_else(|_| r#"{"servers":[]}"#.to_string());

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let servers = json
        .get("servers")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    Some(ServerEntry {
                        id: s.get("id")?.as_str()?.to_string(),
                        name: s.get("name")?.as_str()?.to_string(),
                        host: s.get("host")?.as_str()?.to_string(),
                        port: s.get("port").and_then(|p| p.as_u64()).unwrap_or(22) as u16,
                        username: s
                            .get("username")
                            .and_then(|u| u.as_str())
                            .unwrap_or("root")
                            .to_string(),
                        mac: s.get("mac").and_then(|m| m.as_str()).map(String::from),
                        interface: s
                            .get("interface")
                            .and_then(|i| i.as_str())
                            .map(String::from),
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
        .unwrap_or_default();

    Ok(ServersData { servers })
}

#[server]
pub async fn add_server(
    name: String,
    host: String,
    port: u16,
    username: String,
    mac: String,
    groups: String,
) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .unwrap_or_else(|_| r#"{"servers":[]}"#.to_string());
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

    let mac_opt = if mac.trim().is_empty() {
        None
    } else {
        Some(mac.trim().to_string())
    };
    let groups_vec: Vec<String> = groups
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let server_obj = serde_json::json!({
        "id": id,
        "name": name,
        "host": host,
        "port": port,
        "username": username,
        "mac": mac_opt,
        "groups": groups_vec,
        "status": "unknown",
    });

    json["servers"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format de fichier invalide"))?
        .push(server_obj);

    let tmp = format!("{SERVERS_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/servers?msg=Serveur+ajout%C3%A9");
    Ok(())
}

#[server]
pub async fn update_server(
    id: String,
    name: String,
    host: String,
    port: u16,
    username: String,
    mac: String,
    groups: String,
) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let mac_val = if mac.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(mac.trim().to_string())
    };
    let groups_vec: Vec<String> = groups
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let arr = json["servers"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format de fichier invalide"))?;

    let entry = arr
        .iter_mut()
        .find(|s| s["id"].as_str() == Some(&id))
        .ok_or_else(|| ServerFnError::new("Serveur introuvable"))?;

    entry["name"] = serde_json::Value::String(name);
    entry["host"] = serde_json::Value::String(host);
    entry["port"] = serde_json::json!(port);
    entry["username"] = serde_json::Value::String(username);
    entry["mac"] = mac_val;
    entry["groups"] = serde_json::json!(groups_vec);

    let tmp = format!("{SERVERS_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/servers?msg=Serveur+modifi%C3%A9");
    Ok(())
}

#[server]
pub async fn delete_server(id: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let arr = json["servers"]
        .as_array_mut()
        .ok_or_else(|| ServerFnError::new("Format de fichier invalide"))?;

    let before = arr.len();
    arr.retain(|s| s["id"].as_str() != Some(&id));
    if arr.len() == before {
        return Err(ServerFnError::new("Serveur introuvable"));
    }

    let tmp = format!("{SERVERS_FILE}.tmp");
    tokio::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap())
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    tokio::fs::rename(&tmp, SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    leptos_axum::redirect("/servers?msg=Serveur+supprim%C3%A9");
    Ok(())
}

#[server]
pub async fn test_server_connection(id: String) -> Result<(), ServerFnError> {
    let content = tokio::fs::read_to_string(SERVERS_FILE)
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ServerFnError::new(format!("{e}")))?;

    let server = json["servers"]
        .as_array()
        .and_then(|arr| arr.iter().find(|s| s["id"].as_str() == Some(&id)))
        .ok_or_else(|| ServerFnError::new("Serveur introuvable"))?;

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
            "echo ok",
        ])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("{e}")))?;

    if output.status.success() {
        leptos_axum::redirect("/servers?msg=Connexion+r%C3%A9ussie");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!(
            "Connexion échouée : {}",
            stderr.trim()
        )))
    }
}
