// HTTP message parsing and I/O operations
mod request;
mod response;
pub use request::HttpRequest;
pub use response::HttpResponse;
use std::io::Read;

pub const MAX_HEADER_SIZE: usize = 65_536;
pub const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

pub fn find_hdr_end(d: &[u8]) -> Option<usize> {
    if d.len() < 4 { return None; }
    for i in 0..=(d.len() - 4) {
        if &d[i..i + 4] == b"\r\n\r\n" { return Some(i); }
    }
    None
}

pub fn get_hdr<'a>(h: &'a [(String, String)], n: &str) -> Option<&'a str> {
    for (k, v) in h {
        if k.eq_ignore_ascii_case(n) { return Some(v.as_str()); }
    }
    None
}

fn raw_hdr<'a>(t: &'a str, n: &str) -> Option<&'a str> {
    for l in t.lines() {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case(n) { return Some(v.trim()); }
        }
    }
    None
}

pub enum ReadResult {
    Ok(Vec<u8>),
    TimedOut,
    Error(String),
}

fn find_zero_chunk(d: &[u8]) -> bool {
    if d.len() < 5 { return false; }
    let mut i = 0;
    while i < d.len() {
        let chunk_start = i;
        let mut size_end = i;
        while size_end < d.len() && d[size_end] != b'\r' {
            size_end += 1;
        }
        if size_end + 1 >= d.len() || d[size_end + 1] != b'\n' {
            return false;
        }
        let size_str = match std::str::from_utf8(&d[chunk_start..size_end]) {
            Ok(s) => s.split(';').next().unwrap_or("").trim(),
            Err(_) => return false,
        };
        let chunk_size = match usize::from_str_radix(size_str, 16) {
            Ok(s) => s,
            Err(_) => return false,
        };
        if chunk_size == 0 {
            let after = size_end + 2;
            return after <= d.len() && d[after..].starts_with(b"\r\n")
                || after == d.len();
        }
        i = size_end + 2 + chunk_size;
        if i + 1 >= d.len() { return false; }
        if d[i] != b'\r' || d[i + 1] != b'\n' { return false; }
        i += 2;
    }
    false
}

pub fn read_http_message(r: &mut impl Read, buf_size: usize) -> ReadResult {
    let mut d = Vec::with_capacity(buf_size);
    let mut b = vec![0u8; buf_size];
    let (mut hdr_done, mut body_start, mut content_len) = (false, 0usize, None::<usize>);
    let mut is_chunked = false;
    let mut timed_out = false;

    loop {
        match r.read(&mut b) {
            Ok(0) => break,
            Ok(n) => {
                d.extend_from_slice(&b[..n]);

                if !hdr_done {
                    if d.len() > MAX_HEADER_SIZE {
                        return ReadResult::Error("headers too large".into());
                    }
                    if let Some(p) = find_hdr_end(&d) {
                        hdr_done = true;
                        body_start = p + 4;
                        let hdr_text = match std::str::from_utf8(&d[..p]) {
                            Ok(t) => t,
                            Err(_) => return ReadResult::Error("invalid header encoding".into()),
                        };
                        content_len = raw_hdr(hdr_text, "Content-Length")
                            .and_then(|v| v.parse::<usize>().ok());
                        if let Some(cl) = content_len {
                            if cl > MAX_BODY_SIZE {
                                return ReadResult::Error("body too large".into());
                            }
                        }
                        is_chunked = raw_hdr(hdr_text, "Transfer-Encoding")
                            .map(|v| v.eq_ignore_ascii_case("chunked"))
                            .unwrap_or(false);
                        if content_len.is_none() && !is_chunked {
                            break;
                        }
                    }
                }

                if hdr_done {
                    let body_len = d.len() - body_start;
                    if body_len > MAX_BODY_SIZE {
                        return ReadResult::Error("body too large".into());
                    }
                    if let Some(cl) = content_len {
                        if body_len >= cl { break; }
                    } else if is_chunked {
                        let body = &d[body_start..];
                        if find_zero_chunk(body) { break; }
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut
                       || e.kind() == std::io::ErrorKind::WouldBlock => {
                timed_out = true;
                break;
            }
            Err(e) => return ReadResult::Error(e.to_string()),
        }
    }

    if d.is_empty() {
        return if timed_out { ReadResult::TimedOut } else { ReadResult::Error("connection closed".into()) };
    }
    if timed_out && !hdr_done {
        return ReadResult::TimedOut;
    }
    if timed_out {
        if let Some(cl) = content_len {
            if d.len() - body_start < cl { return ReadResult::TimedOut; }
        }
    }
    ReadResult::Ok(d)
}
