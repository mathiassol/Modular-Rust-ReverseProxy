// Request ID injection for tracing
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "request_id") { return; }
    ctx.pipeline.add(Box::new(RequestId));
}

fn generate_id() -> String {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:04x}")
}

struct RequestId;

impl Module for RequestId {
    fn name(&self) -> &str { "request_id" }

    fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        let id = r.get_header("X-Request-Id")
            .map(|s| s.to_string())
            .unwrap_or_else(generate_id);
        r.set_header("X-Request-Id", &id);
        c.set("_request_id", id);
        None
    }

    fn on_response(&self, _req: &HttpRequest, resp: &mut HttpResponse, ctx: &mut Context) {
        if let Some(id) = ctx.get("_request_id") {
            resp.headers.push(("X-Request-Id".to_string(), id.to_string()));
        }
    }
}
