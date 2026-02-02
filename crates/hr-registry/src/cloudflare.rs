//! Cloudflare DNS record management (AAAA records).
//! Extracted from hr-api/routes/ddns.rs for reuse by the agent registry.

use serde_json::json;
use tracing::{info, warn};

const CF_API_BASE: &str = "https://api.cloudflare.com/client/v4";

/// Create or update a Cloudflare AAAA record. Returns the record ID.
pub async fn upsert_aaaa_record(
    token: &str,
    zone_id: &str,
    record_name: &str,
    ipv6: &str,
    proxied: bool,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    // List existing AAAA records for this name
    let list_url = format!(
        "{}/zones/{}/dns_records?type=AAAA&name={}",
        CF_API_BASE, zone_id, record_name
    );

    let resp = client
        .get(&list_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    check_cf_errors(&body)?;

    let records = body
        .get("result")
        .and_then(|r| r.as_array())
        .ok_or("Invalid response from Cloudflare")?;

    if let Some(record) = records.first() {
        // Update existing record
        let record_id = record
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or("No record ID")?
            .to_string();

        let update_url = format!(
            "{}/zones/{}/dns_records/{}",
            CF_API_BASE, zone_id, record_id
        );

        let resp = client
            .put(&update_url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&json!({
                "type": "AAAA",
                "name": record_name,
                "content": ipv6,
                "ttl": 120,
                "proxied": proxied
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Cloudflare update error: {}", body));
        }

        info!(record = record_name, ipv6, "Updated Cloudflare AAAA record");
        Ok(record_id)
    } else {
        // Create new record
        let create_url = format!("{}/zones/{}/dns_records", CF_API_BASE, zone_id);

        let resp = client
            .post(&create_url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&json!({
                "type": "AAAA",
                "name": record_name,
                "content": ipv6,
                "ttl": 120,
                "proxied": proxied
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let resp_body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        check_cf_errors(&resp_body)?;

        let record_id = resp_body
            .get("result")
            .and_then(|r| r.get("id"))
            .and_then(|i| i.as_str())
            .ok_or("No record ID in create response")?
            .to_string();

        info!(record = record_name, ipv6, "Created Cloudflare AAAA record");
        Ok(record_id)
    }
}

/// Delete a Cloudflare DNS record by ID.
pub async fn delete_record(token: &str, zone_id: &str, record_id: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/zones/{}/dns_records/{}",
        CF_API_BASE, zone_id, record_id
    );

    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        warn!(record_id, "Failed to delete Cloudflare record: {}", body);
        return Err(format!("Cloudflare delete error: {}", body));
    }

    info!(record_id, "Deleted Cloudflare DNS record");
    Ok(())
}

/// Get the content (IPv6 address) of an existing AAAA record.
pub async fn get_aaaa_record_content(
    token: &str,
    zone_id: &str,
    record_name: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/zones/{}/dns_records?type=AAAA&name={}",
        CF_API_BASE, zone_id, record_name
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    check_cf_errors(&body)?;

    body.get("result")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_str())
        .map(String::from)
        .ok_or_else(|| "Record not found".to_string())
}

fn check_cf_errors(body: &serde_json::Value) -> Result<(), String> {
    if let Some(false) = body.get("success").and_then(|s| s.as_bool()) {
        let errors = body
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "Unknown error".to_string());
        return Err(format!("Cloudflare API: {}", errors));
    }
    Ok(())
}
