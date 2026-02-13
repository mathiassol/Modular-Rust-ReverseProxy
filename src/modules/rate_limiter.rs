// Token-bucket rate limiting by client IP
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const BUCKET_CLEANUP_THRESHOLD: usize = 10_000;
const BUCKET_STALE_SECS: f64 = 300.0;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("requests_per_second".into(), toml::Value::Integer(10));
    t.insert("burst".into(), toml::Value::Integer(20));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "rate_limiter") { return; }
    let r = h::config_usize(ctx.config, "rate_limiter", "requests_per_second", 10);
    let b = h::config_usize(ctx.config, "rate_limiter", "burst", r * 2);
    ctx.pipeline.add(Box::new(RateLimit {
        rps: r,
        burst: b,
        buckets: Arc::new(Mutex::new(HashMap::new())),
    }));
}

struct RateLimit {
    rps: usize,
    burst: usize,
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Module for RateLimit {
    fn name(&self) -> &str { "rate_limiter" }
    fn handle(&self, _: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        let ip = h::client_ip(c);
        let mut bs = match self.buckets.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                crate::log::warn("rate_limiter: mutex recovered after panic");
                poisoned.into_inner()
            }
        };

        if bs.len() > BUCKET_CLEANUP_THRESHOLD {
            let now = Instant::now();
            bs.retain(|_, b| now.duration_since(b.last).as_secs_f64() < BUCKET_STALE_SECS);
        }

        let b = bs.entry(ip).or_insert(Bucket { tokens: self.burst as f64, last: Instant::now() });
        let elapsed = b.last.elapsed().as_secs_f64();
        b.tokens = (b.tokens + elapsed * self.rps as f64).min(self.burst as f64);
        b.last = Instant::now();
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            None
        } else {
            Some(HttpResponse::error(429, "Rate limit"))
        }
    }
}
