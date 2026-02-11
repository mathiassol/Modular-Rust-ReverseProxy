// Health check endpoint for monitoring
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(true));
    t.insert("endpoint".into(), toml::Value::String("/health".into()));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "health_check") { return; }
    let e = h::config_str(ctx.config, "health_check", "endpoint", "/health");
    ctx.pipeline.add(Box::new(Health { endpoint: e }));
}

struct Health {
    endpoint: String,
}
impl Module for Health {
    fn name(&self) -> &str { "health_check" }
    fn handle(&self, r: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
        if r.path == self.endpoint {
            Some(h::json_response(200, r#"{"status":"ok"}"#))
        } else {
            None
        }
    }
}
