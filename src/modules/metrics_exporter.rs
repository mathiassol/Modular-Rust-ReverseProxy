// Prometheus metrics exporter
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("endpoint".into(), toml::Value::String("/metrics".into()));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "metrics_exporter") { return; }
    let ep = h::config_str(ctx.config, "metrics_exporter", "endpoint", "/metrics");
    ctx.pipeline.add(Box::new(MetricsExporter { endpoint: ep }));
}

struct MetricsExporter {
    endpoint: String,
}

impl Module for MetricsExporter {
    fn name(&self) -> &str { "metrics_exporter" }

    fn handle(&self, r: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
        if r.method != "GET" || r.path != self.endpoint { return None; }
        let body = crate::metrics::snapshot_prometheus();
        Some(HttpResponse {
            version: "HTTP/1.1".to_string(),
            status_code: 200,
            status_text: "OK".to_string(),
            headers: vec![
                ("Content-Type".to_string(), "text/plain; version=0.0.4; charset=utf-8".to_string()),
                ("Content-Length".to_string(), body.len().to_string()),
            ],
            body: body.into_bytes(),
        })
    }
}
