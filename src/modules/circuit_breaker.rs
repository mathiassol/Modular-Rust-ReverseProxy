// Circuit breaker for backend failure protection
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::Mutex;

const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("failure_threshold".into(), toml::Value::Integer(5));
    t.insert("recovery_timeout".into(), toml::Value::Integer(30));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "circuit_breaker") { return; }
    let threshold = h::config_u64(ctx.config, "circuit_breaker", "failure_threshold", 5);
    let recovery = h::config_u64(ctx.config, "circuit_breaker", "recovery_timeout", 30);
    ctx.pipeline.add(Box::new(CircuitBreaker {
        threshold,
        recovery_secs: recovery,
        failures: Arc::new(AtomicU64::new(0)),
        state: Arc::new(AtomicU8::new(STATE_CLOSED)),
        opened_at: Arc::new(Mutex::new(Instant::now())),
    }));
}

struct CircuitBreaker {
    threshold: u64,
    recovery_secs: u64,
    failures: Arc<AtomicU64>,
    state: Arc<AtomicU8>,
    opened_at: Arc<Mutex<Instant>>,
}

impl Module for CircuitBreaker {
    fn name(&self) -> &str { "circuit_breaker" }

    fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
        let state = self.state.load(Ordering::Acquire);
        match state {
            STATE_OPEN => {
                let elapsed = match self.opened_at.lock() {
                    Ok(t) => t.elapsed(),
                    Err(poisoned) => poisoned.into_inner().elapsed(),
                };
                if elapsed >= Duration::from_secs(self.recovery_secs) {
                    if self.state.compare_exchange(
                        STATE_OPEN, STATE_HALF_OPEN,
                        Ordering::AcqRel, Ordering::Acquire,
                    ).is_ok() {
                        crate::log::info("circuit_breaker: half-open, probing backend");
                    }
                    None
                } else {
                    crate::metrics::inc_cb_rejects();
                    Some(HttpResponse::error(503, "Circuit breaker open"))
                }
            }
            STATE_HALF_OPEN => None,
            _ => None,
        }
    }

    fn on_response(&self, _req: &HttpRequest, resp: &mut HttpResponse, _ctx: &mut Context) {
        let state = self.state.load(Ordering::Acquire);
        if resp.status_code >= 500 {
            let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
            if state == STATE_HALF_OPEN {
                if self.state.compare_exchange(
                    STATE_HALF_OPEN, STATE_OPEN,
                    Ordering::AcqRel, Ordering::Acquire,
                ).is_ok() {
                    match self.opened_at.lock() {
                        Ok(mut t) => *t = Instant::now(),
                        Err(poisoned) => *poisoned.into_inner() = Instant::now(),
                    }
                    crate::metrics::inc_cb_trips();
                    crate::log::warn("circuit_breaker: OPEN (half-open probe failed)");
                }
            } else if count >= self.threshold {
                if self.state.compare_exchange(
                    STATE_CLOSED, STATE_OPEN,
                    Ordering::AcqRel, Ordering::Acquire,
                ).is_ok() {
                    match self.opened_at.lock() {
                        Ok(mut t) => *t = Instant::now(),
                        Err(poisoned) => *poisoned.into_inner() = Instant::now(),
                    }
                    crate::metrics::inc_cb_trips();
                    crate::log::warn(&format!("circuit_breaker: OPEN after {count} failures"));
                }
            }
        } else {
            if state != STATE_CLOSED {
                crate::log::info("circuit_breaker: CLOSED, backend recovered");
            }
            self.failures.store(0, Ordering::Relaxed);
            self.state.store(STATE_CLOSED, Ordering::Release);
        }
    }
}
