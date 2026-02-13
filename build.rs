// Auto-discovery and code generation for proxy modules
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/modules");

    let modules_dir = Path::new("src/modules");
    let mod_file = modules_dir.join("mod.rs");

    let mut module_names = Vec::new();
    let mut registerable = Vec::new();
    let mut has_defaults = Vec::new();

    if let Ok(entries) = fs::read_dir(modules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() {
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if !file_name.ends_with(".rs") || file_name == "mod.rs" {
                    continue;
                }
                println!("cargo:rerun-if-changed={}", path.display());
                let name = file_name.trim_end_matches(".rs").to_string();
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("cargo:warning=Failed to read {}: {}", path.display(), e);
                        continue;
                    }
                };
                module_names.push(name.clone());
                if content.contains("pub fn register") {
                    registerable.push(name.clone());
                }
                if content.contains("pub fn default_config") {
                    has_defaults.push(name);
                }
                continue;
            }

            if path.is_dir() {
                let mod_rs = path.join("mod.rs");
                if !mod_rs.exists() { continue; }
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let content = match fs::read_to_string(&mod_rs) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("cargo:warning=Failed to read {}: {}", mod_rs.display(), e);
                        continue;
                    }
                };
                module_names.push(name.clone());
                if content.contains("pub fn register") {
                    registerable.push(name.clone());
                }
                if content.contains("pub fn default_config") {
                    has_defaults.push(name);
                }
            }
        }
    }

    module_names.sort();
    registerable.sort();
    has_defaults.sort();

    if module_names.is_empty() {
        println!("cargo:warning=No modules found in src/modules/");
    }
    if registerable.is_empty() {
        println!("cargo:warning=No registerable modules found (missing pub fn register)");
    }

    println!("cargo:warning=Discovered {} modules, {} registerable, {} with defaults",
        module_names.len(), registerable.len(), has_defaults.len());

    let mut c = format!(
        "// Auto-generated module registry — {} modules discovered\n",
        module_names.len()
    );
    for name in &module_names {
        c.push_str(&format!("mod {};\n", name));
    }

    c.push_str(r#"
use crate::config::Srv;
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::{HashMap, HashSet};

pub trait Module: Send + Sync {
    fn name(&self) -> &str;
    fn overrides(&self) -> &'static [&'static str] { &[] }
    fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse>;
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

pub struct ModuleContext<'a> {
    pub pipeline: &'a mut Pipeline,
    pub config: &'a HashMap<String, toml::Value>,
    pub server: &'a Srv,
}

pub fn register_all(p: &mut Pipeline, mc: &HashMap<String, toml::Value>, sc: &Srv) {
    let mut ctx = ModuleContext { pipeline: p, config: mc, server: sc };
"#);

    // Desired registration order: early middleware first, proxy_core/raw_tcp last
    let priority = [
        "active_health", "request_id", "rate_limiter", "circuit_breaker",
        "health_check", "metrics_exporter", "admin_api", "cache",
        "url_rewriter", "compression", "load_balancer", "proxy_core", "raw_tcp",
    ];

    let mut ordered: Vec<&String> = Vec::new();
    for p in &priority {
        if let Some(name) = registerable.iter().find(|n| n.as_str() == *p) {
            ordered.push(name);
        }
    }
    // Any remaining modules not in priority list, alphabetically
    for name in &registerable {
        if !priority.contains(&name.as_str()) {
            ordered.push(name);
        }
    }

    for name in &ordered {
        c.push_str(&format!("    {}::register(&mut ctx);\n", name));
    }
    c.push_str("}\n\npub fn collect_defaults() -> HashMap<String, toml::Value> {\n    let mut d = HashMap::new();\n");
    for name in &has_defaults {
        c.push_str(&format!("    d.insert(\"{}\".into(), toml::Value::Table({}::default_config()));\n", name, name));
    }
    c.push_str("    d\n}\n");

    if let Err(e) = fs::write(&mod_file, c) {
        panic!("Failed to write module registry {}: {}", mod_file.display(), e);
    }
}

