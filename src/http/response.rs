// HTTP response parsing and serialization
use super::{find_hdr_end, get_hdr};

#[derive(Clone)]
pub struct HttpResponse {
    pub version: String,
    pub status_code: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn parse(r: &[u8]) -> Option<Self> {
        let e = find_hdr_end(r)?;
        let t = std::str::from_utf8(&r[..e]).ok()?;
        let mut l = t.lines();
        let sl = l.next()?;
        let (v, rest) = sl.split_once(' ')?;
        let (cs, txt) = rest.split_once(' ').unwrap_or((rest, ""));
        let c: u16 = cs.parse().ok()?;
        let mut h = Vec::new();
        for ln in l {
            if ln.is_empty() { break; }
            if let Some((k, val)) = ln.split_once(':') {
                h.push((k.trim().to_string(), val.trim().to_string()));
            }
        }
        let s = e + 4;
        let b = if s < r.len() { r[s..].to_vec() } else { Vec::new() };
        Some(HttpResponse { version: v.to_string(), status_code: c, status_text: txt.to_string(), headers: h, body: b })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = format!("{} {} {}\r\n", self.version, self.status_code, self.status_text);
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

    pub fn error(c: u16, m: &str) -> Self {
        let t = match c {
            400 => "Bad Request",
            403 => "Forbidden",
            411 => "Length Required",
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            431 => "Request Header Fields Too Large",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            _ => "Error",
        };
        HttpResponse {
            version: "HTTP/1.1".to_string(),
            status_code: c,
            status_text: t.to_string(),
            headers: vec![
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("Content-Length".to_string(), m.len().to_string()),
                ("Connection".to_string(), "close".to_string()),
            ],
            body: m.as_bytes().to_vec(),
        }
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
