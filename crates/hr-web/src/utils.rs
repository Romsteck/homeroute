/// Format a large number in compact form: 1000 → "1.0k", 1000000 → "1.0M".
pub fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a Unix timestamp as a human-readable relative expiry.
pub fn format_expiry(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if ts == 0 {
        return "Statique".to_string();
    }
    if ts <= now {
        return "Expiré".to_string();
    }
    let diff = ts - now;
    if diff < 60 {
        format!("{}s", diff)
    } else if diff < 3600 {
        format!("{}min", diff / 60)
    } else if diff < 86400 {
        format!("{}h {}min", diff / 3600, (diff % 3600) / 60)
    } else {
        format!("{}j {}h", diff / 86400, (diff % 86400) / 3600)
    }
}
