// URL path rewriting module
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::HashMap;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "url_rewriter") { return; }
    let r = load_rules(ctx.config);
    if !r.is_empty() {
        ctx.pipeline.add(Box::new(Rewrite { rules: r }));
    }
}

fn load_rules(c: &HashMap<String, toml::Value>) -> Vec<(String, String)> {
    c.get("url_rewriter").and_then(|v| v.get("rules")).and_then(|v| v.as_table()).map(|t| {
        t.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()
    }).unwrap_or_default()
}

struct Rewrite {
    rules: Vec<(String, String)>,
}

impl Module for Rewrite {
    fn name(&self) -> &str { "url_rewriter" }
    fn handle(&self, r: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
        for (p, repl) in &self.rules {
            if r.path.starts_with(p) {
                r.path = r.path.replacen(p, repl, 1);
                break;
            }
        }
        None
    }
}
