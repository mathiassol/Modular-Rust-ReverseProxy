// Script module loader - scans mods/ directory and creates modules
use super::parser::{self, Command, ScriptDef};
use super::runtime;
use crate::config::Srv;
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use crate::modules::{Module, Pipeline};
use std::collections::HashMap;
use std::path::Path;

/// Collect default configs from all .pcmod files in mods/
pub fn collect_script_defaults() -> HashMap<String, toml::Value> {
    let mut defaults = HashMap::new();
    let mods_dir = Path::new("mods");
    if !mods_dir.exists() { return defaults; }

    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            if path.extension().map(|e| e == "pcmod").unwrap_or(false) {
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.len() > 1_048_576 => {
                        crate::log::warn(&format!("{}: file too large (>1MB), skipping", path.display()));
                        continue;
                    }
                    Err(e) => {
                        crate::log::warn(&format!("{}: cannot read metadata: {e}", path.display()));
                        continue;
                    }
                    _ => {}
                }
                if let Ok(src) = std::fs::read_to_string(&path) {
                    match parser::parse(&src) {
                        Ok(def) => {
                            let table = parser::default_config_table(&def);
                            defaults.insert(
                                def.name.clone(),
                                toml::Value::Table(table),
                            );
                        }
                        Err(e) => {
                            crate::log::warn(&format!(
                                "Failed to parse {}: {e}",
                                path.display()
                            ));
                        }
                    }
                }
            }
        }
    }
    defaults
}

/// Load all script modules from mods/ and register them with the pipeline
pub fn load_script_modules(
    pipeline: &mut Pipeline,
    config: &HashMap<String, toml::Value>,
    server: &Srv,
) {
    let mods_dir = Path::new("mods");
    if !mods_dir.exists() { return; }

    let mut defs: Vec<(ScriptDef, std::path::PathBuf)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            if path.extension().map(|e| e == "pcmod").unwrap_or(false) {
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.len() > 1_048_576 => {
                        crate::log::warn(&format!("{}: file too large (>1MB), skipping", path.display()));
                        continue;
                    }
                    Err(_) => continue,
                    _ => {}
                }
                if let Ok(src) = std::fs::read_to_string(&path) {
                    match parser::parse(&src) {
                        Ok(def) => defs.push((def, path)),
                        Err(e) => {
                            crate::log::warn(&format!(
                                "Failed to parse {}: {e}",
                                path.display()
                            ));
                        }
                    }
                }
            }
        }
    }

    // Sort by priority
    defs.sort_by_key(|(d, _)| d.priority);

    let mut cache_eviction_started = false;

    for (def, path) in defs {
        let resolved = parser::resolve_config(&def, config);

        // Check enabled flag
        if resolved.get("enabled").map(|v| v == "false").unwrap_or(false) {
            continue;
        }

        // Check if a Rust module with this name is already loaded
        if pipeline.has_module(&def.name) {
            crate::log::info(&format!(
                "script: {} skipped (Rust module loaded)",
                def.name
            ));
            continue;
        }

        crate::log::info(&format!(
            "script: loading {} v{} from {}",
            def.name, def.version, path.display()
        ));

        // Handle overrides
        for o in &def.overrides {
            pipeline.override_module(o);
        }

        // Execute on_init commands
        for cmd in &def.on_init {
            if let Command::StdCall { func, args } = cmd {
                let resolved_args: Vec<String> = args.iter()
                    .map(|a| resolve_init_arg(a, &resolved))
                    .collect();
                super::stdlib::call_init(func, &resolved_args, server, pipeline, &resolved);
            }
        }

        // Start cache eviction if any module uses cache
        if !cache_eviction_started {
            for cmd in &def.on_request {
                if matches!(cmd, Command::StdCall { func, .. } if func == "cache.check") {
                    super::stdlib::start_cache_eviction();
                    cache_eviction_started = true;
                    break;
                }
            }
        }

        // Add as pipeline module if it has request/response handlers
        if !def.on_request.is_empty() || !def.on_response.is_empty() {
            let module = ScriptModule {
                name: def.name.clone(),
                on_request: def.on_request.clone(),
                on_response: def.on_response.clone(),
                config: resolved,
            };
            pipeline.add_with_priority(Box::new(module), def.priority);
        }
    }
}

fn resolve_init_arg(arg: &str, config: &HashMap<String, String>) -> String {
    if let Some(key) = arg.strip_prefix('$') {
        config.get(key).cloned().unwrap_or_default()
    } else {
        arg.to_string()
    }
}

/// A script-based module that executes .pcmod commands
struct ScriptModule {
    name: String,
    on_request: Vec<Command>,
    on_response: Vec<Command>,
    config: HashMap<String, String>,
}

unsafe impl Send for ScriptModule {}
unsafe impl Sync for ScriptModule {}

impl Module for ScriptModule {
    fn name(&self) -> &str {
        &self.name
    }

    fn handle(&self, r: &mut HttpRequest, c: &mut Context) -> Option<HttpResponse> {
        runtime::exec_request(&self.on_request, r, c, &self.config)
    }

    fn on_response(&self, req: &HttpRequest, resp: &mut HttpResponse, ctx: &mut Context) {
        runtime::exec_response(&self.on_response, req, resp, ctx, &self.config);
    }
}
