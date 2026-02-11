// Gzip compression for HTTP responses
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("min_size".into(), toml::Value::Integer(256));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "compression") { return; }
    let min = h::config_u64(ctx.config, "compression", "min_size", 256) as usize;
    ctx.pipeline.add(Box::new(Compress { min_size: min }));
}

struct Compress {
    min_size: usize,
}

impl Module for Compress {
    fn name(&self) -> &str { "compression" }

    fn handle(&self, req: &mut HttpRequest, ctx: &mut Context) -> Option<HttpResponse> {
        if let Some(ae) = req.get_header("Accept-Encoding") {
            if ae.contains("gzip") {
                ctx.set("_accepts_gzip", "1".to_string());
            }
        }
        None
    }

    fn on_response(&self, _req: &HttpRequest, resp: &mut HttpResponse, ctx: &mut Context) {
        if ctx.get("_accepts_gzip").is_none() { return; }
        if resp.body.len() < self.min_size { return; }
        if resp.get_header("Content-Encoding").is_some() { return; }

        let ct = resp.get_header("Content-Type").unwrap_or("");
        if !is_compressible(ct) { return; }

        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        if enc.write_all(&resp.body).is_err() { return; }
        let compressed = match enc.finish() {
            Ok(v) => v,
            Err(_) => return,
        };

        if compressed.len() >= resp.body.len() { return; }

        resp.body = compressed;
        resp.set_header("Content-Encoding", "gzip");
        resp.set_header("Content-Length", &resp.body.len().to_string());
        resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("Transfer-Encoding"));
    }
}

fn is_compressible(ct: &str) -> bool {
    ct.starts_with("text/")
        || ct.contains("json")
        || ct.contains("xml")
        || ct.contains("javascript")
        || ct.contains("svg")
        || ct.contains("css")
}
