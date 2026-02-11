// HTTP request parsing and serialization
use super::{find_hdr_end, get_hdr};

#[derive(Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn parse(r: &[u8]) -> Option<Self> {
        let e = find_hdr_end(r)?;
        let t = std::str::from_utf8(&r[..e]).ok()?;
        let mut l = t.lines();
        let rl = l.next()?;
        let mut p = rl.split_whitespace();
        let m = p.next()?.to_string();
        let path = p.next()?.to_string();
        let v = p.next()?.to_string();
        let mut h = Vec::new();
        for ln in l {
            if ln.is_empty() { break; }
            if let Some((k, val)) = ln.split_once(':') {
                h.push((k.trim().to_string(), val.trim().to_string()));
            }
        }
        let s = e + 4;
        let cl: Option<usize> = get_hdr(&h, "Content-Length").and_then(|v| v.parse().ok());
        let b = match cl {
            Some(len) if s < r.len() => r[s..r.len().min(s + len)].to_vec(),
            _ => Vec::new(),
        };
        Some(HttpRequest { method: m, path, version: v, headers: h, body: b })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = format!("{} {} {}\r\n", self.method, self.path, self.version);
        for (k, v) in &self.headers {
            o.push_str(k);
            o.push_str(": ");
            o.push_str(v);
            o.push_str("\r\n");
        }
        o.push_str("\r\n");
        let mut b = o.into_bytes();
        b.extend_from_slice(&self.body);
        b
    }

    pub fn get_header(&self, n: &str) -> Option<&str> {
        get_hdr(&self.headers, n)
    }

    pub fn set_header(&mut self, n: &str, val: &str) {
        for (k, v) in self.headers.iter_mut() {
            if k.eq_ignore_ascii_case(n) {
                *v = val.to_string();
                return;
            }
        }
        self.headers.push((n.to_string(), val.to_string()));
    }
}
