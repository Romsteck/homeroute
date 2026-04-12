use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

/// Per-domain counters.
struct DomainCounters {
    total_requests: AtomicU64,
    errors_5xx: AtomicU64,
}

/// Simple atomic counter system for proxy metrics.
///
/// Thread-safe: the `Mutex` only guards the `HashMap` (insert/lookup),
/// while the actual increments are lock-free `AtomicU64` operations.
pub struct ProxyMetrics {
    domains: Mutex<HashMap<String, Arc<DomainCounters>>>,
    global_total: AtomicU64,
    global_2xx: AtomicU64,
    global_4xx: AtomicU64,
    global_5xx: AtomicU64,
    started_at: Instant,
}

impl ProxyMetrics {
    pub fn new() -> Self {
        Self {
            domains: Mutex::new(HashMap::new()),
            global_total: AtomicU64::new(0),
            global_2xx: AtomicU64::new(0),
            global_4xx: AtomicU64::new(0),
            global_5xx: AtomicU64::new(0),
            started_at: Instant::now(),
        }
    }

    /// Get or create the counters for a domain.
    fn counters(&self, domain: &str) -> Arc<DomainCounters> {
        let mut map = self.domains.lock().unwrap();
        map.entry(domain.to_string())
            .or_insert_with(|| {
                Arc::new(DomainCounters {
                    total_requests: AtomicU64::new(0),
                    errors_5xx: AtomicU64::new(0),
                })
            })
            .clone()
    }

    /// Record a completed request.  Call once per request after the
    /// response status code is known.
    pub fn record_request(&self, domain: &str, status: u16) {
        self.global_total.fetch_add(1, Ordering::Relaxed);
        match status {
            200..=299 => {
                self.global_2xx.fetch_add(1, Ordering::Relaxed);
            }
            400..=499 => {
                self.global_4xx.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                self.global_5xx.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        let c = self.counters(domain);
        c.total_requests.fetch_add(1, Ordering::Relaxed);
        if (500..600).contains(&status) {
            c.errors_5xx.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Snapshot of all per-domain metrics for serialization.
    pub fn snapshot(&self) -> Vec<DomainStats> {
        let map = self.domains.lock().unwrap();
        let mut out: Vec<DomainStats> = map
            .iter()
            .map(|(domain, c)| DomainStats {
                domain: domain.clone(),
                total_requests: c.total_requests.load(Ordering::Relaxed),
                errors_5xx: c.errors_5xx.load(Ordering::Relaxed),
            })
            .collect();
        out.sort_by(|a, b| b.total_requests.cmp(&a.total_requests));
        out
    }

    /// Global counters snapshot (all domains combined).
    pub fn global_snapshot(&self) -> GlobalStats {
        let total = self.global_total.load(Ordering::Relaxed);
        let uptime_secs = self.started_at.elapsed().as_secs();
        let rps = if uptime_secs > 0 {
            total as f64 / uptime_secs as f64
        } else {
            0.0
        };
        GlobalStats {
            total_requests: total,
            status_2xx: self.global_2xx.load(Ordering::Relaxed),
            status_4xx: self.global_4xx.load(Ordering::Relaxed),
            status_5xx: self.global_5xx.load(Ordering::Relaxed),
            uptime_secs,
            requests_per_second: rps,
        }
    }
}

/// Global metrics across all domains.
#[derive(Debug, Clone, Serialize)]
pub struct GlobalStats {
    pub total_requests: u64,
    pub status_2xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub uptime_secs: u64,
    pub requests_per_second: f64,
}

/// Serializable per-domain stats entry.
#[derive(Debug, Clone, Serialize)]
pub struct DomainStats {
    pub domain: String,
    pub total_requests: u64,
    pub errors_5xx: u64,
}
