// Configuration loading, validation, and default generation
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: Srv,
    #[serde(default)]
    pub modules: HashMap<String, toml::Value>,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct Srv {
    pub listen_addr: String,
    pub backend_addr: String,
    pub buffer_size: usize,
    pub client_timeout: u64,
    pub backend_timeout: u64,
    pub max_header_size: usize,
    pub max_body_size: usize,
    pub max_connections: usize,
    pub worker_threads: usize,
    pub shutdown_timeout: u64,
    pub log_level: String,
    pub logging: bool,
    pub tls_cert: String,
    pub tls_key: String,
    pub http2: bool,
    pub http3: bool,
    pub h3_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config { server: Srv::default(), modules: HashMap::new() }
    }
}

impl Default for Srv {
    fn default() -> Self {
        Srv {
            listen_addr: "127.0.0.1:3000".to_string(),
            backend_addr: "127.0.0.1:8080".to_string(),
            buffer_size: 8192,
            client_timeout: 30,
            backend_timeout: 30,
            max_header_size: 65_536,
            max_body_size: 16 * 1024 * 1024,
            max_connections: 10_000,
            worker_threads: 0,
            shutdown_timeout: 15,
            log_level: "info".to_string(),
            logging: true,
            tls_cert: String::new(),
            tls_key: String::new(),
            http2: true,
            http3: false,
            h3_port: 0,
        }
    }
}

impl Srv {
    pub fn validate(&mut self) -> bool {
        let mut valid = true;

        if self.listen_addr.parse::<std::net::SocketAddr>().is_err() {
            crate::log::error(&format!("listen_addr '{}' is not a valid address (expected ip:port)", self.listen_addr));
            valid = false;
        }
        if self.backend_addr.parse::<std::net::SocketAddr>().is_err() {
            crate::log::error(&format!("backend_addr '{}' is not a valid address (expected ip:port)", self.backend_addr));
            valid = false;
        }

        if self.buffer_size < 1024 {
            crate::log::warn(&format!("buffer_size too small ({}), using 1024", self.buffer_size));
            self.buffer_size = 1024;
        }
        if self.client_timeout == 0 {
            crate::log::warn("client_timeout is 0, using 30");
            self.client_timeout = 30;
        }
        if self.backend_timeout == 0 {
            crate::log::warn("backend_timeout is 0, using 30");
            self.backend_timeout = 30;
        }
        if self.client_timeout < self.backend_timeout {
            crate::log::warn(&format!(
                "client_timeout ({}) < backend_timeout ({}), clients may time out before backend responds",
                self.client_timeout, self.backend_timeout
            ));
        }
        if self.max_body_size == 0 {
            self.max_body_size = 16 * 1024 * 1024;
        }
        if self.max_header_size == 0 {
            self.max_header_size = 65_536;
        }
        if self.max_connections == 0 {
            self.max_connections = 10_000;
        }
        if self.shutdown_timeout == 0 {
            self.shutdown_timeout = 15;
        }
        if self.max_connections > 100_000 {
            crate::log::warn(&format!("max_connections very high ({}), may exhaust file descriptors", self.max_connections));
        }

        if !self.tls_cert.is_empty() || !self.tls_key.is_empty() {
            if self.tls_cert.is_empty() {
                crate::log::error("tls_key is set but tls_cert is missing");
                valid = false;
            } else if self.tls_key.is_empty() {
                crate::log::error("tls_cert is set but tls_key is missing");
                valid = false;
            } else {
                if !std::path::Path::new(&self.tls_cert).exists() {
                    crate::log::error(&format!("tls_cert file not found: {}", self.tls_cert));
                    valid = false;
                }
                if !std::path::Path::new(&self.tls_key).exists() {
                    crate::log::error(&format!("tls_key file not found: {}", self.tls_key));
                    valid = false;
                }
            }
        }

        valid
    }
}

fn atomic_write(path: &str, content: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_config(module_defaults: &HashMap<String, toml::Value>) -> Config {
    let p = path();
    let mut cfg = match fs::read_to_string(&p) {
        Ok(txt) => match toml::from_str(&txt) {
            Ok(c) => {
                crate::log::info(&format!("Loaded {p}"));
                c
            }
            Err(e) => {
                crate::log::error(&format!("Parse error {p}: {e}"));
                crate::log::warn("Using defaults");
                Config::default()
            }
        },
        Err(_) => {
            let mut cfg = Config::default();
            cfg.modules = module_defaults.clone();
            let content = generate_config(&cfg);
            if atomic_write(&p, &content).is_ok() {
                crate::log::info(&format!("Generated {p}"));
            } else {
                crate::log::warn(&format!("No config at '{p}', using defaults"));
            }
            cfg
        }
    };
    if !cfg.server.validate() {
        crate::log::error("Fatal configuration errors — falling back to safe defaults for invalid fields");
        if cfg.server.listen_addr.parse::<std::net::SocketAddr>().is_err() {
            let fallback = "127.0.0.1:3000";
            crate::log::warn(&format!("listen_addr invalid, using {fallback}"));
            cfg.server.listen_addr = fallback.to_string();
        }
        if cfg.server.backend_addr.parse::<std::net::SocketAddr>().is_err() {
            let fallback = "127.0.0.1:8080";
            crate::log::warn(&format!("backend_addr invalid, using {fallback}"));
            cfg.server.backend_addr = fallback.to_string();
        }
        if !cfg.server.tls_cert.is_empty() || !cfg.server.tls_key.is_empty() {
            let cert_ok = !cfg.server.tls_cert.is_empty() && std::path::Path::new(&cfg.server.tls_cert).exists();
            let key_ok = !cfg.server.tls_key.is_empty() && std::path::Path::new(&cfg.server.tls_key).exists();
            if !cert_ok || !key_ok {
                crate::log::warn("TLS config invalid, disabling TLS");
                cfg.server.tls_cert.clear();
                cfg.server.tls_key.clear();
            }
        }
    }
    let mut changed = false;
    for (name, value) in module_defaults {
        cfg.modules.entry(name.clone()).or_insert_with(|| {
            changed = true;
            value.clone()
        });
    }
    if changed {
        let content = generate_config(&cfg);
        if let Err(e) = atomic_write(&p, &content) {
            crate::log::error(&format!("Failed to write config: {e}"));
        } else {
            crate::log::info("Config updated with new module defaults");
        }
    }
    cfg
}

fn generate_config(cfg: &Config) -> String {
    let mut doc = toml::Table::new();
    let mut srv = toml::Table::new();
    srv.insert("listen_addr".into(), toml::Value::String(cfg.server.listen_addr.clone()));
    srv.insert("backend_addr".into(), toml::Value::String(cfg.server.backend_addr.clone()));
    srv.insert("buffer_size".into(), toml::Value::Integer(cfg.server.buffer_size as i64));
    srv.insert("client_timeout".into(), toml::Value::Integer(cfg.server.client_timeout as i64));
    srv.insert("backend_timeout".into(), toml::Value::Integer(cfg.server.backend_timeout as i64));
    srv.insert("max_header_size".into(), toml::Value::Integer(cfg.server.max_header_size as i64));
    srv.insert("max_body_size".into(), toml::Value::Integer(cfg.server.max_body_size as i64));
    srv.insert("max_connections".into(), toml::Value::Integer(cfg.server.max_connections as i64));
    srv.insert("worker_threads".into(), toml::Value::Integer(cfg.server.worker_threads as i64));
    srv.insert("shutdown_timeout".into(), toml::Value::Integer(cfg.server.shutdown_timeout as i64));
    srv.insert("log_level".into(), toml::Value::String(cfg.server.log_level.clone()));
    srv.insert("logging".into(), toml::Value::Boolean(cfg.server.logging));
    srv.insert("tls_cert".into(), toml::Value::String(cfg.server.tls_cert.clone()));
    srv.insert("tls_key".into(), toml::Value::String(cfg.server.tls_key.clone()));
    srv.insert("http2".into(), toml::Value::Boolean(cfg.server.http2));
    srv.insert("http3".into(), toml::Value::Boolean(cfg.server.http3));
    srv.insert("h3_port".into(), toml::Value::Integer(cfg.server.h3_port as i64));
    doc.insert("server".into(), toml::Value::Table(srv));
    let mut mods = toml::Table::new();
    for (name, value) in &cfg.modules {
        mods.insert(name.clone(), value.clone());
    }
    doc.insert("modules".into(), toml::Value::Table(mods));
    match toml::to_string_pretty(&doc) {
        Ok(s) => s,
        Err(e) => {
            crate::log::error(&format!("Config serialization failed: {e}"));
            String::new()
        }
    }
}

fn path() -> String {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == "--config")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "config.toml".to_string())
}
