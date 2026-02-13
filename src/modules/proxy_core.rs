// Core proxy module for backend forwarding
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::io::Write;
use std::time::Duration;

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
        let pool = crate::pool::global_pool();
        let mut s = match pool.get(&sock_addr, timeout) {
            Ok(s) => s,
            Err(_) => return Some(HttpResponse::error(502, "Backend unavailable")),
        };
        let _ = s.set_read_timeout(Some(timeout));
        let _ = s.set_write_timeout(Some(timeout));
        if let Err(e) = s.write_all(&r.to_bytes()) {
            crate::log::warn(&format!("proxy_core: backend write error: {e}"));
            return Some(HttpResponse::error(502, "Backend write failed"));
        }
        let raw = crate::http::read_http_message(&mut s, self.buf);
        let resp = match raw {
            crate::http::ReadResult::Ok(d) => {
                match HttpResponse::parse(&d) {
                    Some(parsed) => {
                        let conn_hdr = parsed.get_header("Connection").unwrap_or("");
                        let keep_alive = if parsed.version == "HTTP/1.0" {
                            conn_hdr.eq_ignore_ascii_case("keep-alive")
                        } else {
                            !conn_hdr.eq_ignore_ascii_case("close")
                        };
                        if keep_alive {
                            pool.put(sock_addr, s);
                        }
                        parsed
                    }
                    None => {
                        crate::log::warn("proxy_core: failed to parse backend response");
                        HttpResponse::error(502, "Parse failed")
                    }
                }
            }
            crate::http::ReadResult::TimedOut => HttpResponse::error(504, "Backend timeout"),
            crate::http::ReadResult::Error(e) => {
                crate::log::warn(&format!("proxy_core: backend error: {e}"));
                HttpResponse::error(502, "Backend error")
            }
        };
        Some(resp)
    }
}
