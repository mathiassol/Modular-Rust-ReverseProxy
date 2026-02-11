// Load balancer with round-robin selection
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("backends".into(), toml::Value::Array(vec![]));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "load_balancer") {
        ctx.pipeline.add(Box::new(Single { addr: ctx.server.backend_addr.clone() }));
        return;
    }
    let bs = h::config_vec_str(ctx.config, "load_balancer", "backends");
    if bs.is_empty() {
        ctx.pipeline.add(Box::new(Single { addr: ctx.server.backend_addr.clone() }));
    } else {
        ctx.pipeline.add(Box::new(RoundRobin { backends: bs, idx: Arc::new(AtomicUsize::new(0)) }));
    }
}

struct Single {
    addr: String,
}
impl Module for Single {
    fn name(&self) -> &str { "load_balancer" }
    fn handle(&self, _: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        c.set("_backend_addr", self.addr.clone());
        None
    }
}

struct RoundRobin {
    backends: Vec<String>,
    idx: Arc<AtomicUsize>,
}
impl Module for RoundRobin {
    fn name(&self) -> &str { "load_balancer" }
    fn handle(&self, _: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        let len = self.backends.len();
        let start = self.idx.fetch_add(1, Ordering::Relaxed) % len;
        for offset in 0..len {
            let addr = &self.backends[(start + offset) % len];
            if super::active_health::is_healthy(addr) {
                c.set("_backend_addr", addr.clone());
                return None;
            }
        }
        c.set("_backend_addr", self.backends[start % len].clone());
        None
    }
}
