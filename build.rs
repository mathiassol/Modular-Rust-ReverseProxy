// Auto-discovery and code generation for proxy modules
use std::fs;
use std::path::Path;

fn scan_module_dir(dir: &Path, module_names: &mut Vec<String>, registerable: &mut Vec<String>, has_defaults: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() {
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if !file_name.ends_with(".rs") || file_name == "mod.rs" || file_name == "helpers.rs" {
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
}

fn main() {
    println!("cargo:rerun-if-changed=src/modules");
    println!("cargo:rerun-if-changed=imports");

    let modules_dir = Path::new("src/modules");
    let imports_dir = Path::new("imports");
    let mod_file = modules_dir.join("mod.rs");

    let mut module_names = Vec::new();
    let mut registerable = Vec::new();
    let mut has_defaults = Vec::new();

    // Scan src/modules/
    scan_module_dir(modules_dir, &mut module_names, &mut registerable, &mut has_defaults);

    // Scan imports/ directory and copy .rs files into src/modules/
    let mut imported = Vec::new();
    if imports_dir.exists() {
        if let Ok(entries) = fs::read_dir(imports_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() { continue; }
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if !file_name.ends_with(".rs") { continue; }
                println!("cargo:rerun-if-changed={}", path.display());

                let name = file_name.trim_end_matches(".rs").to_string();
                if module_names.contains(&name) {
                    println!("cargo:warning=Import '{}' conflicts with existing module, skipping", name);
                    continue;
                }

                let dest = modules_dir.join(&file_name);
                if let Err(e) = fs::copy(&path, &dest) {
                    println!("cargo:warning=Failed to copy import {}: {}", file_name, e);
                    continue;
                }
                imported.push(dest.display().to_string());

                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
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

    let import_count = imported.len();
    println!("cargo:warning=Discovered {} modules ({} imported), {} registerable, {} with defaults",
        module_names.len(), import_count, registerable.len(), has_defaults.len());

    let mut c = format!(
        "// Auto-generated module registry\n",
    );
    for name in &module_names {
        c.push_str(&format!("mod {};\n", name));
    }

    c.push_str(r#"
pub mod helpers;

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

fn default_priority(name: &str) -> i32 {
    match name {
        "active_health" => 10,
        "request_id" => 20,
        "rate_limiter" => 30,
        "circuit_breaker" => 40,
        "health_check" => 50,
        "metrics_exporter" => 60,
        "admin_api" => 70,
        "cache" => 80,
        "url_rewriter" => 90,
        "compression" => 100,
        "load_balancer" => 110,
        "proxy_core" => 120,
        "raw_tcp" => 130,
        _ => 75,
    }
}

pub struct Pipeline {
    mods: Vec<(i32, Box<dyn Module>)>,
    raw: Option<Box<dyn RawHandler>>,
    overridden: HashSet<String>,
    to: u64,
}

impl Pipeline {
    pub fn new(t: u64) -> Self {
        Pipeline { mods: Vec::new(), raw: None, overridden: HashSet::new(), to: t }
    }
    pub fn add(&mut self, m: Box<dyn Module>) {
        let p = default_priority(m.name());
        self.add_with_priority(m, p);
    }
    pub fn add_with_priority(&mut self, m: Box<dyn Module>, priority: i32) {
        let name = m.name().to_string();
        if self.overridden.contains(&name) {
            crate::log::module_skipped(&name);
            return;
        }
        for o in m.overrides() {
            self.override_module(o);
        }
        crate::log::module_loaded(&name);
        self.mods.push((priority, m));
    }
    pub fn override_module(&mut self, name: &str) {
        self.overridden.insert(name.to_string());
        self.mods.retain(|(_, m)| m.name() != name);
    }
    pub fn set_raw_handler(&mut self, h: Box<dyn RawHandler>) {
        crate::log::module_loaded("raw connection handler");
        self.raw = Some(h);
    }
    pub fn raw_handler(&self) -> Option<&dyn RawHandler> {
        self.raw.as_deref()
    }
    /// Sort modules by priority (call after all registration is done)
    pub fn sort(&mut self) {
        self.mods.sort_by_key(|(p, _)| *p);
    }
    /// Check if a module with the given name is already loaded
    pub fn has_module(&self, name: &str) -> bool {
        self.mods.iter().any(|(_, m)| m.name() == name)
    }
    /// Get names of all loaded modules
    #[allow(dead_code)]
    pub fn module_names(&self) -> Vec<String> {
        self.mods.iter().map(|(_, m)| m.name().to_string()).collect()
    }
    pub fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> HttpResponse {
        let mut resp_idx = None;
        let mut resp = HttpResponse::error(500, "No handler");
        for (i, (_, m)) in self.mods.iter().enumerate() {
            if let Some(r) = m.handle(r, c) {
                resp = r;
                resp_idx = Some(i);
                break;
            }
        }
        let limit = resp_idx.map(|i| i + 1).unwrap_or(self.mods.len());
        for (_, m) in self.mods[..limit].iter().rev() {
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

