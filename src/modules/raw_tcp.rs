// Raw TCP passthrough mode
use super::{helpers as h, RawHandler};
use std::net::TcpStream;
use std::time::Duration;

const OVERRIDES: [&str; 5] = [
    "proxy_core",
    "load_balancer",
    "health_check",
    "url_rewriter",
    "cache",
];

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "raw_tcp") { return; }
    for name in &OVERRIDES {
        ctx.pipeline.override_module(name);
    }
    let backend = h::config_str(ctx.config, "raw_tcp", "backend_addr", &ctx.server.backend_addr);
    let buf = h::config_usize(ctx.config, "raw_tcp", "buffer_size", ctx.server.buffer_size);
    let timeout = h::config_u64(ctx.config, "raw_tcp", "timeout", ctx.server.client_timeout);
    ctx.pipeline.set_raw_handler(Box::new(RawTcp { backend, buf, timeout }));
}

struct RawTcp {
    backend: String,
    buf: usize,
    timeout: u64,
}

impl RawHandler for RawTcp {
    fn handle_raw(&self, client: TcpStream) {
        let ip = client.peer_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "?".into());
        let _ = client.set_read_timeout(Some(Duration::from_secs(self.timeout)));
        let _ = client.set_write_timeout(Some(Duration::from_secs(self.timeout)));
        let backend = match TcpStream::connect(&self.backend) {
            Ok(s) => s,
            Err(e) => {
                crate::log::error(&format!("{ip} → backend connect failed: {e}"));
                return;
            }
        };
        let _ = backend.set_read_timeout(Some(Duration::from_secs(self.timeout)));
        let _ = backend.set_write_timeout(Some(Duration::from_secs(self.timeout)));
        crate::log::request("TCP", &self.backend, &ip);
        h::bidirectional_stream(client, backend, self.buf);
        crate::log::info(&format!("TCP {ip} closed"));
        crate::log::separator();
    }
}
