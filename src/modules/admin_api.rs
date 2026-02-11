// Admin API for proxy management
use super::helpers as h;
use crate::server;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(true));
    t.insert("listen_addr".into(), toml::Value::String("127.0.0.1:9090".into()));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "admin_api") { return; }
    let addr = h::config_str(ctx.config, "admin_api", "listen_addr", "127.0.0.1:9090");
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            crate::log::error(&format!("admin_api: {e}"));
            return;
        }
    };
    crate::log::module_loaded(&format!("admin_api ({addr})"));
    let info = Arc::new(Info {
        start: Instant::now(),
        listen: ctx.server.listen_addr.clone(),
        backend: ctx.server.backend_addr.clone(),
        max_conns: ctx.server.max_connections,
    });
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let i = Arc::clone(&info);
            thread::spawn(move || handle(conn, &i));
        }
    });
}

struct Info {
    start: Instant,
    listen: String,
    backend: String,
    max_conns: usize,
}

fn handle(mut s: TcpStream, info: &Info) {
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = match s.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let raw = String::from_utf8_lossy(&buf[..n]);
    let line = match raw.lines().next() {
        Some(l) if !l.is_empty() => l,
        _ => { respond(&mut s, 400, r#"{"error":"empty request"}"#); return; }
    };

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        respond(&mut s, 400, r#"{"error":"malformed request line"}"#);
        return;
    }
    let method = parts[0];
    let path = parts[1];

    if !matches!(method, "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS") {
        respond(&mut s, 405, r#"{"error":"method not allowed"}"#);
        return;
    }

    if !path.starts_with('/') {
        respond(&mut s, 400, r#"{"error":"invalid path"}"#);
        return;
    }

    match (method, path) {
        ("GET", "/") => {
            respond(&mut s, 200, r#"{"endpoints":["/ping","/status","/config","/stop","/reload","/connections","/metrics"]}"#);
        }
        ("GET", "/ping") => {
            respond(&mut s, 200, r#"{"ping":"pong"}"#);
        }
        ("GET", "/status") => {
            let up = info.start.elapsed().as_secs();
            let (d, h, m, sec) = (up / 86400, (up % 86400) / 3600, (up % 3600) / 60, up % 60);
            let pid = std::process::id();
            let active = server::active_connections();
            let body = format!(
                r#"{{"status":"running","uptime_seconds":{up},"uptime":"{d}d {h}h {m}m {sec}s","listen":"{l}","backend":"{b}","pid":{pid},"active_connections":{active},"max_connections":{mc}}}"#,
                l = info.listen, b = info.backend, mc = info.max_conns,
            );
            respond(&mut s, 200, &body);
        }
        ("GET", "/connections") => {
            let active = server::active_connections();
            respond(&mut s, 200, &format!(r#"{{"active":{active},"max":{}}}"#, info.max_conns));
        }
        ("GET", "/metrics") => {
            respond(&mut s, 200, &crate::metrics::snapshot_json());
        }
        ("GET", "/config") => {
            let body = format!(
                r#"{{"listen":"{l}","backend":"{b}"}}"#,
                l = info.listen, b = info.backend,
            );
            respond(&mut s, 200, &body);
        }
        ("POST", "/stop") => {
            respond(&mut s, 200, r#"{"action":"stopping"}"#);
            let _ = s.flush();
            server::request_shutdown();
        }
        ("POST", "/reload") => {
            respond(&mut s, 200, r#"{"action":"reloading"}"#);
            let _ = s.flush();
            let _ = std::fs::write(".proxycache-reload", "");
            server::request_shutdown();
        }
        _ => {
            respond(&mut s, 404, r#"{"error":"not found"}"#);
        }
    }
}

fn respond(s: &mut TcpStream, code: u16, body: &str) {
    let status = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let r = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(r.as_bytes());
}
