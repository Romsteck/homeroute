//! Synchronise les FQDN des routes du reverse proxy vers le DNS local
//! (hr-netcore) sous l'ownership "hr-edge".
//!
//! Source de vérité = `ProxyState` (qui agrège routes manuelles, app routes,
//! et builtins `proxy.`/`auth.`). À chaque mutation, on signale via `Notify` ;
//! une boucle de fond fait un débounce 500ms puis envoie l'ensemble complet
//! (idempotent) à hr-netcore via `dns_set_managed_records`.
//!
//! Les records utilisateur (`managed_by = None`) côté hr-netcore ne sont jamais
//! écrasés — `replace_managed_records` ne touche qu'aux records owned by "hr-edge".

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use hr_ipc::client::NetcoreClient;
use hr_ipc::types::StaticRecordDto;
use hr_proxy::ProxyState;
use tokio::sync::Notify;
use tokio::time::sleep;
use tracing::{debug, info, warn};

const OWNER: &str = "hr-edge";
const TTL_SECONDS: u32 = 60;
const DEBOUNCE_MS: u64 = 500;
const PERIODIC_SYNC_SECS: u64 = 300;

pub struct DnsRouteSync {
    proxy: Arc<ProxyState>,
    netcore: Arc<NetcoreClient>,
    base_domain: String,
    server_ip: Ipv4Addr,
    notify: Arc<Notify>,
}

impl DnsRouteSync {
    pub fn new(
        proxy: Arc<ProxyState>,
        netcore: Arc<NetcoreClient>,
        base_domain: String,
        server_ip: Ipv4Addr,
    ) -> Arc<Self> {
        Arc::new(Self {
            proxy,
            netcore,
            base_domain,
            server_ip,
            notify: Arc::new(Notify::new()),
        })
    }

    /// Demande un sync. Cheap, idempotent, débouncé côté `run()`.
    pub fn request_sync(&self) {
        self.notify.notify_one();
    }

    /// Boucle de fond. Spawn une fois par démarrage de hr-edge :
    /// `tokio::spawn(sync.clone().run())`.
    pub async fn run(self: Arc<Self>) {
        info!(
            base_domain = %self.base_domain,
            server_ip = %self.server_ip,
            "DnsRouteSync started"
        );

        // Trigger initial sync on startup.
        self.notify.notify_one();

        // Periodic safety net: re-push the full set every PERIODIC_SYNC_SECS.
        // Cheap, idempotent — covers the case where hr-netcore restarted
        // without any route mutation in between.
        let periodic_notify = self.notify.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(PERIODIC_SYNC_SECS)).await;
                periodic_notify.notify_one();
            }
        });

        loop {
            self.notify.notified().await;
            // Debounce: drain any quick successive notifies.
            sleep(Duration::from_millis(DEBOUNCE_MS)).await;

            let records = self.build_record_set();
            let count = records.len();

            match self.netcore.dns_set_managed_records(OWNER, records).await {
                Ok(resp) if resp.ok => {
                    debug!(count = count, "Pushed DNS managed records to hr-netcore");
                }
                Ok(resp) => {
                    warn!(
                        error = ?resp.error,
                        count = count,
                        "hr-netcore rejected dns_set_managed_records"
                    );
                }
                Err(e) => {
                    warn!(error = %e, count = count, "Failed to push DNS managed records");
                }
            }
        }
    }

    /// Calcule l'ensemble des FQDN à publier dans le DNS local.
    /// Inclus :
    /// - Builtins : `proxy.{base_domain}`, `auth.{base_domain}`, `dv.{base_domain}`
    /// - Routes manuelles enabled (`ProxyConfig::routes`)
    /// - Toutes les app routes (locales et distantes)
    /// Exclus :
    /// - L'apex `{base_domain}` (laissé NXDOMAIN)
    /// - Tout domain hors zone `{base_domain}` (custom domains externes)
    fn build_record_set(&self) -> Vec<StaticRecordDto> {
        let mut names: BTreeSet<String> = BTreeSet::new();

        // 1. Builtins (management endpoints — TLS terminé par hr-edge,
        //    routés vers homeroute API sur le port 4000).
        names.insert(format!("proxy.{}", self.base_domain));
        names.insert(format!("auth.{}", self.base_domain));
        names.insert(format!("dv.{}", self.base_domain));

        // 2. Routes manuelles
        let cfg = self.proxy.config();
        for r in &cfg.routes {
            if r.enabled && self.is_under_base_domain(&r.domain) && !self.is_apex(&r.domain) {
                names.insert(r.domain.to_lowercase());
            }
        }

        // 3. App routes (locales + distantes — TLS terminé sur hr-edge dans tous les cas)
        for (domain, _route) in self.proxy.list_app_routes() {
            if self.is_under_base_domain(&domain) && !self.is_apex(&domain) {
                names.insert(domain.to_lowercase());
            }
        }

        names
            .into_iter()
            .map(|name| StaticRecordDto {
                name,
                record_type: "A".to_string(),
                value: self.server_ip.to_string(),
                ttl: TTL_SECONDS,
                managed_by: None, // owner stamped server-side
            })
            .collect()
    }

    fn is_under_base_domain(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        let suffix = format!(".{}", self.base_domain.to_lowercase());
        n.ends_with(&suffix)
    }

    fn is_apex(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(&self.base_domain)
    }
}

/// Détermine l'IP que les FQDN internes doivent résoudre. Ordre de préférence :
/// 1. `EDGE_SERVER_IP` (champ EnvConfig)
/// 2. Auto-détection : 1ère IPv4 non-loopback de l'hôte
/// 3. Fallback hardcodé `10.0.0.254` avec warning
pub fn resolve_server_ip(explicit: Option<Ipv4Addr>) -> Ipv4Addr {
    if let Some(ip) = explicit {
        info!(ip = %ip, "Using EDGE_SERVER_IP for DNS sync");
        return ip;
    }

    if let Some(ip) = autodetect_server_ip() {
        info!(ip = %ip, "Auto-detected edge_server_ip from network interfaces");
        return ip;
    }

    let fallback: Ipv4Addr = "10.0.0.254".parse().unwrap();
    warn!(ip = %fallback, "No EDGE_SERVER_IP set and auto-detection failed — using hardcoded fallback");
    fallback
}

/// Best-effort autodétection : lit l'output de `ip -4 -o addr show` et prend
/// la première adresse non-loopback. Volontairement low-deps (pas de pnet).
fn autodetect_server_ip() -> Option<Ipv4Addr> {
    let out = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        // Format: "2: eth0    inet 10.0.0.254/24 brd ..."
        if line.contains("inet 127.") {
            continue;
        }
        if let Some(idx) = line.find("inet ") {
            let after = &line[idx + 5..];
            if let Some(slash) = after.find('/') {
                if let Ok(ip) = after[..slash].parse::<Ipv4Addr>() {
                    if !ip.is_loopback() {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_server_ip_uses_explicit() {
        let ip: Ipv4Addr = "10.42.0.1".parse().unwrap();
        assert_eq!(resolve_server_ip(Some(ip)), ip);
    }
}
