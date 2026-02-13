// STD library - provides all complex proxy functionality to script modules
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use crate::modules::helpers as h;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

struct RateBucket {
    tokens: f64,
    last: Instant,
}

static RATE_BUCKETS: OnceLock<Mutex<HashMap<String, RateBucket>>> = OnceLock::new();

fn rate_buckets() -> &'static Mutex<HashMap<String, RateBucket>> {
    RATE_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct CacheEntry {
    resp: HttpResponse,
    exp: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

static CB_FAILURES: OnceLock<std::sync::atomic::AtomicU64> = OnceLock::new();
static CB_STATE: OnceLock<std::sync::atomic::AtomicU8> = OnceLock::new();
static CB_OPENED: OnceLock<Mutex<Instant>> = OnceLock::new();

fn cb_failures() -> &'static std::sync::atomic::AtomicU64 {
    CB_FAILURES.get_or_init(|| std::sync::atomic::AtomicU64::new(0))
}
fn cb_state() -> &'static std::sync::atomic::AtomicU8 {
    CB_STATE.get_or_init(|| std::sync::atomic::AtomicU8::new(0))
}
fn cb_opened() -> &'static Mutex<Instant> {
    CB_OPENED.get_or_init(|| Mutex::new(Instant::now()))
}

static REQ_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

static HEALTH_MAP: OnceLock<Arc<RwLock<HashMap<String, bool>>>> = OnceLock::new();

fn health_map() -> &'static Arc<RwLock<HashMap<String, bool>>> {
    HEALTH_MAP.get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
}

pub fn call_request(
    func: &str,
    args: &[String],
    req: &mut HttpRequest,
    ctx: &mut Context,
    _config: &HashMap<String, String>,
) -> Option<HttpResponse> {
    match func {
        "rate_limit" => std_rate_limit(args, ctx),
        "cache.check" => std_cache_check(args, req),
        "circuit_breaker.check" => std_cb_check(args),
        "compress.check" => { std_compress_check(req, ctx); None }
        "request_id.inject" => { std_request_id_inject(req, ctx); None }
        "url_rewrite" => { std_url_rewrite(args, req, _config); None }
        "load_balance" => { std_load_balance(args, ctx, _config); None }
        "set_backend" => {
            if let Some(addr) = args.first() {
                ctx.set("_backend_addr", addr.clone());
            }
            None
        }
        "proxy.forward" => std_proxy_forward(args, req, ctx),
        "metrics.prometheus" => std_metrics_prometheus(),
        "health_response" => std_health_response(args),
        _ => {
            crate::log::warn(&format!("std: unknown request function '{func}'"));
            None
        }
    }
}

pub fn call_response(
    func: &str,
    args: &[String],
    req: &HttpRequest,
    resp: &mut HttpResponse,
    ctx: &mut Context,
    _config: &HashMap<String, String>,
) {
    match func {
        "cache.store" => std_cache_store(args, req, resp),
        "circuit_breaker.record" => std_cb_record(args, resp),
        "compress.apply" => std_compress_apply(args, resp, ctx),
        "request_id.propagate" => std_request_id_propagate(resp, ctx),
        _ => {
            crate::log::warn(&format!("std: unknown response function '{func}'"));
        }
    }
}

pub fn call_init(
    func: &str,
    args: &[String],
    server: &crate::config::Srv,
    pipeline: &mut crate::modules::Pipeline,
    config: &HashMap<String, String>,
) {
    match func {
        "active_health" => std_active_health_start(args, config, server),
        "admin_api" => std_admin_api_start(args, server),
        "raw_tcp" => std_raw_tcp_enable(args, server, pipeline, config),
        _ => {
            crate::log::warn(&format!("std: unknown init function '{func}'"));
        }
    }
}

fn std_rate_limit(args: &[String], ctx: &Context) -> Option<HttpResponse> {
    let rps: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let burst: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(rps * 2.0);
    let ip = ctx.get("_client_ip").unwrap_or("?").to_string();

    let mut bs = match rate_buckets().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    if bs.len() > 10_000 {
        let now = Instant::now();
        bs.retain(|_, b| now.duration_since(b.last).as_secs_f64() < 300.0);
    }

    let b = bs.entry(ip).or_insert(RateBucket { tokens: burst, last: Instant::now() });
    let elapsed = b.last.elapsed().as_secs_f64();
    b.tokens = (b.tokens + elapsed * rps).min(burst);
    b.last = Instant::now();

    if b.tokens >= 1.0 {
        b.tokens -= 1.0;
        None
    } else {
        Some(HttpResponse::error(429, "Rate limit"))
    }
}

fn std_cache_check(args: &[String], req: &HttpRequest) -> Option<HttpResponse> {
    if req.method != "GET" { return None; }
    let _ttl: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(300);
    let key = req.path.clone();

    let mut m = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    if let Some(e) = m.get(&key) {
        if Instant::now() < e.exp {
            if let Some(tag) = req.get_header("If-None-Match") {
                if let Some(etag) = e.resp.get_header("ETag") {
                    if tag == etag {
                        return Some(HttpResponse {
                            version: "HTTP/1.1".to_string(),
                            status_code: 304,
                            status_text: "Not Modified".to_string(),
                            headers: vec![("X-Cache".to_string(), "HIT".to_string())],
                            body: Vec::new(),
                        });
                    }
                }
            }
            let mut cached = e.resp.clone();
            cached.headers.push(("X-Cache".to_string(), "HIT".to_string()));
            return Some(cached);
        } else {
            m.remove(&key);
        }
    }
    None
}

fn std_cache_store(args: &[String], req: &HttpRequest, resp: &mut HttpResponse) {
    if resp.get_header("X-Cache").is_some() || resp.status_code != 200 { return; }
    let ttl: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(300);
    let key = req.path.clone();

    let mut m = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    m.insert(key, CacheEntry {
        resp: resp.clone(),
        exp: Instant::now() + Duration::from_secs(ttl),
    });
}

const CB_CLOSED: u8 = 0;
const CB_OPEN: u8 = 1;
const CB_HALF_OPEN: u8 = 2;

fn std_cb_check(args: &[String]) -> Option<HttpResponse> {
    use std::sync::atomic::Ordering;
    let threshold: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
    let recovery: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let _ = threshold;

    let state = cb_state().load(Ordering::Acquire);
    match state {
        CB_OPEN => {
            let elapsed = match cb_opened().lock() {
                Ok(t) => t.elapsed(),
                Err(p) => {
                    let inner = p.into_inner();
                    // Validate: if Instant is in the future (corrupt state), reset to now
                    let now = Instant::now();
                    if *inner > now {
                        crate::log::warn("circuit_breaker: corrupt timestamp after panic, resetting");
                        Duration::from_secs(0)
                    } else {
                        inner.elapsed()
                    }
                }
            };
            if elapsed >= Duration::from_secs(recovery) {
                if cb_state().compare_exchange(CB_OPEN, CB_HALF_OPEN, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    crate::log::info("circuit_breaker: half-open, probing");
                }
                None
            } else {
                crate::metrics::inc_cb_rejects();
                Some(HttpResponse::error(503, "Circuit breaker open"))
            }
        }
        _ => None,
    }
}

fn std_cb_record(args: &[String], resp: &mut HttpResponse) {
    use std::sync::atomic::Ordering;
    let threshold: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
    let state = cb_state().load(Ordering::Acquire);

    if resp.status_code >= 500 {
        let count = cb_failures().fetch_add(1, Ordering::Relaxed) + 1;
        if state == CB_HALF_OPEN {
            if cb_state().compare_exchange(CB_HALF_OPEN, CB_OPEN, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                match cb_opened().lock() {
                    Ok(mut t) => *t = Instant::now(),
                    Err(p) => {
                        crate::log::warn("circuit_breaker: mutex recovered in record, resetting timestamp");
                        *p.into_inner() = Instant::now();
                    }
                }
                crate::metrics::inc_cb_trips();
                crate::log::warn("circuit_breaker: OPEN (probe failed)");
            }
        } else if count >= threshold {
            if cb_state().compare_exchange(CB_CLOSED, CB_OPEN, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                match cb_opened().lock() {
                    Ok(mut t) => *t = Instant::now(),
                    Err(p) => {
                        crate::log::warn("circuit_breaker: mutex recovered in record, resetting timestamp");
                        *p.into_inner() = Instant::now();
                    }
                }
                crate::metrics::inc_cb_trips();
                crate::log::warn(&format!("circuit_breaker: OPEN after {count} failures"));
            }
        }
    } else {
        if state != CB_CLOSED {
            crate::log::info("circuit_breaker: CLOSED, recovered");
        }
        cb_failures().store(0, Ordering::Relaxed);
        cb_state().store(CB_CLOSED, Ordering::Release);
    }
}

fn std_compress_check(req: &HttpRequest, ctx: &mut Context) {
    if let Some(ae) = req.get_header("Accept-Encoding") {
        if ae.contains("gzip") {
            ctx.set("_accepts_gzip", "1".to_string());
        }
    }
}

fn std_compress_apply(args: &[String], resp: &mut HttpResponse, ctx: &Context) {
    if ctx.get("_accepts_gzip").is_none() { return; }
    let min_size: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(256);
    if resp.body.len() < min_size { return; }
    if resp.get_header("Content-Encoding").is_some() { return; }

    let ct = resp.get_header("Content-Type").unwrap_or("");
    if !is_compressible(ct) { return; }

    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    if enc.write_all(&resp.body).is_err() { return; }
    let compressed = match enc.finish() {
        Ok(v) => v,
        Err(_) => return,
    };
    if compressed.len() >= resp.body.len() { return; }

    resp.body = compressed;
    resp.set_header("Content-Encoding", "gzip");
    resp.set_header("Content-Length", &resp.body.len().to_string());
    resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("Transfer-Encoding"));
}

fn is_compressible(ct: &str) -> bool {
    ct.starts_with("text/") || ct.contains("json") || ct.contains("xml")
        || ct.contains("javascript") || ct.contains("svg") || ct.contains("css")
}

fn std_request_id_inject(req: &mut HttpRequest, ctx: &mut Context) {
    let id = req.get_header("X-Request-Id")
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;
            let seq = REQ_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            format!("{ts:x}-{seq:04x}")
        });
    req.set_header("X-Request-Id", &id);
    ctx.set("_request_id", id);
}

fn std_request_id_propagate(resp: &mut HttpResponse, ctx: &Context) {
    if let Some(id) = ctx.get("_request_id") {
        resp.headers.push(("X-Request-Id".to_string(), id.to_string()));
    }
}

fn std_url_rewrite(args: &[String], req: &mut HttpRequest, config: &HashMap<String, String>) {
    if args.len() >= 2 {
        let from = &args[0];
        let to = &args[1];
        if req.path.starts_with(from.as_str()) {
            req.path = req.path.replacen(from.as_str(), to, 1);
        }
        return;
    }

    if let Some(rules_str) = args.first().and_then(|k| {
        let k = k.strip_prefix('$').unwrap_or(k);
        config.get(k)
    }) {
        for rule in rules_str.split(';') {
            if let Some((from, to)) = rule.split_once(':') {
                if req.path.starts_with(from) {
                    req.path = req.path.replacen(from, to, 1);
                    break;
                }
            }
        }
    }
}

static LB_INDEX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn std_load_balance(args: &[String], ctx: &mut Context, config: &HashMap<String, String>) {
    let backends_str = args.first()
        .map(|a| {
            let key = a.strip_prefix('$').unwrap_or(a);
            config.get(key).cloned().unwrap_or_else(|| a.clone())
        })
        .unwrap_or_default();

    let backends: Vec<&str> = backends_str.split(',').filter(|s| !s.is_empty()).collect();
    if backends.is_empty() { return; }

    let len = backends.len();
    let start = LB_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % len;

    for offset in 0..len {
        let addr = backends[(start + offset) % len];
        if is_backend_healthy(addr) {
            ctx.set("_backend_addr", addr.to_string());
            return;
        }
    }
    ctx.set("_backend_addr", backends[start % len].to_string());
}

fn is_backend_healthy(addr: &str) -> bool {
    health_map().read().ok()
        .and_then(|m| m.get(addr).copied())
        .unwrap_or(true)
}

fn std_proxy_forward(
    _args: &[String],
    req: &mut HttpRequest,
    ctx: &mut Context,
) -> Option<HttpResponse> {
    let addr = ctx.get("_backend_addr")?;
    let sock_addr: std::net::SocketAddr = match addr.parse() {
        Ok(a) => a,
        Err(_) => return Some(HttpResponse::error(502, "Invalid backend address")),
    };

    let timeout = Duration::from_secs(30);
    let pool = crate::pool::global_pool();

    let mut s = match pool.get(&sock_addr, timeout) {
        Ok(s) => s,
        Err(_) => return Some(HttpResponse::error(502, "Backend unavailable")),
    };

    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));

    use std::io::Write;
    if let Err(e) = s.write_all(&req.to_bytes()) {
        crate::log::warn(&format!("std.proxy: backend write error: {e}"));
        return Some(HttpResponse::error(502, "Backend write failed"));
    }

    match crate::http::read_http_message(&mut s, 8192) {
        crate::http::ReadResult::Ok(d) => {
            match HttpResponse::parse(&d) {
                Some(parsed) => {
                    let conn_hdr = parsed.get_header("Connection").unwrap_or("");
                    let keep_alive = if parsed.version == "HTTP/1.0" {
                        conn_hdr.eq_ignore_ascii_case("keep-alive")
                    } else {
                        !conn_hdr.eq_ignore_ascii_case("close")
                    };
                    if keep_alive {
                        pool.put(sock_addr, s);
                    }
                    Some(parsed)
                }
                None => Some(HttpResponse::error(502, "Parse failed")),
            }
        }
        crate::http::ReadResult::TimedOut => Some(HttpResponse::error(504, "Backend timeout")),
        crate::http::ReadResult::Error(e) => {
            crate::log::warn(&format!("std.proxy: backend error: {e}"));
            Some(HttpResponse::error(502, "Backend error"))
        }
    }
}

fn std_metrics_prometheus() -> Option<HttpResponse> {
    let body = crate::metrics::snapshot_prometheus();
    Some(HttpResponse {
        version: "HTTP/1.1".to_string(),
        status_code: 200,
        status_text: "OK".to_string(),
        headers: vec![
            ("Content-Type".to_string(), "text/plain; version=0.0.4; charset=utf-8".to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        body: body.into_bytes(),
    })
}

fn std_health_response(args: &[String]) -> Option<HttpResponse> {
    let body = args.first().cloned().unwrap_or_else(|| r#"{"status":"ok"}"#.to_string());
    Some(h::json_response(200, &body))
}

fn std_active_health_start(args: &[String], config: &HashMap<String, String>, server: &crate::config::Srv) {
    let interval: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);
    let timeout: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);

    let backends_str = config.get("backends").cloned().unwrap_or_default();
    let mut backends: Vec<String> = backends_str.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if backends.is_empty() {
        backends.push(server.backend_addr.clone());
    }

    let map: HashMap<String, bool> = backends.iter().map(|b| (b.clone(), true)).collect();
    let health = health_map();
    if let Ok(mut m) = health.write() {
        *m = map;
    }
    let health = Arc::clone(health);

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) { break; }
            if let Ok(mut m) = health.write() {
                for (addr, up) in m.iter_mut() {
                    let ok = std::net::TcpStream::connect_timeout(
                        &addr.parse().unwrap_or_else(|_| ([127, 0, 0, 1], 80).into()),
                        Duration::from_secs(timeout),
                    ).is_ok();
                    if *up && !ok {
                        crate::log::warn(&format!("std.active_health: {addr} DOWN"));
                    } else if !*up && ok {
                        crate::log::info(&format!("std.active_health: {addr} UP"));
                    }
                    *up = ok;
                }
            }
        }
    });
}

fn std_admin_api_start(args: &[String], server: &crate::config::Srv) {
    let listen = args.first().cloned().unwrap_or_else(|| "127.0.0.1:9090".to_string());
    let api_key = args.get(1).cloned().unwrap_or_default();

    let listener = match std::net::TcpListener::bind(&listen) {
        Ok(l) => l,
        Err(e) => {
            crate::log::error(&format!("std.admin_api: {e}"));
            return;
        }
    };
    if api_key.is_empty() {
        crate::log::warn("std.admin_api: no api_key set, endpoints are unprotected");
    }
    crate::log::module_loaded(&format!("admin_api ({listen})"));

    let info = Arc::new(AdminInfo {
        start: Instant::now(),
        listen: server.listen_addr.clone(),
        backend: server.backend_addr.clone(),
        max_conns: server.max_connections,
        api_key,
    });

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let i = Arc::clone(&info);
            std::thread::spawn(move || admin_handle(conn, &i));
        }
    });
}

struct AdminInfo {
    start: Instant,
    listen: String,
    backend: String,
    max_conns: usize,
    api_key: String,
}

fn admin_handle(mut s: std::net::TcpStream, info: &AdminInfo) {
    use std::io::Read;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = match s.read(&mut buf) { Ok(n) if n > 0 => n, _ => return };
    let raw = String::from_utf8_lossy(&buf[..n]);
    let line = match raw.lines().next() { Some(l) if !l.is_empty() => l, _ => return };
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 { return; }
    let (_method, path) = (parts[0], parts[1]);

    if !info.api_key.is_empty() && path != "/ping" {
        let provided = raw.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("X-API-Key") { Some(v.trim()) } else { None }
        }).unwrap_or("");
        if provided != info.api_key {
            admin_respond(&mut s, 403, r#"{"error":"unauthorized"}"#);
            return;
        }
    }

    match path {
        "/ping" => admin_respond(&mut s, 200, r#"{"ping":"pong"}"#),
        "/status" => {
            let up = info.start.elapsed().as_secs();
            let (d, hr, mi, sc) = (up / 86400, (up % 86400) / 3600, (up % 3600) / 60, up % 60);
            let body = format!(
                r#"{{"status":"running","uptime_seconds":{up},"uptime":"{d}d {hr}h {mi}m {sc}s","listen":"{}","backend":"{}","pid":{},"active_connections":{},"max_connections":{}}}"#,
                info.listen, info.backend, std::process::id(), crate::server::active_connections(), info.max_conns,
            );
            admin_respond(&mut s, 200, &body);
        }
        "/metrics" => admin_respond(&mut s, 200, &crate::metrics::snapshot_json()),
        "/stop" => { admin_respond(&mut s, 200, r#"{"action":"stopping"}"#); crate::server::request_shutdown(); }
        "/reload" => {
            admin_respond(&mut s, 200, r#"{"action":"reloading"}"#);
            let _ = std::fs::write(".proxycache-reload", "");
            crate::server::request_shutdown();
        }
        _ => admin_respond(&mut s, 404, r#"{"error":"not found"}"#),
    }
}

fn admin_respond(s: &mut std::net::TcpStream, code: u16, body: &str) {
    use std::io::Write;
    let status = match code { 200 => "OK", 403 => "Forbidden", 404 => "Not Found", _ => "Error" };
    let r = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(r.as_bytes());
}

fn std_raw_tcp_enable(
    _args: &[String],
    server: &crate::config::Srv,
    pipeline: &mut crate::modules::Pipeline,
    config: &HashMap<String, String>,
) {
    let overrides = ["proxy_core", "load_balancer", "health_check", "url_rewriter", "cache"];
    for name in &overrides {
        pipeline.override_module(name);
    }

    let backend = config.get("backend_addr").cloned().unwrap_or_else(|| server.backend_addr.clone());
    let buf: usize = config.get("buffer_size").and_then(|s| s.parse().ok()).unwrap_or(server.buffer_size);
    let timeout: u64 = config.get("timeout").and_then(|s| s.parse().ok()).unwrap_or(server.client_timeout);

    pipeline.set_raw_handler(Box::new(ScriptRawTcp { backend, buf, timeout }));
}

struct ScriptRawTcp {
    backend: String,
    buf: usize,
    timeout: u64,
}

impl crate::modules::RawHandler for ScriptRawTcp {
    fn handle_raw(&self, client: std::net::TcpStream) {
        let ip = client.peer_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "?".into());
        let _ = client.set_read_timeout(Some(Duration::from_secs(self.timeout)));
        let _ = client.set_write_timeout(Some(Duration::from_secs(self.timeout)));
        let backend = match std::net::TcpStream::connect(&self.backend) {
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

pub fn start_cache_eviction() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) { break; }
            let mut m = match cache().lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let before = m.len();
            let now = Instant::now();
            m.retain(|_, e| now < e.exp);
            let evicted = before - m.len();
            if evicted > 0 {
                crate::log::info(&format!("std.cache: evicted {evicted} ({} left)", m.len()));
            }
        }
    });
}

pub fn list_loaded_mods() -> Vec<(String, String, bool)> {
    let mods_dir = std::path::Path::new("mods");
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "pcmod").unwrap_or(false) {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    if let Ok(def) = super::parser::parse(&src) {
                        let enabled = def.config.iter()
                            .find(|f| f.key == "enabled")
                            .map(|f| matches!(f.default, super::parser::FieldValue::Bool(true)))
                            .unwrap_or(true);
                        result.push((def.name, def.version, enabled));
                    }
                }
            }
        }
    }
    result
}
