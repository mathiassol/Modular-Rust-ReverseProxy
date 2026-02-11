// Core proxy module for backend forwarding
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use crate::pool::ConnPool;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

static POOL: std::sync::OnceLock<Arc<ConnPool>> = std::sync::OnceLock::new();

fn get_pool() -> &'static Arc<ConnPool> {
    POOL.get_or_init(|| Arc::new(ConnPool::new()))
}

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(true));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "proxy_core") { return; }
    ctx.pipeline.add(Box::new(ProxyCore { to: ctx.server.backend_timeout, buf: ctx.server.buffer_size }));
}

struct ProxyCore {
    to: u64,
    buf: usize,
}

impl Module for ProxyCore {
    fn name(&self) -> &str { "proxy_core" }
    fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        let addr = c.get("_backend_addr")?;
        let sock_addr: std::net::SocketAddr = match addr.parse() {
            Ok(a) => a,
            Err(_) => return Some(HttpResponse::error(502, "Invalid backend address")),
        };
        let timeout = Duration::from_secs(self.to);
        let pool = get_pool();
        let mut s = match pool.get(&sock_addr, timeout) {
            Ok(s) => s,
            Err(_) => return Some(HttpResponse::error(502, "Backend unavailable")),
        };
        let _ = s.set_read_timeout(Some(timeout));
        let _ = s.set_write_timeout(Some(timeout));
        if s.write_all(&r.to_bytes()).is_err() {
            return Some(HttpResponse::error(502, "Backend write failed"));
        }
        let raw = crate::http::read_http_message(&mut s, self.buf);
        let resp = match raw {
            crate::http::ReadResult::Ok(d) => {
                let reuse = !d.is_empty();
                let parsed = HttpResponse::parse(&d).unwrap_or_else(|| HttpResponse::error(502, "Parse failed"));
                if reuse {
                    let conn_hdr = parsed.get_header("Connection").unwrap_or("");
                    if !conn_hdr.eq_ignore_ascii_case("close") {
                        if let Ok(cloned) = s.try_clone() {
                            pool.put(sock_addr, cloned);
                        }
                    }
                }
                parsed
            }
            crate::http::ReadResult::TimedOut => HttpResponse::error(504, "Backend timeout"),
            crate::http::ReadResult::Error(_) => HttpResponse::error(502, "Backend error"),
        };
        Some(resp)
    }
}
