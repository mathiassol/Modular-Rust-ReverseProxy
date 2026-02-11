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
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_integer()).map(|v| v as u64).unwrap_or(d)
}

pub fn config_usize(c: &HashMap<String, toml::Value>, m: &str, k: &str, d: usize) -> usize {
    c.get(m).and_then(|v| v.get(k)).and_then(|v| v.as_integer()).map(|v| v as usize).unwrap_or(d)
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
    let a_read = a;
    let a_write = match a_read.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let b_read = b;
    let b_write = match b_read.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let bs = buf_size;
    let t1 = std::thread::spawn(move || {
        stream_copy(a_read, b_write, bs);
    });
    let t2 = std::thread::spawn(move || {
        stream_copy(b_read, a_write, bs);
    });
    let _ = t1.join();
    let _ = t2.join();
}

fn stream_copy(mut r: std::net::TcpStream, mut w: std::net::TcpStream, buf_size: usize) {
    use std::io::{Read, Write};
    let mut buf = vec![0u8; buf_size];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if w.write_all(&buf[..n]).is_err() { break; }
            }
            Err(_) => break,
        }
    }
    let _ = w.shutdown(std::net::Shutdown::Write);
}
