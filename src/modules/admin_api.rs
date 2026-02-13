// Admin API for proxy management
use super::helpers as h;
use crate::server;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const MAX_ADMIN_CONNECTIONS: usize = 16;

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(true));
    t.insert("listen_addr".into(), toml::Value::String("127.0.0.1:9090".into()));
    t.insert("api_key".into(), toml::Value::String("".into()));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "admin_api") { return; }
    let addr = h::config_str(ctx.config, "admin_api", "listen_addr", "127.0.0.1:9090");
    let api_key = h::config_str(ctx.config, "admin_api", "api_key", "");
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            crate::log::error(&format!("admin_api: {e}"));
            return;
        }
    };
    if api_key.is_empty() {
        crate::log::warn("admin_api: no api_key set, endpoints are unprotected");
    }
    crate::log::module_loaded(&format!("admin_api ({addr})"));
    let info = Arc::new(Info {
        start: Instant::now(),
        listen: ctx.server.listen_addr.clone(),
        backend: ctx.server.backend_addr.clone(),
        max_conns: ctx.server.max_connections,
        api_key,
        tls_enabled: !ctx.server.tls_cert.is_empty() && !ctx.server.tls_key.is_empty(),
        tls_cert: ctx.server.tls_cert.clone(),
        tls_key: ctx.server.tls_key.clone(),
        http2: ctx.server.http2,
        http3: ctx.server.http3,
        h3_port: ctx.server.h3_port,
        buffer_size: ctx.server.buffer_size,
        client_timeout: ctx.server.client_timeout,
        backend_timeout: ctx.server.backend_timeout,
        max_header_size: ctx.server.max_header_size,
        max_body_size: ctx.server.max_body_size,
        worker_threads: ctx.server.worker_threads,
        shutdown_timeout: ctx.server.shutdown_timeout,
        log_level: ctx.server.log_level.clone(),
        logging: ctx.server.logging,
    });
    let active_admin = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) { break; }
            let current = active_admin.load(Ordering::Acquire);
            if current >= MAX_ADMIN_CONNECTIONS {
                crate::log::warn("admin_api: connection limit reached, rejecting");
                drop(conn);
                continue;
            }
            let i = Arc::clone(&info);
            let counter = Arc::clone(&active_admin);
            counter.fetch_add(1, Ordering::AcqRel);
            thread::spawn(move || {
                handle(conn, &i);
                counter.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
}

struct Info {
    start: Instant,
    listen: String,
    backend: String,
    max_conns: usize,
    api_key: String,
    tls_enabled: bool,
    tls_cert: String,
    tls_key: String,
    http2: bool,
    http3: bool,
    h3_port: u16,
    buffer_size: usize,
    client_timeout: u64,
    backend_timeout: u64,
    max_header_size: usize,
    max_body_size: usize,
    worker_threads: usize,
    shutdown_timeout: u64,
    log_level: String,
    logging: bool,
}

fn extract_header<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    for line in raw.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

const MAX_ADMIN_REQUEST: usize = 65_536;

fn handle(mut s: TcpStream, info: &Info) {
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = vec![0u8; 4096];
    let mut total = 0usize;
    loop {
        if total >= MAX_ADMIN_REQUEST {
            respond(&mut s, 413, r#"{"error":"request too large"}"#);
            return;
        }
        let n = match s.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                       || e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(_) => return,
        };
        total += n;
        if let Some(hdr_end) = buf[..total].windows(4).position(|w| w == b"\r\n\r\n") {
            let header_part = String::from_utf8_lossy(&buf[..hdr_end]);
            let content_len: usize = header_part.lines().find_map(|line| {
                let (k, v) = line.split_once(':')?;
                if k.trim().eq_ignore_ascii_case("Content-Length") {
                    v.trim().parse().ok()
                } else { None }
            }).unwrap_or(0);
            let body_start = hdr_end + 4;
            let body_needed = body_start + content_len;
            if body_needed > MAX_ADMIN_REQUEST {
                respond(&mut s, 413, r#"{"error":"request too large"}"#);
                return;
            }
            if total >= body_needed { break; }
            if buf.len() < body_needed {
                buf.resize(body_needed, 0);
            }
            continue;
        }
        if total == buf.len() {
            if buf.len() >= MAX_ADMIN_REQUEST { break; }
            buf.resize((buf.len() * 2).min(MAX_ADMIN_REQUEST), 0);
        }
    }
    if total == 0 { return; }
    let raw = String::from_utf8_lossy(&buf[..total]);
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

    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    crate::log::info(&format!("admin_api: {method} {path} from {peer}"));

    if !info.api_key.is_empty() && path != "/ping" {
        let provided = extract_header(&raw, "X-API-Key").unwrap_or("");
        if !constant_time_eq(provided.as_bytes(), info.api_key.as_bytes()) {
            crate::log::warn(&format!("admin_api: unauthorized access from {peer}"));
            respond(&mut s, 403, r#"{"error":"unauthorized"}"#);
            return;
        }
    }

    match (method, path) {
        ("GET", "/") => {
            respond(&mut s, 200, r#"{"endpoints":["/ping","/status","/config","/server","/stop","/reload","/connections","/metrics","/mods","/protocols","/tls","/config/verify","/config/repair"]}"#);
        }
        ("GET", "/ping") => {
            respond(&mut s, 200, r#"{"ping":"pong"}"#);
        }
        ("GET", "/status") => {
            let up = info.start.elapsed().as_secs();
            let (d, h, m, sec) = (up / 86400, (up % 86400) / 3600, (up % 3600) / 60, up % 60);
            let pid = std::process::id();
            let active = server::active_connections();
            let snap = crate::metrics::snapshot();
            let scheme = if info.tls_enabled { "https" } else { "http" };
            let mut protocols = vec!["HTTP/1.1"];
            if info.tls_enabled && info.http2 { protocols.push("HTTP/2"); }
            if info.tls_enabled && info.http3 { protocols.push("HTTP/3"); }
            let body = format!(
                r#"{{"status":"running","uptime_seconds":{up},"uptime":"{d}d {h}h {m}m {sec}s","listen":"{l}","backend":"{b}","scheme":"{scheme}","protocols":"{protos}","pid":{pid},"active_connections":{active},"max_connections":{mc},"requests_total":{rt},"requests_ok":{ro},"requests_err":{re},"bytes_in":{bi},"bytes_out":{bo},"avg_latency_ms":{lat}}}"#,
                l = info.listen, b = info.backend, mc = info.max_conns,
                protos = protocols.join(", "),
                rt = snap.requests_total, ro = snap.requests_ok, re = snap.requests_err,
                bi = snap.bytes_in, bo = snap.bytes_out, lat = snap.avg_latency_ms(),
            );
            respond(&mut s, 200, &body);
        }
        ("GET", "/connections") => {
            let active = server::active_connections();
            let snap = crate::metrics::snapshot();
            respond(&mut s, 200, &format!(
                r#"{{"active":{active},"max":{},"total_connections":{}}}"#,
                info.max_conns, snap.connections_total
            ));
        }
        ("GET", "/metrics") => {
            respond(&mut s, 200, &crate::metrics::snapshot_json());
        }
        ("GET", "/config") => {
            respond(&mut s, 200, &full_config_json(info));
        }
        ("GET", "/server") => {
            respond(&mut s, 200, &server_config_json(info));
        }
        ("GET", "/protocols") => {
            respond(&mut s, 200, &protocols_json(info));
        }
        ("GET", "/tls") => {
            respond(&mut s, 200, &tls_json(info));
        }
        ("GET", "/mods") => {
            respond(&mut s, 200, &mods_list());
        }
        ("GET", "/config/verify") => {
            respond(&mut s, 200, &config_verify());
        }
        ("POST", "/config/repair") => {
            respond(&mut s, 200, &config_repair());
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        let mut _acc: u8 = 1;
        for i in 0..a.len().max(b.len()) {
            let x = a.get(i).copied().unwrap_or(0);
            let y = b.get(i).copied().unwrap_or(0);
            _acc |= x ^ y;
        }
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn respond(s: &mut TcpStream, code: u16, body: &str) {
    let status = match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let r = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(r.as_bytes());
}

fn server_config_json(info: &Info) -> String {
    format!(
        r#"{{"listen_addr":"{la}","backend_addr":"{ba}","buffer_size":{bs},"client_timeout":{ct},"backend_timeout":{bt},"max_header_size":{mh},"max_body_size":{mb},"max_connections":{mc},"worker_threads":{wt},"shutdown_timeout":{st},"log_level":"{ll}","logging":{lo},"tls_cert":"{tc}","tls_key":"{tk}","http2":{h2},"http3":{h3},"h3_port":{hp}}}"#,
        la = info.listen, ba = info.backend, bs = info.buffer_size,
        ct = info.client_timeout, bt = info.backend_timeout,
        mh = info.max_header_size, mb = info.max_body_size,
        mc = info.max_conns, wt = info.worker_threads,
        st = info.shutdown_timeout, ll = info.log_level, lo = info.logging,
        tc = info.tls_cert, tk = info.tls_key,
        h2 = info.http2, h3 = info.http3, hp = info.h3_port,
    )
}

fn full_config_json(info: &Info) -> String {
    let server = server_config_json(info);
    let mods = mods_list();
    format!(r#"{{"server":{server},"modules":{mods}}}"#)
}

fn protocols_json(info: &Info) -> String {
    let _h1 = true;
    let h2 = info.tls_enabled && info.http2;
    let h3 = info.tls_enabled && info.http3;
    let h3_port = if info.h3_port > 0 { info.h3_port } else {
        info.listen.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(0)
    };
    format!(
        r#"{{"http1":{{  "enabled":true,"port":"{la}"}},"http2":{{"enabled":{h2},"requires_tls":true,"alpn":"h2"}},"http3":{{"enabled":{h3},"requires_tls":true,"transport":"QUIC","port":{h3p}}},"tls_enabled":{tls}}}"#,
        la = info.listen, h2 = h2, h3 = h3, h3p = h3_port, tls = info.tls_enabled,
    )
}

fn tls_json(info: &Info) -> String {
    if !info.tls_enabled {
        return r#"{"enabled":false}"#.to_string();
    }
    let cert_exists = std::path::Path::new(&info.tls_cert).exists();
    let key_exists = std::path::Path::new(&info.tls_key).exists();
    let mut alpn = vec![];
    if info.http2 { alpn.push("h2"); }
    alpn.push("http/1.1");
    let alpn_str = alpn.join(", ");
    format!(
        r#"{{"enabled":true,"cert_path":"{}","key_path":"{}","cert_exists":{},"key_exists":{},"alpn_protocols":"{}","session_cache_size":2048}}"#,
        info.tls_cert, info.tls_key, cert_exists, key_exists, alpn_str,
    )
}

fn mods_list() -> String {
    use std::fmt::Write;

    let script_mods = crate::script::stdlib::list_loaded_mods();
    let mut out = String::from(r#"{"rust_modules":["#);

    // List Rust modules from src/modules directory
    let rust_dir = std::path::Path::new("src/modules");
    let mut rust_names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(rust_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                    if name != "mod" && name != "helpers" {
                        rust_names.push(name.to_string());
                    }
                }
            }
        }
    }
    rust_names.sort();
    for (i, name) in rust_names.iter().enumerate() {
        if i > 0 { out.push(','); }
        let _ = write!(out, r#""{}""#, name);
    }
    out.push_str(r#"],"script_modules":["#);

    for (i, (name, ver, enabled)) in script_mods.iter().enumerate() {
        if i > 0 { out.push(','); }
        let _ = write!(out, r#"{{"name":"{}","version":"{}","enabled":{}}}"#, name, ver, enabled);
    }
    out.push_str("]}");
    out
}

fn config_verify() -> String {
    let path = std::path::Path::new("config.toml");
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!(r#"{{"ok":false,"error":"Cannot read config.toml: {}"}}"#, e),
    };

    match content.parse::<toml::Table>() {
        Err(e) => format!(r#"{{"ok":false,"error":"Parse error: {}"}}"#, e),
        Ok(table) => {
            let mut issues: Vec<String> = Vec::new();

            // Check [server] section
            if !table.contains_key("server") {
                issues.push("missing [server] section".to_string());
            } else if let Some(srv) = table.get("server").and_then(|v| v.as_table()) {
                for key in &["listen_addr", "backend_addr"] {
                    if !srv.contains_key(*key) {
                        issues.push(format!("server.{} missing", key));
                    }
                }
            }

            // Check [modules] section exists
            if !table.contains_key("modules") {
                issues.push("missing [modules] section".to_string());
            }

            // Check script module configs
            let script_mods = crate::script::stdlib::list_loaded_mods();
            if let Some(modules) = table.get("modules").and_then(|v| v.as_table()) {
                for (name, _, _) in &script_mods {
                    if !modules.contains_key(name) {
                        issues.push(format!("missing config for script module '{}'", name));
                    }
                }
            }

            if issues.is_empty() {
                r#"{"ok":true,"issues":[]}"#.to_string()
            } else {
                let issues_json: Vec<String> = issues.iter()
                    .map(|i| format!(r#""{}""#, i))
                    .collect();
                format!(r#"{{"ok":false,"issues":[{}]}}"#, issues_json.join(","))
            }
        }
    }
}

fn config_repair() -> String {
    let path = std::path::Path::new("config.toml");
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => String::new(),
    };

    let mut table: toml::Table = content.parse().unwrap_or_default();
    let mut fixes: Vec<String> = Vec::new();

    // Ensure [server] section
    if !table.contains_key("server") {
        let mut srv = toml::Table::new();
        srv.insert("listen_addr".into(), toml::Value::String("127.0.0.1:8080".into()));
        srv.insert("backend_addr".into(), toml::Value::String("127.0.0.1:3000".into()));
        table.insert("server".into(), toml::Value::Table(srv));
        fixes.push("added [server] section with defaults".to_string());
    }

    // Ensure [modules] section
    if !table.contains_key("modules") {
        table.insert("modules".into(), toml::Value::Table(toml::Table::new()));
        fixes.push("added [modules] section".to_string());
    }

    // Add missing script module defaults
    let mods_dir = std::path::Path::new("mods");
    if mods_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(mods_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map(|e| e == "pcmod").unwrap_or(false) {
                    if let Ok(src) = std::fs::read_to_string(&p) {
                        if let Ok(def) = crate::script::parser::parse(&src) {
                            if let Some(modules) = table.get_mut("modules").and_then(|v| v.as_table_mut()) {
                                if !modules.contains_key(&def.name) {
                                    let defaults = crate::script::parser::default_config_table(&def);
                                    modules.insert(def.name.clone(), toml::Value::Table(defaults));
                                    fixes.push(format!("added defaults for module '{}'", def.name));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Write back
    if !fixes.is_empty() {
        match toml::to_string_pretty(&table) {
            Ok(new_content) => {
                if let Err(e) = std::fs::write(path, &new_content) {
                    return format!(r#"{{"ok":false,"error":"Failed to write config.toml: {}"}}"#, e);
                }
            }
            Err(e) => {
                return format!(r#"{{"ok":false,"error":"Failed to serialize config: {}"}}"#, e);
            }
        }
    }

    let fixes_json: Vec<String> = fixes.iter().map(|f| format!(r#""{}""#, f)).collect();
    format!(r#"{{"ok":true,"fixes":[{}]}}"#, fixes_json.join(","))
}
