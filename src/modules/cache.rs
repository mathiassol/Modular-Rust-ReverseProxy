// In-memory HTTP response cache
use super::{helpers as h, Module};
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub fn default_config() -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("enabled".into(), toml::Value::Boolean(false));
    t.insert("ttl_seconds".into(), toml::Value::Integer(300));
    t.insert("max_size".into(), toml::Value::Integer(100));
    t.insert("warm_urls".into(), toml::Value::Array(vec![]));
    t
}

pub fn register(ctx: &mut super::ModuleContext) {
    if !h::is_enabled(ctx.config, "cache") { return; }
    let ttl = h::config_u64(ctx.config, "cache", "ttl_seconds", 300);
    let max = h::config_usize(ctx.config, "cache", "max_size", 100);
    let urls = h::config_vec_str(ctx.config, "cache", "warm_urls");
    let cache = Arc::new(Mutex::new(HashMap::new()));
    if !urls.is_empty() {
        warm_cache(Arc::clone(&cache), urls);
    }
    start_eviction_thread(Arc::clone(&cache));
    ctx.pipeline.add(Box::new(Cache { cache, _ttl: ttl, _max: max }));
}

fn start_eviction_thread(cache: Arc<Mutex<HashMap<String, Entry>>>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(30));
            if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            if let Ok(mut m) = cache.lock() {
                let before = m.len();
                let now = Instant::now();
                m.retain(|_, e| now < e.exp);
                let evicted = before - m.len();
                if evicted > 0 {
                    crate::log::info(&format!("cache: evicted {evicted} expired ({} left)", m.len()));
                }
            }
        }
    });
}

fn warm_cache(c: Arc<Mutex<HashMap<String, Entry>>>, urls: Vec<String>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        for u in urls {
            if let Ok(resp) = fetch(&u) {
                if let Ok(mut m) = c.lock() {
                    m.insert(u, Entry { resp, exp: Instant::now() + Duration::from_secs(300) });
                }
            }
        }
    });
}

fn fetch(u: &str) -> Result<HttpResponse, ()> {
    use std::io::Write;
    use std::net::TcpStream;
    let mut s = TcpStream::connect("127.0.0.1:8080").map_err(|_| ())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let req = format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", u);
    s.write_all(req.as_bytes()).map_err(|_| ())?;
    let raw = crate::http::read_http_message(&mut s, 8192);
    match raw {
        crate::http::ReadResult::Ok(d) => HttpResponse::parse(&d).ok_or(()),
        _ => Err(()),
    }
}

struct Cache {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
    _ttl: u64,
    _max: usize,
}

struct Entry {
    resp: HttpResponse,
    exp: Instant,
}

impl Module for Cache {
    fn name(&self) -> &str { "cache" }
    fn handle(&self, r: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
        if r.method != "GET" { return None; }
        let k = r.path.clone();
        let mut m = match self.cache.lock() {
            Ok(guard) => guard,
            Err(_) => {
                crate::log::warn("cache: mutex poisoned, bypassing");
                return None;
            }
        };

        if let Some(e) = m.get(&k) {
            if Instant::now() < e.exp {
                if let Some(tag) = r.get_header("If-None-Match") {
                    if let Some(etag) = e.resp.get_header("ETag") {
                        if tag == etag {
                            let resp = HttpResponse {
                                version: "HTTP/1.1".to_string(),
                                status_code: 304,
                                status_text: "Not Modified".to_string(),
                                headers: vec![("X-Cache".to_string(), "HIT".to_string())],
                                body: Vec::new(),
                            };
                            return Some(resp);
                        }
                    }
                }
                let mut cached = e.resp.clone();
                cached.headers.push(("X-Cache".to_string(), "HIT".to_string()));
                return Some(cached);
            } else {
                m.remove(&k);
            }
        }

        None
    }

    fn on_response(&self, _req: &HttpRequest, resp: &mut HttpResponse, _ctx: &mut Context) {
        if resp.get_header("X-Cache").is_none() && resp.status_code == 200 {
            let key = _req.path.clone();
            if let Ok(mut m) = self.cache.lock() {
                let entry = Entry {
                    resp: resp.clone(),
                    exp: Instant::now() + Duration::from_secs(self._ttl),
                };
                m.insert(key, entry);
            }
        }
    }
}
