// Shared utilities for modules
use crate::context::Context;
use crate::http::HttpResponse;
use std::collections::HashMap;

pub fn is_enabled(c: &HashMap<String, toml::Value>, m: &str) -> bool {
    config_bool(c, m, "enabled", true)
}

pub fn config_bool(c: &HashMap<String, toml::Value>, m: &str, k: &str, d: bool) -> bool {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_bool()).unwrap_or(d)
}

pub fn config_u64(c: &HashMap<String, toml::Value>, m: &str, k: &str, d: u64) -> u64 {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_integer())
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(d)
}

pub fn config_usize(c: &HashMap<String, toml::Value>, m: &str, k: &str, d: usize) -> usize {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_integer())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(d)
}

pub fn config_str(c: &HashMap<String, toml::Value>, m: &str, k: &str, d: &str) -> String {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_str()).unwrap_or(d).to_string()
}

pub fn config_vec_str(c: &HashMap<String, toml::Value>, m: &str, k: &str) -> Vec<String> {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_array()).map(|a| {
        a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect()
    }).unwrap_or_default()
}

pub fn client_ip(c: &Context) -> String {
    c.get("_client_ip").unwrap_or("?").to_string()
}

pub fn json_response(c: u16, j: &str) -> HttpResponse {
    HttpResponse {
        version: "HTTP/1.1".to_string(),
        status_code: c,
        status_text: "OK".to_string(),
        headers: vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), j.len().to_string()),
        ],
        body: j.as_bytes().to_vec(),
    }
}

pub fn bidirectional_stream(a: std::net::TcpStream, b: std::net::TcpStream, buf_size: usize) {
    use std::time::Duration;
    const STREAM_TIMEOUT: Duration = Duration::from_secs(120);

    let a_read = a;
    let a_write = match a_read.try_clone() {
        Ok(s) => s,
        Err(e) => {
            crate::log::debug(&format!("bidirectional_stream: clone failed: {e}"));
            return;
        }
    };
    let b_read = b;
    let b_write = match b_read.try_clone() {
        Ok(s) => s,
        Err(e) => {
            crate::log::debug(&format!("bidirectional_stream: clone failed: {e}"));
            return;
        }
    };

    let _ = a_read.set_read_timeout(Some(STREAM_TIMEOUT));
    let _ = b_read.set_read_timeout(Some(STREAM_TIMEOUT));

    let bs = buf_size;
    let t1 = std::thread::spawn(move || {
        stream_copy(a_read, b_write, bs);
    });
    let t2 = std::thread::spawn(move || {
        stream_copy(b_read, a_write, bs);
    });
    if let Err(e) = t1.join() {
        crate::log::warn(&format!("bidirectional_stream: thread panicked: {:?}", e));
    }
    if let Err(e) = t2.join() {
        crate::log::warn(&format!("bidirectional_stream: thread panicked: {:?}", e));
    }
}

fn stream_copy(mut r: std::net::TcpStream, mut w: std::net::TcpStream, buf_size: usize) {
    use std::io::{Read, Write};
    let mut buf = vec![0u8; buf_size];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = w.write_all(&buf[..n]) {
                    crate::log::debug(&format!("stream_copy write error: {e}"));
                    break;
                }
            }
            Err(e) => {
                crate::log::debug(&format!("stream_copy read error: {e}"));
                break;
            }
        }
    }
    let _ = w.shutdown(std::net::Shutdown::Write);
}
