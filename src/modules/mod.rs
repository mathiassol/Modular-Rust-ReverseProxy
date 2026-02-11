// Auto-generated - DO NOT EDIT
// Drop .rs files (or dirs with mod.rs) into src/modules/ for auto-discovery
mod active_health;
mod admin_api;
mod cache;
mod circuit_breaker;
mod compression;
mod health_check;
mod helpers;
mod load_balancer;
mod metrics_exporter;
mod proxy_core;
mod rate_limiter;
mod raw_tcp;
mod request_id;
mod url_rewriter;

use crate::config::Srv;
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::{HashMap, HashSet};

pub trait Module: Send + Sync {
    fn name(&self) -> &str;
    fn overrides(&self) -> &'static [&'static str] { &[] }
    /// Return Some(response) to short-circuit, None to continue.
    fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse>;
    /// Called after response is produced, in reverse pipeline order.
    fn on_response(&self, _req: &HttpRequest, _resp: &mut HttpResponse, _ctx: &mut Context) {}
}

pub trait RawHandler: Send + Sync {
    fn handle_raw(&self, client: std::net::TcpStream);
}

pub struct Pipeline {
    mods: Vec<Box<dyn Module>>,
    raw: Option<Box<dyn RawHandler>>,
    overridden: HashSet<String>,
    to: u64,
}

impl Pipeline {
    pub fn new(t: u64) -> Self {
        Pipeline { mods: Vec::new(), raw: None, overridden: HashSet::new(), to: t }
    }
    pub fn add(&mut self, m: Box<dyn Module>) {
        let name = m.name().to_string();
        if self.overridden.contains(&name) {
            crate::log::module_skipped(&name);
            return;
        }
        for o in m.overrides() {
            self.override_module(o);
        }
        crate::log::module_loaded(&name);
        self.mods.push(m);
    }
    pub fn override_module(&mut self, name: &str) {
        self.overridden.insert(name.to_string());
        self.mods.retain(|m| m.name() != name);
    }
    pub fn set_raw_handler(&mut self, h: Box<dyn RawHandler>) {
        crate::log::module_loaded("raw connection handler");
        self.raw = Some(h);
    }
    pub fn raw_handler(&self) -> Option<&dyn RawHandler> {
        self.raw.as_deref()
    }
    pub fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> HttpResponse {
        let mut resp_idx = None;
        let mut resp = HttpResponse::error(500, "No handler");
        for (i, m) in self.mods.iter().enumerate() {
            if let Some(r) = m.handle(r, c) {
                resp = r;
                resp_idx = Some(i);
                break;
            }
        }
        let limit = resp_idx.map(|i| i + 1).unwrap_or(self.mods.len());
        for m in self.mods[..limit].iter().rev() {
            m.on_response(r, &mut resp, c);
        }
        resp
    }
    pub fn timeout(&self) -> u64 { self.to }
}

/// Registration context: pipeline + config + server settings.
pub struct ModuleContext<'a> {
    pub pipeline: &'a mut Pipeline,
    pub config: &'a HashMap<String, toml::Value>,
    pub server: &'a Srv,
}

pub fn register_all(p: &mut Pipeline, mc: &HashMap<String, toml::Value>, sc: &Srv) {
    let mut ctx = ModuleContext { pipeline: p, config: mc, server: sc };
    active_health::register(&mut ctx);
    request_id::register(&mut ctx);
    rate_limiter::register(&mut ctx);
    circuit_breaker::register(&mut ctx);
    health_check::register(&mut ctx);
    metrics_exporter::register(&mut ctx);
    admin_api::register(&mut ctx);
    cache::register(&mut ctx);
    url_rewriter::register(&mut ctx);
    compression::register(&mut ctx);
    load_balancer::register(&mut ctx);
    proxy_core::register(&mut ctx);
    raw_tcp::register(&mut ctx);
}

pub fn collect_defaults() -> HashMap<String, toml::Value> {
    let mut d = HashMap::new();
    d.insert("active_health".into(), toml::Value::Table(active_health::default_config()));
    d.insert("admin_api".into(), toml::Value::Table(admin_api::default_config()));
    d.insert("cache".into(), toml::Value::Table(cache::default_config()));
    d.insert("circuit_breaker".into(), toml::Value::Table(circuit_breaker::default_config()));
    d.insert("compression".into(), toml::Value::Table(compression::default_config()));
    d.insert("health_check".into(), toml::Value::Table(health_check::default_config()));
    d.insert("load_balancer".into(), toml::Value::Table(load_balancer::default_config()));
    d.insert("metrics_exporter".into(), toml::Value::Table(metrics_exporter::default_config()));
    d.insert("proxy_core".into(), toml::Value::Table(proxy_core::default_config()));
    d.insert("rate_limiter".into(), toml::Value::Table(rate_limiter::default_config()));
    d.insert("raw_tcp".into(), toml::Value::Table(raw_tcp::default_config()));
    d.insert("request_id".into(), toml::Value::Table(request_id::default_config()));
    d.insert("url_rewriter".into(), toml::Value::Table(url_rewriter::default_config()));
    d
}
