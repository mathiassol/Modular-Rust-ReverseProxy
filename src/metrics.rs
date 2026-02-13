// Lock-free metrics using atomic counters
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static START_TIME: OnceLock<Instant> = OnceLock::new();

static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static REQUESTS_OK: AtomicU64 = AtomicU64::new(0);
static REQUESTS_ERR: AtomicU64 = AtomicU64::new(0);
static BYTES_IN: AtomicU64 = AtomicU64::new(0);
static BYTES_OUT: AtomicU64 = AtomicU64::new(0);
static LATENCY_SUM_MS: AtomicU64 = AtomicU64::new(0);
static LATENCY_MAX_MS: AtomicU64 = AtomicU64::new(0);
static CONNECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static POOL_HITS: AtomicU64 = AtomicU64::new(0);
static POOL_MISSES: AtomicU64 = AtomicU64::new(0);
static CB_TRIPS: AtomicU64 = AtomicU64::new(0);
static CB_REJECTS: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    START_TIME.get_or_init(Instant::now);
}

#[inline] pub fn inc_requests() { REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_requests_ok() { REQUESTS_OK.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_requests_err() { REQUESTS_ERR.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn add_bytes_in(n: u64) { BYTES_IN.fetch_add(n, Ordering::Relaxed); }
#[inline] pub fn add_bytes_out(n: u64) { BYTES_OUT.fetch_add(n, Ordering::Relaxed); }
#[inline] pub fn inc_connections() { CONNECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_pool_hits() { POOL_HITS.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_pool_misses() { POOL_MISSES.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_cb_trips() { CB_TRIPS.fetch_add(1, Ordering::Relaxed); }
#[inline] pub fn inc_cb_rejects() { CB_REJECTS.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn record_latency(ms: u64) {
    let capped = ms.min(600_000);
    LATENCY_SUM_MS.fetch_add(capped, Ordering::Relaxed);
    let mut current = LATENCY_MAX_MS.load(Ordering::Relaxed);
    while capped > current {
        match LATENCY_MAX_MS.compare_exchange_weak(current, capped, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(c) => current = c,
        }
    }
}

pub struct Snapshot {
    pub latency_max_ms: u64,
    pub latency_sum_ms: u64,
    pub requests_total: u64,
    pub requests_ok: u64,
    pub requests_err: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub connections_total: u64,
    pub active_connections: usize,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub cb_trips: u64,
    pub cb_rejects: u64,
    pub uptime_secs: u64,
}

impl Snapshot {
    pub fn avg_latency_ms(&self) -> u64 {
        if self.requests_total > 0 {
            self.latency_sum_ms / self.requests_total
        } else {
            0
        }
    }
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        requests_total: REQUESTS_TOTAL.load(Ordering::Relaxed),
        requests_ok: REQUESTS_OK.load(Ordering::Relaxed),
        requests_err: REQUESTS_ERR.load(Ordering::Relaxed),
        bytes_in: BYTES_IN.load(Ordering::Relaxed),
        bytes_out: BYTES_OUT.load(Ordering::Relaxed),
        latency_sum_ms: LATENCY_SUM_MS.load(Ordering::Relaxed),
        latency_max_ms: LATENCY_MAX_MS.load(Ordering::Relaxed),
        connections_total: CONNECTIONS_TOTAL.load(Ordering::Relaxed),
        active_connections: crate::server::active_connections(),
        pool_hits: POOL_HITS.load(Ordering::Relaxed),
        pool_misses: POOL_MISSES.load(Ordering::Relaxed),
        cb_trips: CB_TRIPS.load(Ordering::Relaxed),
        cb_rejects: CB_REJECTS.load(Ordering::Relaxed),
        uptime_secs: START_TIME.get().map(|t| t.elapsed().as_secs()).unwrap_or(0),
    }
}

pub fn snapshot_prometheus() -> String {
    let s = snapshot();

    format!(
        "# HELP proxycache_uptime_seconds Server uptime\n\
         # TYPE proxycache_uptime_seconds gauge\n\
         proxycache_uptime_seconds {}\n\
         # HELP proxycache_requests_total Total requests\n\
         # TYPE proxycache_requests_total counter\n\
         proxycache_requests_total {}\n\
         # TYPE proxycache_requests_ok counter\n\
         proxycache_requests_ok {}\n\
         # TYPE proxycache_requests_err counter\n\
         proxycache_requests_err {}\n\
         # HELP proxycache_active_connections Current active connections\n\
         # TYPE proxycache_active_connections gauge\n\
         proxycache_active_connections {}\n\
         # TYPE proxycache_connections_total counter\n\
         proxycache_connections_total {}\n\
         # TYPE proxycache_bytes_in counter\n\
         proxycache_bytes_in {}\n\
         # TYPE proxycache_bytes_out counter\n\
         proxycache_bytes_out {}\n\
         # TYPE proxycache_latency_sum_ms counter\n\
         proxycache_latency_sum_ms {}\n\
         # TYPE proxycache_latency_max_ms gauge\n\
         proxycache_latency_max_ms {}\n\
         # TYPE proxycache_pool_hits counter\n\
         proxycache_pool_hits {}\n\
         # TYPE proxycache_pool_misses counter\n\
         proxycache_pool_misses {}\n\
         # TYPE proxycache_circuit_breaker_trips counter\n\
         proxycache_circuit_breaker_trips {}\n\
         # TYPE proxycache_circuit_breaker_rejects counter\n\
         proxycache_circuit_breaker_rejects {}\n",
        s.uptime_secs, s.requests_total, s.requests_ok, s.requests_err,
        s.active_connections, s.connections_total, s.bytes_in, s.bytes_out,
        s.latency_sum_ms, s.latency_max_ms, s.pool_hits, s.pool_misses,
        s.cb_trips, s.cb_rejects,
    )
}

pub fn snapshot_json() -> String {
    let s = snapshot();
    let avg_lat = if s.requests_total > 0 { s.latency_sum_ms / s.requests_total } else { 0 };

    format!(
        r#"{{"uptime_seconds":{},"requests_total":{},"requests_ok":{},"requests_err":{},"active_connections":{},"connections_total":{},"bytes_in":{},"bytes_out":{},"latency_avg_ms":{},"latency_max_ms":{},"pool_hits":{},"pool_misses":{},"circuit_breaker_trips":{},"circuit_breaker_rejects":{}}}"#,
        s.uptime_secs, s.requests_total, s.requests_ok, s.requests_err,
        s.active_connections, s.connections_total, s.bytes_in, s.bytes_out,
        avg_lat, s.latency_max_ms, s.pool_hits, s.pool_misses,
        s.cb_trips, s.cb_rejects,
    )
}
