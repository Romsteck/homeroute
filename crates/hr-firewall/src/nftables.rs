//! nftables CLI interaction for IPv6 firewall rules.

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::config::{FirewallConfig, FirewallRule};

const TABLE_NAME: &str = "homeroute_fw";

/// Build the complete nftables ruleset string.
pub fn build_ruleset(config: &FirewallConfig, lan_prefix: &str) -> String {
    let mut r = String::with_capacity(1024);

    // Atomic flush + recreate
    r.push_str(&format!("table ip6 {} {{\n", TABLE_NAME));
    r.push_str("  chain forward {\n");
    r.push_str("    type filter hook forward priority 0; policy accept;\n");
    r.push('\n');

    // Allow established/related connections
    r.push_str("    ct state established,related accept\n");
    r.push('\n');

    // ICMPv6 must always be allowed (NDP, PMTUD, etc.)
    r.push_str("    meta l4proto icmpv6 accept\n");
    r.push('\n');

    // Allow all outbound from LAN
    if !config.lan_interface.is_empty() {
        r.push_str(&format!(
            "    iifname \"{}\" accept\n",
            config.lan_interface
        ));
        r.push('\n');
    }

    // User-defined allow rules for inbound to LAN
    for rule in &config.allow_rules {
        if !rule.enabled {
            continue;
        }
        if let Some(nft_rule) = build_allow_rule(rule, &config.lan_interface, lan_prefix) {
            r.push_str(&format!("    {} accept\n", nft_rule));
        }
    }

    // Default: drop/reject unsolicited inbound to LAN prefix
    if !config.lan_interface.is_empty() && !lan_prefix.is_empty() {
        r.push_str(&format!(
            "    oifname \"{}\" ip6 daddr {} {}\n",
            config.lan_interface, lan_prefix, config.default_inbound_policy
        ));
    }

    r.push_str("  }\n");
    r.push_str("}\n");

    r
}

/// Build a single nftables allow rule from a FirewallRule.
fn build_allow_rule(rule: &FirewallRule, lan_iface: &str, lan_prefix: &str) -> Option<String> {
    let mut parts = Vec::new();

    // Direction: inbound to LAN
    if !lan_iface.is_empty() {
        parts.push(format!("oifname \"{}\"", lan_iface));
    }

    // Source address filter
    if !rule.source_address.is_empty() {
        parts.push(format!("ip6 saddr {}", rule.source_address));
    }

    // Destination address filter
    if !rule.dest_address.is_empty() {
        parts.push(format!("ip6 daddr {}", rule.dest_address));
    } else if !lan_prefix.is_empty() {
        parts.push(format!("ip6 daddr {}", lan_prefix));
    }

    // Protocol + port
    match rule.protocol.as_str() {
        "tcp" => {
            parts.push("meta l4proto tcp".into());
            if rule.dest_port > 0 {
                if rule.dest_port_end > rule.dest_port {
                    parts.push(format!("tcp dport {}-{}", rule.dest_port, rule.dest_port_end));
                } else {
                    parts.push(format!("tcp dport {}", rule.dest_port));
                }
            }
        }
        "udp" => {
            parts.push("meta l4proto udp".into());
            if rule.dest_port > 0 {
                if rule.dest_port_end > rule.dest_port {
                    parts.push(format!("udp dport {}-{}", rule.dest_port, rule.dest_port_end));
                } else {
                    parts.push(format!("udp dport {}", rule.dest_port));
                }
            }
        }
        "icmpv6" => {
            parts.push("meta l4proto icmpv6".into());
        }
        "any" | "" => {}
        other => {
            warn!("Unknown protocol '{}' in firewall rule {}", other, rule.id);
            return None;
        }
    }

    Some(parts.join(" "))
}

/// Apply the full firewall ruleset atomically via `nft -f -`.
pub async fn apply_ruleset(config: &FirewallConfig, lan_prefix: &str) -> Result<()> {
    let ruleset = build_ruleset(config, lan_prefix);
    debug!("Applying nftables ruleset:\n{}", ruleset);

    // First ensure the table exists (create if not, flush if it does)
    let preamble = format!(
        "flush table ip6 {table} 2>/dev/null; \
         nft -f - <<'NFTEOF'\ntable ip6 {table} {{ }}\nNFTEOF",
        table = TABLE_NAME
    );
    // Use a simpler approach: delete + recreate
    let full_ruleset = format!("#!/usr/sbin/nft -f\n\nflush ruleset ip6\n\n{}", ruleset);

    let mut child = tokio::process::Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn nft")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(full_ruleset.as_bytes()).await?;
        // Drop stdin to close it
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nft failed: {}", stderr);
    }

    info!("Applied IPv6 firewall ruleset for prefix {}", lan_prefix);
    Ok(())
}

/// Remove all firewall rules.
pub async fn flush_rules() -> Result<()> {
    let output = tokio::process::Command::new("nft")
        .args(["delete", "table", "ip6", TABLE_NAME])
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        // Table might not exist, which is fine
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("No such file or directory") && !stderr.contains("does not exist") {
            warn!("Failed to flush nftables: {}", stderr);
        }
    } else {
        info!("Flushed IPv6 firewall rules");
    }

    Ok(())
}

/// Get the current nftables ruleset for display.
pub async fn get_current_rules() -> Result<String> {
    let output = tokio::process::Command::new("nft")
        .args(["list", "table", "ip6", TABLE_NAME])
        .output()
        .await
        .context("Failed to run nft list")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("(no rules: {})", stderr.trim()))
    }
}
