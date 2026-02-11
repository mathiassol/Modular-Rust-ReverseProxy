// Active health checking for backends
use super::helpers as h;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, RwLock, OnceLock};
use std::time::Duration;

static HEALTH: OnceLock<Arc<RwLock<HashMap<String, bool>>>> = OnceLock::new();

pub fn is_healthy(addr: &str) -> bool {
    HEALTH.get()
        .and_then(|m| m.read().ok())
        .and_then(|m| m.get(addr).copied())
        .unwrap_or(true)
}

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("interval".into(), toml::Value::Integer(10));
    t.insert("timeout".into(), toml::Value::Integer(3));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "active_health") { return; }
    let interval = h::config_u64(ctx.config, "active_health", "interval", 10);
    let timeout = h::config_u64(ctx.config, "active_health", "timeout", 3);

    let mut backends = h::config_vec_str(ctx.config, "load_balancer", "backends");
    if backends.is_empty() {
        backends.push(ctx.server.backend_addr.clone());
    }

    let map: HashMap<String, bool> = backends.iter().map(|b| (b.clone(), true)).collect();
    let health = HEALTH.get_or_init(|| Arc::new(RwLock::new(map)));
    let health = Arc::clone(health);

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if let Ok(mut m) = health.write() {
                for (addr, up) in m.iter_mut() {
                    let ok = TcpStream::connect_timeout(
                        &addr.parse().unwrap_or_else(|_| ([127,0,0,1], 80).into()),
                        Duration::from_secs(timeout),
                    ).is_ok();
                    if *up && !ok {
                        crate::log::warn(&format!("active_health: {addr} DOWN"));
                    } else if !*up && ok {
                        crate::log::info(&format!("active_health: {addr} UP"));
                    }
                    *up = ok;
                }
            }
        }
    });
}
