// Active health checking for backends
use super::helpers as h;
use std::collections::HashMap;
use std::net::{TcpStream, SocketAddr};
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

    let mut valid_backends = Vec::new();
    for b in &backends {
        match b.parse::<SocketAddr>() {
            Ok(_) => valid_backends.push(b.clone()),
            Err(_) => crate::log::warn(&format!("active_health: invalid backend address '{}', skipping", b)),
        }
    }
    if valid_backends.is_empty() {
        crate::log::warn("active_health: no valid backends to monitor");
        return;
    }

    let map: HashMap<String, bool> = valid_backends.iter().map(|b| (b.clone(), true)).collect();
    let health = HEALTH.get_or_init(|| Arc::new(RwLock::new(map)));
    let health = Arc::clone(health);

    let handle = std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            let addrs: Vec<String> = match health.read() {
                Ok(m) => m.keys().cloned().collect(),
                Err(_) => continue,
            };
            let results: Vec<(String, bool)> = addrs.into_iter().map(|addr| {
                let ok = addr.parse::<SocketAddr>().ok().map(|sa| {
                    TcpStream::connect_timeout(&sa, Duration::from_secs(timeout)).is_ok()
                }).unwrap_or(false);
                (addr, ok)
            }).collect();
            if let Ok(mut m) = health.write() {
                for (addr, ok) in results {
                    if let Some(up) = m.get_mut(&addr) {
                        if *up && !ok {
                            crate::log::warn(&format!("active_health: {addr} DOWN"));
                        } else if !*up && ok {
                            crate::log::info(&format!("active_health: {addr} UP"));
                        }
                        *up = ok;
                    }
                }
            }
        }
        crate::log::info("active_health: thread stopped");
    });
    std::thread::spawn(move || {
        if let Err(e) = handle.join() {
            crate::log::error(&format!("active_health: thread panicked: {:?}", e));
        }
    });
}
