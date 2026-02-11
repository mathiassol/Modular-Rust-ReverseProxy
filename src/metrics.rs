// Lock-free metrics using atomic counters
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static COUNTERS: OnceLock<Mutex<HashMap<String, &'static AtomicU64>>> = OnceLock::new();
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
    COUNTERS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("requests_total".into(), &REQUESTS_TOTAL);
        m.insert("requests_ok".into(), &REQUESTS_OK);
        m.insert("requests_err".into(), &REQUESTS_ERR);
        m.insert("bytes_in".into(), &BYTES_IN);
        m.insert("bytes_out".into(), &BYTES_OUT);
        m.insert("latency_sum_ms".into(), &LATENCY_SUM_MS);
        m.insert("latency_max_ms".into(), &LATENCY_MAX_MS);
        m.insert("connections_total".into(), &CONNECTIONS_TOTAL);
        m.insert("pool_hits".into(), &POOL_HITS);
        m.insert("pool_misses".into(), &POOL_MISSES);
        m.insert("circuit_breaker_trips".into(), &CB_TRIPS);
        m.insert("circuit_breaker_rejects".into(), &CB_REJECTS);
        Mutex::new(m)
    });
}

#[inline]pub fn inc_requests() { REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_requests_ok() { REQUESTS_OK.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_requests_err() { REQUESTS_ERR.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn add_bytes_in(n: u64) { BYTES_IN.fetch_add(n, Ordering::Relaxed); }

#[inline]
pub fn add_bytes_out(n: u64) { BYTES_OUT.fetch_add(n, Ordering::Relaxed); }

#[inline]
pub fn inc_connections() { CONNECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_pool_hits() { POOL_HITS.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_pool_misses() { POOL_MISSES.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_cb_trips() { CB_TRIPS.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn inc_cb_rejects() { CB_REJECTS.fetch_add(1, Ordering::Relaxed); }

#[inline]
pub fn record_latency(ms: u64) {
    LATENCY_SUM_MS.fetch_add(ms, Ordering::Relaxed);
    let mut current = LATENCY_MAX_MS.load(Ordering::Relaxed);
    while ms > current {
        match LATENCY_MAX_MS.compare_exchange_weak(current, ms, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(c) => current = c,
        }
    }
}

pub fn snapshot_prometheus() -> String {
    let total = REQUESTS_TOTAL.load(Ordering::Relaxed);
    let ok = REQUESTS_OK.load(Ordering::Relaxed);
    let err = REQUESTS_ERR.load(Ordering::Relaxed);
    let b_in = BYTES_IN.load(Ordering::Relaxed);
    let b_out = BYTES_OUT.load(Ordering::Relaxed);
    let lat_sum = LATENCY_SUM_MS.load(Ordering::Relaxed);
    let lat_max = LATENCY_MAX_MS.load(Ordering::Relaxed);
    let conns = CONNECTIONS_TOTAL.load(Ordering::Relaxed);
    let active = crate::server::active_connections() as u64;
    let pool_h = POOL_HITS.load(Ordering::Relaxed);
    let pool_m = POOL_MISSES.load(Ordering::Relaxed);
    let cb_t = CB_TRIPS.load(Ordering::Relaxed);
    let cb_r = CB_REJECTS.load(Ordering::Relaxed);
    let uptime = START_TIME.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);

    format!(
        "# HELP proxycache_uptime_seconds Server uptime\n\
         # TYPE proxycache_uptime_seconds gauge\n\
         proxycache_uptime_seconds {uptime}\n\
         # HELP proxycache_requests_total Total requests\n\
         # TYPE proxycache_requests_total counter\n\
         proxycache_requests_total {total}\n\
         # TYPE proxycache_requests_ok counter\n\
         proxycache_requests_ok {ok}\n\
         # TYPE proxycache_requests_err counter\n\
         proxycache_requests_err {err}\n\
         # HELP proxycache_active_connections Current active connections\n\
         # TYPE proxycache_active_connections gauge\n\
         proxycache_active_connections {active}\n\
         # TYPE proxycache_connections_total counter\n\
         proxycache_connections_total {conns}\n\
         # TYPE proxycache_bytes_in counter\n\
         proxycache_bytes_in {b_in}\n\
         # TYPE proxycache_bytes_out counter\n\
         proxycache_bytes_out {b_out}\n\
         # TYPE proxycache_latency_sum_ms counter\n\
         proxycache_latency_sum_ms {lat_sum}\n\
         # TYPE proxycache_latency_max_ms gauge\n\
         proxycache_latency_max_ms {lat_max}\n\
         # TYPE proxycache_pool_hits counter\n\
         proxycache_pool_hits {pool_h}\n\
         # TYPE proxycache_pool_misses counter\n\
         proxycache_pool_misses {pool_m}\n\
         # TYPE proxycache_circuit_breaker_trips counter\n\
         proxycache_circuit_breaker_trips {cb_t}\n\
         # TYPE proxycache_circuit_breaker_rejects counter\n\
         proxycache_circuit_breaker_rejects {cb_r}\n"
    )
}

pub fn snapshot_json() -> String {
    let total = REQUESTS_TOTAL.load(Ordering::Relaxed);
    let ok = REQUESTS_OK.load(Ordering::Relaxed);
    let err = REQUESTS_ERR.load(Ordering::Relaxed);
    let b_in = BYTES_IN.load(Ordering::Relaxed);
    let b_out = BYTES_OUT.load(Ordering::Relaxed);
    let lat_sum = LATENCY_SUM_MS.load(Ordering::Relaxed);
    let lat_max = LATENCY_MAX_MS.load(Ordering::Relaxed);
    let conns = CONNECTIONS_TOTAL.load(Ordering::Relaxed);
    let active = crate::server::active_connections();
    let pool_h = POOL_HITS.load(Ordering::Relaxed);
    let pool_m = POOL_MISSES.load(Ordering::Relaxed);
    let cb_t = CB_TRIPS.load(Ordering::Relaxed);
    let cb_r = CB_REJECTS.load(Ordering::Relaxed);
    let avg_lat = if total > 0 { lat_sum / total } else { 0 };
    let uptime = START_TIME.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);

    format!(
        r#"{{"uptime_seconds":{uptime},"requests_total":{total},"requests_ok":{ok},"requests_err":{err},"active_connections":{active},"connections_total":{conns},"bytes_in":{b_in},"bytes_out":{b_out},"latency_avg_ms":{avg_lat},"latency_max_ms":{lat_max},"pool_hits":{pool_h},"pool_misses":{pool_m},"circuit_breaker_trips":{cb_t},"circuit_breaker_rejects":{cb_r}}}"#
    )
}
