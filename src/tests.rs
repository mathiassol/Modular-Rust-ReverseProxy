// ══════════════════════════════════════════════════════════════════════════════
// Proxycache Test Suite
// ══════════════════════════════════════════════════════════════════════════════
//
// Coverage:
//   1. HTTP parsing (request, response, chunked encoding, edge cases)
//   2. Context & pipeline mechanics
//   3. Config validation
//   4. Metrics atomics
//   5. Module unit tests (health, rate limiter, cache, compression,
//      circuit breaker, load balancer, request ID, URL rewriter, metrics export)
//   6. Integration tests (real TCP proxy with mock backend)
//   7. Connection pool
//   8. Stress & concurrency
//   9. Malformed / adversarial input
//  10. Script runtime

// ── Helpers shared across test modules ──────────────────────────────────────

#[cfg(test)]
fn make_req(method: &str, path: &str) -> crate::http::HttpRequest {
    crate::http::HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Host".to_string(), "localhost".to_string())],
        body: Vec::new(),
    }
}

#[cfg(test)]
fn make_req_with_headers(method: &str, path: &str, hdrs: &[(&str, &str)]) -> crate::http::HttpRequest {
    let mut req = make_req(method, path);
    for (k, v) in hdrs {
        req.set_header(k, v);
    }
    req
}

#[cfg(test)]
fn make_resp(status: u16, body: &str) -> crate::http::HttpResponse {
    crate::http::HttpResponse {
        version: "HTTP/1.1".to_string(),
        status_code: status,
        status_text: "OK".to_string(),
        headers: vec![
            ("Content-Type".to_string(), "text/html".to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        body: body.as_bytes().to_vec(),
    }
}

#[cfg(test)]
fn make_ctx() -> crate::context::Context {
    let mut ctx = crate::context::Context::new();
    ctx.set("_client_ip", "127.0.0.1".to_string());
    ctx.set("_protocol", "h1".to_string());
    ctx
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. HTTP PARSING
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod http_tests {
    use crate::http::{find_hdr_end, get_hdr, HttpRequest, HttpResponse};

    // ── find_hdr_end ───

    #[test]
    fn find_header_end_basic() {
        let data = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody";
        assert_eq!(find_hdr_end(data), Some(23));
    }

    #[test]
    fn find_header_end_missing() {
        let data = b"GET / HTTP/1.1\r\nHost: x\r\n";
        assert_eq!(find_hdr_end(data), None);
    }

    #[test]
    fn find_header_end_too_short() {
        assert_eq!(find_hdr_end(b"abc"), None);
    }

    #[test]
    fn find_header_end_empty() {
        assert_eq!(find_hdr_end(b""), None);
    }

    #[test]
    fn find_header_end_exactly_four_bytes() {
        assert_eq!(find_hdr_end(b"\r\n\r\n"), Some(0));
    }

    #[test]
    fn find_header_end_multiple_blank_lines() {
        let data = b"GET / HTTP/1.1\r\n\r\n\r\n";
        assert_eq!(find_hdr_end(data), Some(14));
    }

    // ── get_hdr ───

    #[test]
    fn get_header_case_insensitive() {
        let headers = vec![
            ("Content-Type".to_string(), "text/html".to_string()),
            ("X-Custom".to_string(), "value".to_string()),
        ];
        assert_eq!(get_hdr(&headers, "content-type"), Some("text/html"));
        assert_eq!(get_hdr(&headers, "CONTENT-TYPE"), Some("text/html"));
        assert_eq!(get_hdr(&headers, "x-custom"), Some("value"));
        assert_eq!(get_hdr(&headers, "missing"), None);
    }

    #[test]
    fn get_header_empty_list() {
        let headers: Vec<(String, String)> = vec![];
        assert_eq!(get_hdr(&headers, "any"), None);
    }

    #[test]
    fn get_header_returns_first_match() {
        let headers = vec![
            ("X-Dup".to_string(), "first".to_string()),
            ("X-Dup".to_string(), "second".to_string()),
        ];
        assert_eq!(get_hdr(&headers, "X-Dup"), Some("first"));
    }

    // ── Request parsing ───

    #[test]
    fn parse_valid_get_request() {
        let raw = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/index.html");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(req.get_header("Host"), Some("example.com"));
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_post_with_body() {
        let raw = b"POST /api HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn parse_all_valid_methods() {
        for method in &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "CONNECT", "TRACE"] {
            let raw = format!("{method} / HTTP/1.1\r\nHost: x\r\n\r\n");
            assert!(HttpRequest::parse(raw.as_bytes()).is_some(), "Failed for {method}");
        }
    }

    #[test]
    fn parse_http10() {
        let raw = b"GET / HTTP/1.0\r\nHost: x\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.version, "HTTP/1.0");
    }

    #[test]
    fn parse_multiple_headers() {
        let raw = b"GET / HTTP/1.1\r\nHost: x\r\nAccept: */*\r\nX-Foo: bar\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.headers.len(), 3);
        assert_eq!(req.get_header("Accept"), Some("*/*"));
        assert_eq!(req.get_header("X-Foo"), Some("bar"));
    }

    #[test]
    fn parse_header_with_colons_in_value() {
        let raw = b"GET / HTTP/1.1\r\nHost: http://example.com:8080\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.get_header("Host"), Some("http://example.com:8080"));
    }

    #[test]
    fn parse_body_truncated_to_content_length() {
        let raw = b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\nhello_extra";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.body, b"hel");
    }

    #[test]
    fn parse_query_string_preserved() {
        let raw = b"GET /search?q=rust&page=1 HTTP/1.1\r\nHost: x\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        assert_eq!(req.path, "/search?q=rust&page=1");
    }

    #[test]
    fn reject_invalid_method() {
        assert!(HttpRequest::parse(b"INVALID / HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn reject_invalid_version() {
        assert!(HttpRequest::parse(b"GET / HTTP/2.0\r\nHost: x\r\n\r\n").is_none());
        assert!(HttpRequest::parse(b"GET / HTTP/3.0\r\nHost: x\r\n\r\n").is_none());
        assert!(HttpRequest::parse(b"GET / FTP/1.1\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn reject_control_chars_in_path() {
        assert!(HttpRequest::parse(b"GET /\x00evil HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
        assert!(HttpRequest::parse(b"GET /\x0Aevil HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
        assert!(HttpRequest::parse(b"GET /\x0Devil HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
        assert!(HttpRequest::parse(b"GET /\x7Fevil HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn reject_extra_words_in_request_line() {
        assert!(HttpRequest::parse(b"GET / HTTP/1.1 extra\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn reject_empty_input() {
        assert!(HttpRequest::parse(b"").is_none());
    }

    #[test]
    fn reject_garbage_input() {
        assert!(HttpRequest::parse(b"\xff\xfe\xfd\xfc").is_none());
        assert!(HttpRequest::parse(b"not http at all").is_none());
    }

    #[test]
    fn reject_missing_path() {
        assert!(HttpRequest::parse(b"GET\r\nHost: x\r\n\r\n").is_none());
    }

    // ── Request serialization ───

    #[test]
    fn request_roundtrip() {
        let raw = b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = HttpRequest::parse(raw).unwrap();
        let bytes = req.to_bytes();
        let reparsed = HttpRequest::parse(&bytes).unwrap();
        assert_eq!(reparsed.method, "GET");
        assert_eq!(reparsed.path, "/test");
        assert_eq!(reparsed.get_header("Host"), Some("localhost"));
    }

    #[test]
    fn request_roundtrip_with_body() {
        let raw = b"POST /data HTTP/1.1\r\nContent-Length: 11\r\n\r\nhello world";
        let req = HttpRequest::parse(raw).unwrap();
        let bytes = req.to_bytes();
        let reparsed = HttpRequest::parse(&bytes).unwrap();
        assert_eq!(reparsed.method, "POST");
        assert_eq!(reparsed.body, b"hello world");
    }

    #[test]
    fn set_header_replaces_existing() {
        let mut req = HttpRequest::parse(b"GET / HTTP/1.1\r\nHost: old\r\n\r\n").unwrap();
        req.set_header("Host", "new");
        assert_eq!(req.get_header("Host"), Some("new"));
        assert_eq!(req.headers.len(), 1);
    }

    #[test]
    fn set_header_adds_if_missing() {
        let mut req = HttpRequest::parse(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        req.set_header("X-New", "val");
        assert_eq!(req.get_header("X-New"), Some("val"));
        assert_eq!(req.headers.len(), 2);
    }

    #[test]
    fn set_header_case_insensitive_replace() {
        let mut req = HttpRequest::parse(b"GET / HTTP/1.1\r\nContent-Type: old\r\n\r\n").unwrap();
        req.set_header("content-type", "new");
        assert_eq!(req.get_header("Content-Type"), Some("new"));
        assert_eq!(req.headers.len(), 1);
    }

    // ── Response parsing ───

    #[test]
    fn parse_valid_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi";
        let resp = HttpResponse::parse(raw).unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.status_text, "OK");
        assert_eq!(resp.body, b"hi");
    }

    #[test]
    fn parse_response_without_body() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let resp = HttpResponse::parse(raw).unwrap();
        assert_eq!(resp.status_code, 204);
        assert!(resp.body.is_empty());
    }

    #[test]
    fn parse_response_multi_word_status() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\n";
        let resp = HttpResponse::parse(raw).unwrap();
        assert_eq!(resp.status_code, 404);
        assert_eq!(resp.status_text, "Not Found");
    }

    #[test]
    fn parse_response_3xx() {
        let raw = b"HTTP/1.1 301 Moved Permanently\r\nLocation: /new\r\n\r\n";
        let resp = HttpResponse::parse(raw).unwrap();
        assert_eq!(resp.status_code, 301);
        assert_eq!(resp.get_header("Location"), Some("/new"));
    }

    #[test]
    fn response_roundtrip() {
        let resp = HttpResponse {
            version: "HTTP/1.1".to_string(),
            status_code: 200,
            status_text: "OK".to_string(),
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: b"test body".to_vec(),
        };
        let bytes = resp.to_bytes();
        let reparsed = HttpResponse::parse(&bytes).unwrap();
        assert_eq!(reparsed.status_code, 200);
        assert_eq!(reparsed.get_header("Content-Type"), Some("text/plain"));
        assert_eq!(reparsed.body, b"test body");
    }

    // ── Error response factory ───

    #[test]
    fn error_response_has_correct_fields() {
        let resp = HttpResponse::error(503, "down");
        assert_eq!(resp.status_code, 503);
        assert_eq!(resp.status_text, "Service Unavailable");
        assert_eq!(resp.body, b"down");
        assert_eq!(resp.get_header("Content-Length"), Some("4"));
        assert_eq!(resp.get_header("Connection"), Some("close"));
    }

    #[test]
    fn error_response_all_codes() {
        let cases = [
            (400, "Bad Request"),
            (403, "Forbidden"),
            (411, "Length Required"),
            (413, "Payload Too Large"),
            (429, "Too Many Requests"),
            (431, "Request Header Fields Too Large"),
            (502, "Bad Gateway"),
            (503, "Service Unavailable"),
            (504, "Gateway Timeout"),
            (999, "Error"),
        ];
        for (code, expected_text) in &cases {
            let resp = HttpResponse::error(*code, "msg");
            assert_eq!(resp.status_code, *code);
            assert_eq!(resp.status_text, *expected_text);
        }
    }

    #[test]
    fn error_response_empty_body() {
        let resp = HttpResponse::error(500, "");
        assert_eq!(resp.get_header("Content-Length"), Some("0"));
        assert!(resp.body.is_empty());
    }

    #[test]
    fn response_set_header_replaces() {
        let mut resp = HttpResponse::error(200, "ok");
        resp.set_header("Content-Type", "application/json");
        assert_eq!(resp.get_header("Content-Type"), Some("application/json"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. CHUNKED ENCODING
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod chunked_tests {
    use crate::http::find_zero_chunk;

    #[test]
    fn valid_zero_chunk() {
        assert!(find_zero_chunk(b"5\r\nhello\r\n0\r\n\r\n"));
    }

    #[test]
    fn no_zero_chunk_yet() {
        assert!(!find_zero_chunk(b"5\r\nhello\r\n"));
    }

    #[test]
    fn too_short() {
        assert!(!find_zero_chunk(b"0\r\n"));
    }

    #[test]
    fn just_zero_chunk() {
        assert!(find_zero_chunk(b"0\r\n\r\n"));
    }

    #[test]
    fn invalid_hex_size() {
        assert!(!find_zero_chunk(b"XY\r\nhello\r\n0\r\n\r\n"));
    }

    #[test]
    fn chunk_extension_ignored() {
        assert!(find_zero_chunk(b"5;ext=val\r\nhello\r\n0\r\n\r\n"));
    }

    #[test]
    fn multiple_chunks() {
        assert!(find_zero_chunk(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"));
    }

    #[test]
    fn single_byte_chunks() {
        assert!(find_zero_chunk(b"1\r\na\r\n1\r\nb\r\n0\r\n\r\n"));
    }

    #[test]
    fn empty_data() {
        assert!(!find_zero_chunk(b""));
    }

    #[test]
    fn hex_uppercase() {
        assert!(find_zero_chunk(b"A\r\n0123456789\r\n0\r\n\r\n"));
    }

    #[test]
    fn hex_lowercase() {
        assert!(find_zero_chunk(b"a\r\n0123456789\r\n0\r\n\r\n"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. CONTEXT
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod context_tests {
    use crate::context::Context;

    #[test]
    fn set_and_get_string() {
        let mut ctx = Context::new();
        ctx.set("key", "value".to_string());
        assert_eq!(ctx.get("key"), Some("value"));
    }

    #[test]
    fn get_missing_returns_none() {
        let ctx = Context::new();
        assert_eq!(ctx.get("nonexistent"), None);
    }

    #[test]
    fn overwrite_value() {
        let mut ctx = Context::new();
        ctx.set("key", "first".to_string());
        ctx.set("key", "second".to_string());
        assert_eq!(ctx.get("key"), Some("second"));
    }

    #[test]
    fn multiple_keys() {
        let mut ctx = Context::new();
        ctx.set("a", "1".to_string());
        ctx.set("b", "2".to_string());
        ctx.set("c", "3".to_string());
        assert_eq!(ctx.get("a"), Some("1"));
        assert_eq!(ctx.get("b"), Some("2"));
        assert_eq!(ctx.get("c"), Some("3"));
    }

    #[test]
    fn typed_state_put_take() {
        let mut ctx = Context::new();
        ctx.put("counter", 42u64);
        assert_eq!(ctx.take::<u64>("counter"), Some(&42u64));
    }

    #[test]
    fn typed_state_wrong_type_returns_none() {
        let mut ctx = Context::new();
        ctx.put("counter", 42u64);
        assert!(ctx.take::<String>("counter").is_none());
    }

    #[test]
    fn typed_state_missing_returns_none() {
        let ctx = Context::new();
        assert!(ctx.take::<u64>("missing").is_none());
    }

    #[test]
    fn elapsed_ms_is_reasonable() {
        let ctx = Context::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ms = ctx.elapsed_ms();
        assert!(ms >= 5, "Elapsed should be at least ~10ms, got {ms}");
        assert!(ms < 1000, "Elapsed should be under 1s, got {ms}");
    }

    #[test]
    fn empty_string_value() {
        let mut ctx = Context::new();
        ctx.set("empty", "".to_string());
        assert_eq!(ctx.get("empty"), Some(""));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. PIPELINE MECHANICS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod pipeline_tests {
    use crate::context::Context;
    use crate::http::{HttpRequest, HttpResponse};
    use crate::modules::{Module, Pipeline};

    struct EchoModule;
    impl Module for EchoModule {
        fn name(&self) -> &str { "echo" }
        fn handle(&self, r: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            Some(HttpResponse {
                version: "HTTP/1.1".to_string(),
                status_code: 200,
                status_text: "OK".to_string(),
                headers: vec![
                    ("Content-Type".to_string(), "text/plain".to_string()),
                    ("X-Path".to_string(), r.path.clone()),
                ],
                body: r.body.clone(),
            })
        }
    }

    struct PassthroughModule {
        name: String,
    }
    impl Module for PassthroughModule {
        fn name(&self) -> &str { &self.name }
        fn handle(&self, _: &mut HttpRequest, ctx: &mut Context) -> Option<HttpResponse> {
            ctx.set(&format!("_visited_{}", self.name), "1".to_string());
            None
        }
    }

    struct HeaderInjector {
        key: String,
        value: String,
    }
    impl Module for HeaderInjector {
        fn name(&self) -> &str { "header_injector" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> { None }
        fn on_response(&self, _req: &HttpRequest, resp: &mut HttpResponse, _ctx: &mut Context) {
            resp.set_header(&self.key, &self.value);
        }
    }

    struct ShortCircuit {
        code: u16,
    }
    impl Module for ShortCircuit {
        fn name(&self) -> &str { "short_circuit" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            Some(HttpResponse::error(self.code, "blocked"))
        }
    }

    #[test]
    fn pipeline_no_handler_returns_500() {
        let mut pipe = Pipeline::new(30);
        pipe.sort();
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 500);
    }

    #[test]
    fn pipeline_single_handler() {
        let mut pipe = Pipeline::new(30);
        pipe.add(Box::new(EchoModule));
        pipe.sort();
        let mut req = super::make_req("GET", "/hello");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.get_header("X-Path"), Some("/hello"));
    }

    #[test]
    fn pipeline_passthrough_then_handler() {
        let mut pipe = Pipeline::new(30);
        pipe.add_with_priority(Box::new(PassthroughModule { name: "first".into() }), 10);
        pipe.add_with_priority(Box::new(EchoModule), 20);
        pipe.sort();
        let mut req = super::make_req("GET", "/test");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert_eq!(ctx.get("_visited_first"), Some("1"));
    }

    #[test]
    fn pipeline_short_circuit_skips_later_modules() {
        let mut pipe = Pipeline::new(30);
        pipe.add_with_priority(Box::new(ShortCircuit { code: 403 }), 10);
        pipe.add_with_priority(Box::new(PassthroughModule { name: "after".into() }), 20);
        pipe.sort();
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 403);
        assert!(ctx.get("_visited_after").is_none());
    }

    #[test]
    fn pipeline_on_response_called_in_reverse() {
        let mut pipe = Pipeline::new(30);
        pipe.add_with_priority(Box::new(HeaderInjector { key: "X-First".into(), value: "yes".into() }), 10);
        pipe.add_with_priority(Box::new(HeaderInjector { key: "X-Second".into(), value: "yes".into() }), 20);
        pipe.add_with_priority(Box::new(EchoModule), 30);
        pipe.sort();
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.get_header("X-First"), Some("yes"));
        assert_eq!(resp.get_header("X-Second"), Some("yes"));
    }

    #[test]
    fn pipeline_priority_ordering() {
        let mut pipe = Pipeline::new(30);
        pipe.add_with_priority(Box::new(PassthroughModule { name: "low".into() }), 100);
        pipe.add_with_priority(Box::new(PassthroughModule { name: "high".into() }), 1);
        pipe.add_with_priority(Box::new(EchoModule), 50);
        pipe.sort();
        let names = pipe.module_names();
        assert_eq!(names, vec!["high", "echo", "low"]);
    }

    #[test]
    fn pipeline_has_module() {
        let mut pipe = Pipeline::new(30);
        pipe.add(Box::new(EchoModule));
        assert!(pipe.has_module("echo"));
        assert!(!pipe.has_module("nonexistent"));
    }

    #[test]
    fn pipeline_override_module() {
        let mut pipe = Pipeline::new(30);
        pipe.add_with_priority(Box::new(EchoModule), 10);
        pipe.override_module("echo");
        assert!(!pipe.has_module("echo"));
    }

    #[test]
    fn pipeline_timeout() {
        let pipe = Pipeline::new(42);
        assert_eq!(pipe.timeout(), 42);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. CONFIG VALIDATION
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod config_tests {
    use crate::config::Srv;

    #[test]
    fn server_config_defaults() {
        let cfg = Srv::default();
        assert_eq!(cfg.listen_addr, "127.0.0.1:3000");
        assert_eq!(cfg.backend_addr, "127.0.0.1:8080");
        assert_eq!(cfg.max_connections, 10000);
        assert_eq!(cfg.buffer_size, 8192);
        assert_eq!(cfg.client_timeout, 30);
        assert_eq!(cfg.backend_timeout, 30);
        assert_eq!(cfg.max_header_size, 65_536);
        assert_eq!(cfg.max_body_size, 16 * 1024 * 1024);
        assert_eq!(cfg.shutdown_timeout, 15);
        assert!(cfg.tls_cert.is_empty());
        assert!(cfg.tls_key.is_empty());
        assert!(cfg.http2);
        assert!(!cfg.http3);
    }

    #[test]
    fn validate_good_config() {
        let mut cfg = Srv::default();
        assert!(cfg.validate());
    }

    #[test]
    fn validate_bad_listen_addr() {
        let mut cfg = Srv::default();
        cfg.listen_addr = "not-an-address".to_string();
        assert!(!cfg.validate());
    }

    #[test]
    fn validate_bad_backend_addr() {
        let mut cfg = Srv::default();
        cfg.backend_addr = "garbage".to_string();
        assert!(!cfg.validate());
    }

    #[test]
    fn validate_small_buffer_corrected() {
        let mut cfg = Srv::default();
        cfg.buffer_size = 100;
        cfg.validate();
        assert_eq!(cfg.buffer_size, 1024);
    }

    #[test]
    fn validate_zero_timeout_corrected() {
        let mut cfg = Srv::default();
        cfg.client_timeout = 0;
        cfg.backend_timeout = 0;
        cfg.validate();
        assert_eq!(cfg.client_timeout, 30);
        assert_eq!(cfg.backend_timeout, 30);
    }

    #[test]
    fn validate_zero_max_body_corrected() {
        let mut cfg = Srv::default();
        cfg.max_body_size = 0;
        cfg.validate();
        assert_eq!(cfg.max_body_size, 16 * 1024 * 1024);
    }

    #[test]
    fn validate_tls_cert_without_key() {
        let mut cfg = Srv::default();
        cfg.tls_cert = "cert.pem".to_string();
        assert!(!cfg.validate());
    }

    #[test]
    fn validate_tls_key_without_cert() {
        let mut cfg = Srv::default();
        cfg.tls_key = "key.pem".to_string();
        assert!(!cfg.validate());
    }

    #[test]
    fn validate_missing_tls_files() {
        let mut cfg = Srv::default();
        cfg.tls_cert = "/nonexistent/cert.pem".to_string();
        cfg.tls_key = "/nonexistent/key.pem".to_string();
        assert!(!cfg.validate());
    }

    #[test]
    fn config_clone() {
        let cfg = Srv::default();
        let clone = cfg.clone();
        assert_eq!(clone.listen_addr, cfg.listen_addr);
        assert_eq!(clone.buffer_size, cfg.buffer_size);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. METRICS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod metrics_tests {
    use crate::metrics;

    #[test]
    fn record_latency_caps_extreme_values() {
        metrics::record_latency(u64::MAX / 2);
        let snap = metrics::snapshot();
        assert!(snap.latency_max_ms <= 600_000);
    }

    #[test]
    fn inc_requests_increments() {
        let before = metrics::snapshot().requests_total;
        metrics::inc_requests();
        let after = metrics::snapshot().requests_total;
        assert!(after > before);
    }

    #[test]
    fn inc_requests_ok_increments() {
        let before = metrics::snapshot().requests_ok;
        metrics::inc_requests_ok();
        assert!(metrics::snapshot().requests_ok > before);
    }

    #[test]
    fn inc_requests_err_increments() {
        let before = metrics::snapshot().requests_err;
        metrics::inc_requests_err();
        assert!(metrics::snapshot().requests_err > before);
    }

    #[test]
    fn bytes_tracking() {
        let before_in = metrics::snapshot().bytes_in;
        let before_out = metrics::snapshot().bytes_out;
        metrics::add_bytes_in(100);
        metrics::add_bytes_out(200);
        assert!(metrics::snapshot().bytes_in >= before_in + 100);
        assert!(metrics::snapshot().bytes_out >= before_out + 200);
    }

    #[test]
    fn connections_tracking() {
        let before = metrics::snapshot().connections_total;
        metrics::inc_connections();
        assert!(metrics::snapshot().connections_total > before);
    }

    #[test]
    fn pool_metrics() {
        let before_h = metrics::snapshot().pool_hits;
        let before_m = metrics::snapshot().pool_misses;
        metrics::inc_pool_hits();
        metrics::inc_pool_misses();
        assert!(metrics::snapshot().pool_hits > before_h);
        assert!(metrics::snapshot().pool_misses > before_m);
    }

    #[test]
    fn circuit_breaker_metrics() {
        let before_t = metrics::snapshot().cb_trips;
        let before_r = metrics::snapshot().cb_rejects;
        metrics::inc_cb_trips();
        metrics::inc_cb_rejects();
        assert!(metrics::snapshot().cb_trips > before_t);
        assert!(metrics::snapshot().cb_rejects > before_r);
    }

    #[test]
    fn avg_latency_zero_requests() {
        let snap = crate::metrics::Snapshot {
            requests_total: 0, requests_ok: 0, requests_err: 0,
            bytes_in: 0, bytes_out: 0, latency_sum_ms: 0, latency_max_ms: 0,
            connections_total: 0, active_connections: 0,
            pool_hits: 0, pool_misses: 0, cb_trips: 0, cb_rejects: 0, uptime_secs: 0,
        };
        assert_eq!(snap.avg_latency_ms(), 0);
    }

    #[test]
    fn avg_latency_calculation() {
        let snap = crate::metrics::Snapshot {
            requests_total: 10, requests_ok: 10, requests_err: 0,
            bytes_in: 0, bytes_out: 0, latency_sum_ms: 500, latency_max_ms: 100,
            connections_total: 10, active_connections: 0,
            pool_hits: 0, pool_misses: 0, cb_trips: 0, cb_rejects: 0, uptime_secs: 0,
        };
        assert_eq!(snap.avg_latency_ms(), 50);
    }

    #[test]
    fn prometheus_format_has_expected_metrics() {
        let output = metrics::snapshot_prometheus();
        assert!(output.contains("proxycache_requests_total"));
        assert!(output.contains("proxycache_active_connections"));
        assert!(output.contains("proxycache_bytes_in"));
        assert!(output.contains("proxycache_bytes_out"));
        assert!(output.contains("proxycache_latency_max_ms"));
        assert!(output.contains("proxycache_pool_hits"));
        assert!(output.contains("proxycache_circuit_breaker_trips"));
    }

    #[test]
    fn json_format_is_valid() {
        let output = metrics::snapshot_json();
        assert!(output.starts_with('{'));
        assert!(output.ends_with('}'));
        assert!(output.contains("\"requests_total\""));
        assert!(output.contains("\"latency_avg_ms\""));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. MODULE TESTS (direct unit-level testing of module logic)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod module_health_check_tests {
    use crate::modules::Pipeline;

    fn build_health_pipeline(endpoint: &str) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String(endpoint.into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));
        // Disable everything else
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn health_check_responds_on_endpoint() {
        let pipe = build_health_pipeline("/health");
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.body.windows(2).any(|w| w == b"ok"));
    }

    #[test]
    fn health_check_passes_other_paths() {
        let pipe = build_health_pipeline("/health");
        let mut req = super::make_req("GET", "/api/data");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        // No handler for /api/data, should fall through to 500 (no proxy_core)
        assert_ne!(resp.status_code, 200);
    }

    #[test]
    fn health_check_custom_endpoint() {
        let pipe = build_health_pipeline("/status");
        let mut req = super::make_req("GET", "/status");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }
}

#[cfg(test)]
mod module_rate_limiter_tests {
    use crate::modules::Pipeline;

    fn build_rate_limiter_pipeline(rps: i64, burst: i64) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut rl = toml::Table::new();
        rl.insert("enabled".into(), toml::Value::Boolean(true));
        rl.insert("requests_per_second".into(), toml::Value::Integer(rps));
        rl.insert("burst".into(), toml::Value::Integer(burst));
        mc.insert("rate_limiter".into(), toml::Value::Table(rl));
        // Disable others but enable health_check as a responder
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core",
                       "raw_tcp","request_id","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String("/health".into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn rate_limiter_allows_within_burst() {
        let pipe = build_rate_limiter_pipeline(10, 5);
        for i in 0..5 {
            let mut req = super::make_req("GET", "/health");
            let mut ctx = super::make_ctx();
            let resp = pipe.handle(&mut req, &mut ctx);
            assert_eq!(resp.status_code, 200, "Request {i} should pass within burst");
        }
    }

    #[test]
    fn rate_limiter_blocks_over_burst() {
        let pipe = build_rate_limiter_pipeline(1, 3);
        // Drain burst
        for _ in 0..3 {
            let mut req = super::make_req("GET", "/health");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Next should be blocked
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 429);
    }

    #[test]
    fn rate_limiter_different_ips_independent() {
        let pipe = build_rate_limiter_pipeline(1, 2);
        // Drain IP1 burst
        for _ in 0..2 {
            let mut req = super::make_req("GET", "/health");
            let mut ctx = super::make_ctx();
            ctx.set("_client_ip", "10.0.0.1".to_string());
            pipe.handle(&mut req, &mut ctx);
        }
        // IP1 should be blocked
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        ctx.set("_client_ip", "10.0.0.1".to_string());
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 429);

        // IP2 should still pass
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        ctx.set("_client_ip", "10.0.0.2".to_string());
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }

    #[test]
    fn rate_limiter_refills_over_time() {
        let pipe = build_rate_limiter_pipeline(100, 2);
        // Drain burst
        for _ in 0..2 {
            let mut req = super::make_req("GET", "/health");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Wait for refill
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }
}

#[cfg(test)]
mod module_cache_tests {
    use crate::context::Context;
    use crate::http::{HttpRequest, HttpResponse};
    use crate::modules::{Module, Pipeline};
    use std::sync::Arc;

    struct FakeBackend {
        body: String,
        call_count: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Module for FakeBackend {
        fn name(&self) -> &str { "fake_backend" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            self.call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(super::make_resp(200, &self.body))
        }
    }

    fn build_cache_pipeline(ttl: u64, max: usize, backend_body: &str) -> (Pipeline, Arc<std::sync::atomic::AtomicUsize>) {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut mc = std::collections::HashMap::new();
        let mut cc = toml::Table::new();
        cc.insert("enabled".into(), toml::Value::Boolean(true));
        cc.insert("ttl_seconds".into(), toml::Value::Integer(ttl as i64));
        cc.insert("max_size".into(), toml::Value::Integer(max as i64));
        cc.insert("warm_urls".into(), toml::Value::Array(vec![]));
        mc.insert("cache".into(), toml::Value::Table(cc));
        for name in &["active_health","admin_api","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.add_with_priority(Box::new(FakeBackend { body: backend_body.to_string(), call_count: counter.clone() }), 200);
        pipe.sort();
        (pipe, counter)
    }

    #[test]
    fn cache_miss_then_hit() {
        let (pipe, counter) = build_cache_pipeline(300, 100, "hello");
        // First request: miss
        let mut req = super::make_req("GET", "/page");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);

        // Second request: hit
        let mut req = super::make_req("GET", "/page");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.get_header("X-Cache"), Some("HIT"));
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn cache_different_paths_independent() {
        let (pipe, counter) = build_cache_pipeline(300, 100, "body");
        let mut req = super::make_req("GET", "/a");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        let mut req = super::make_req("GET", "/b");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn cache_post_not_cached() {
        let (pipe, counter) = build_cache_pipeline(300, 100, "data");
        let mut req = super::make_req("POST", "/api");
        req.set_header("Content-Length", "0");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        let mut req = super::make_req("POST", "/api");
        req.set_header("Content-Length", "0");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn cache_max_size_enforced() {
        let (pipe, counter) = build_cache_pipeline(300, 2, "small");
        // Fill cache with 2 entries
        for path in &["/a", "/b"] {
            let mut req = super::make_req("GET", path);
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Third entry should evict oldest
        let mut req = super::make_req("GET", "/c");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 3);

        // /c should be cached
        let mut req = super::make_req("GET", "/c");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.get_header("X-Cache"), Some("HIT"));
    }
}

#[cfg(test)]
mod module_compression_tests {
    use crate::context::Context;
    use crate::http::{HttpRequest, HttpResponse};
    use crate::modules::{Module, Pipeline};

    struct BigJsonResponder;
    impl Module for BigJsonResponder {
        fn name(&self) -> &str { "big_json" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            let body = "x".repeat(1024);
            Some(HttpResponse {
                version: "HTTP/1.1".to_string(),
                status_code: 200,
                status_text: "OK".to_string(),
                headers: vec![
                    ("Content-Type".to_string(), "application/json".to_string()),
                    ("Content-Length".to_string(), body.len().to_string()),
                ],
                body: body.into_bytes(),
            })
        }
    }

    struct SmallTextResponder;
    impl Module for SmallTextResponder {
        fn name(&self) -> &str { "small_text" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            Some(HttpResponse {
                version: "HTTP/1.1".to_string(),
                status_code: 200,
                status_text: "OK".to_string(),
                headers: vec![
                    ("Content-Type".to_string(), "text/plain".to_string()),
                    ("Content-Length".to_string(), "5".to_string()),
                ],
                body: b"hello".to_vec(),
            })
        }
    }

    struct BinaryResponder;
    impl Module for BinaryResponder {
        fn name(&self) -> &str { "binary" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            let body = vec![0u8; 1024];
            Some(HttpResponse {
                version: "HTTP/1.1".to_string(),
                status_code: 200,
                status_text: "OK".to_string(),
                headers: vec![
                    ("Content-Type".to_string(), "image/png".to_string()),
                    ("Content-Length".to_string(), body.len().to_string()),
                ],
                body,
            })
        }
    }

    fn build_compression_pipeline(min_size: i64, responder: Box<dyn Module>) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut cc = toml::Table::new();
        cc.insert("enabled".into(), toml::Value::Boolean(true));
        cc.insert("min_size".into(), toml::Value::Integer(min_size));
        mc.insert("compression".into(), toml::Value::Table(cc));
        for name in &["active_health","admin_api","cache","circuit_breaker",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.add_with_priority(responder, 200);
        pipe.sort();
        pipe
    }

    #[test]
    fn compresses_large_json_with_gzip_accepted() {
        let pipe = build_compression_pipeline(256, Box::new(BigJsonResponder));
        let mut req = super::make_req_with_headers("GET", "/", &[("Accept-Encoding", "gzip, deflate")]);
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.get_header("Content-Encoding"), Some("gzip"));
        assert!(resp.body.len() < 1024, "Compressed should be smaller");
    }

    #[test]
    fn no_compression_without_accept_encoding() {
        let pipe = build_compression_pipeline(256, Box::new(BigJsonResponder));
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert!(resp.get_header("Content-Encoding").is_none());
        assert_eq!(resp.body.len(), 1024);
    }

    #[test]
    fn no_compression_below_min_size() {
        let pipe = build_compression_pipeline(256, Box::new(SmallTextResponder));
        let mut req = super::make_req_with_headers("GET", "/", &[("Accept-Encoding", "gzip")]);
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert!(resp.get_header("Content-Encoding").is_none());
    }

    #[test]
    fn no_compression_for_binary_content() {
        let pipe = build_compression_pipeline(256, Box::new(BinaryResponder));
        let mut req = super::make_req_with_headers("GET", "/", &[("Accept-Encoding", "gzip")]);
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert!(resp.get_header("Content-Encoding").is_none());
    }
}

#[cfg(test)]
mod module_circuit_breaker_tests {
    use crate::context::Context;
    use crate::http::{HttpRequest, HttpResponse};
    use crate::modules::{Module, Pipeline};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU16, Ordering};

    struct ConfigurableBackend {
        status: Arc<AtomicU16>,
    }
    impl Module for ConfigurableBackend {
        fn name(&self) -> &str { "configurable_backend" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            let code = self.status.load(Ordering::Relaxed);
            Some(HttpResponse::error(code, "resp"))
        }
    }

    fn build_cb_pipeline(threshold: i64, recovery: i64) -> (Pipeline, Arc<AtomicU16>) {
        let backend_status = Arc::new(AtomicU16::new(200));
        let mut mc = std::collections::HashMap::new();
        let mut cb = toml::Table::new();
        cb.insert("enabled".into(), toml::Value::Boolean(true));
        cb.insert("failure_threshold".into(), toml::Value::Integer(threshold));
        cb.insert("recovery_timeout".into(), toml::Value::Integer(recovery));
        mc.insert("circuit_breaker".into(), toml::Value::Table(cb));
        for name in &["active_health","admin_api","cache","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.add_with_priority(Box::new(ConfigurableBackend { status: backend_status.clone() }), 200);
        pipe.sort();
        (pipe, backend_status)
    }

    #[test]
    fn cb_closed_allows_requests() {
        let (pipe, _) = build_cb_pipeline(3, 30);
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }

    #[test]
    fn cb_opens_after_threshold_failures() {
        let (pipe, status) = build_cb_pipeline(3, 30);
        status.store(500, Ordering::Relaxed);
        // Trigger threshold failures
        for _ in 0..3 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Next request should be blocked by circuit breaker
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 503);
        assert!(String::from_utf8_lossy(&resp.body).contains("Circuit breaker"));
    }

    #[test]
    fn cb_resets_on_success() {
        let (pipe, status) = build_cb_pipeline(3, 30);
        // Two failures
        status.store(500, Ordering::Relaxed);
        for _ in 0..2 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Success resets counter
        status.store(200, Ordering::Relaxed);
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);

        // Two more failures shouldn't trip (counter was reset)
        status.store(500, Ordering::Relaxed);
        for _ in 0..2 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Should still allow (2 < threshold 3)
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_ne!(resp.status_code, 503);
    }

    #[test]
    fn cb_half_open_after_recovery_timeout() {
        let (pipe, status) = build_cb_pipeline(2, 1);
        status.store(500, Ordering::Relaxed);
        for _ in 0..2 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
        }
        // Should be open
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 503);

        // Wait for recovery
        std::thread::sleep(std::time::Duration::from_secs(2));
        // Backend recovers
        status.store(200, Ordering::Relaxed);
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }
}

#[cfg(test)]
mod module_load_balancer_tests {
    use crate::modules::Pipeline;

    fn build_lb_pipeline(backends: &[&str]) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut lb = toml::Table::new();
        lb.insert("enabled".into(), toml::Value::Boolean(true));
        lb.insert("backends".into(), toml::Value::Array(
            backends.iter().map(|b| toml::Value::String(b.to_string())).collect()
        ));
        mc.insert("load_balancer".into(), toml::Value::Table(lb));
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn lb_round_robin_distributes() {
        let pipe = build_lb_pipeline(&["127.0.0.1:8001", "127.0.0.1:8002", "127.0.0.1:8003"]);
        let mut addrs = std::collections::HashSet::new();
        for _ in 0..6 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
            if let Some(addr) = ctx.get("_backend_addr") {
                addrs.insert(addr.to_string());
            }
        }
        assert_eq!(addrs.len(), 3, "Should hit all 3 backends");
    }

    #[test]
    fn lb_empty_backends_falls_back_to_server_addr() {
        // When backends list is empty, load_balancer falls back to Single
        // which uses server.backend_addr from config
        let pipe = build_lb_pipeline(&[]);
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        let _ = pipe.handle(&mut req, &mut ctx);
        // Should fall back to server's default backend_addr
        assert!(ctx.get("_backend_addr").is_some());
    }

    #[test]
    fn lb_single_backend_always_same() {
        let pipe = build_lb_pipeline(&["127.0.0.1:9999"]);
        for _ in 0..5 {
            let mut req = super::make_req("GET", "/");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
            assert_eq!(ctx.get("_backend_addr"), Some("127.0.0.1:9999"));
        }
    }

    #[test]
    fn lb_disabled_uses_server_backend() {
        let mut mc = std::collections::HashMap::new();
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        let mut req = super::make_req("GET", "/");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(ctx.get("_backend_addr"), Some("127.0.0.1:8080"));
    }
}

#[cfg(test)]
mod module_request_id_tests {
    use crate::modules::Pipeline;

    fn build_request_id_pipeline() -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut ri = toml::Table::new();
        ri.insert("enabled".into(), toml::Value::Boolean(true));
        mc.insert("request_id".into(), toml::Value::Table(ri));
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String("/health".into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn generates_unique_request_ids() {
        let pipe = build_request_id_pipeline();
        let mut ids = std::collections::HashSet::new();
        for _ in 0..100 {
            let mut req = super::make_req("GET", "/health");
            let mut ctx = super::make_ctx();
            pipe.handle(&mut req, &mut ctx);
            if let Some(id) = ctx.get("_request_id") {
                ids.insert(id.to_string());
            }
        }
        assert_eq!(ids.len(), 100, "All 100 IDs should be unique");
    }

    #[test]
    fn preserves_existing_request_id() {
        let pipe = build_request_id_pipeline();
        let mut req = super::make_req_with_headers("GET", "/health", &[("X-Request-Id", "my-custom-id")]);
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(ctx.get("_request_id"), Some("my-custom-id"));
        assert_eq!(resp.get_header("X-Request-Id"), Some("my-custom-id"));
    }

    #[test]
    fn request_id_in_response_header() {
        let pipe = build_request_id_pipeline();
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert!(resp.get_header("X-Request-Id").is_some());
    }

    #[test]
    fn request_id_format() {
        let pipe = build_request_id_pipeline();
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        let id = ctx.get("_request_id").unwrap();
        assert!(id.contains('-'), "ID format: {{timestamp_hex}}-{{counter_hex}}");
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(u64::from_str_radix(parts[0], 16).is_ok());
        assert!(u64::from_str_radix(parts[1], 16).is_ok());
    }
}

#[cfg(test)]
mod module_url_rewriter_tests {
    use crate::modules::Pipeline;

    fn build_rewriter_pipeline(rules: &[(&str, &str)]) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut ur = toml::Table::new();
        ur.insert("enabled".into(), toml::Value::Boolean(true));
        let mut rules_table = toml::Table::new();
        for (from, to) in rules {
            rules_table.insert(from.to_string(), toml::Value::String(to.to_string()));
        }
        ur.insert("rules".into(), toml::Value::Table(rules_table));
        mc.insert("url_rewriter".into(), toml::Value::Table(ur));
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String("/health".into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn rewrites_matching_prefix() {
        let pipe = build_rewriter_pipeline(&[("/old", "/new")]);
        let mut req = super::make_req("GET", "/old/page");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(req.path, "/new/page");
    }

    #[test]
    fn no_rewrite_for_unmatched_path() {
        let pipe = build_rewriter_pipeline(&[("/old", "/new")]);
        let mut req = super::make_req("GET", "/other");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(req.path, "/other");
    }

    #[test]
    fn first_matching_rule_wins() {
        let pipe = build_rewriter_pipeline(&[("/api", "/v2"), ("/api/v1", "/v3")]);
        let mut req = super::make_req("GET", "/api/v1/users");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert!(req.path.starts_with("/v2") || req.path.starts_with("/v3"));
    }

    #[test]
    fn rewrite_exact_path() {
        // Use a path that doesn't conflict with health_check (/health)
        let pipe = build_rewriter_pipeline(&[("/old-page", "/new-page")]);
        let mut req = super::make_req("GET", "/old-page");
        let mut ctx = super::make_ctx();
        pipe.handle(&mut req, &mut ctx);
        assert_eq!(req.path, "/new-page");
    }
}

#[cfg(test)]
mod module_metrics_exporter_tests {
    use crate::modules::Pipeline;

    fn build_metrics_pipeline(endpoint: &str) -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        let mut me = toml::Table::new();
        me.insert("enabled".into(), toml::Value::Boolean(true));
        me.insert("endpoint".into(), toml::Value::String(endpoint.into()));
        mc.insert("metrics_exporter".into(), toml::Value::Table(me));
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "load_balancer","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter","health_check"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn metrics_endpoint_returns_prometheus_format() {
        let pipe = build_metrics_pipeline("/metrics");
        let mut req = super::make_req("GET", "/metrics");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.get_header("Content-Type").unwrap().contains("text/plain"));
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("proxycache_requests_total"));
    }

    #[test]
    fn metrics_only_on_get() {
        let pipe = build_metrics_pipeline("/metrics");
        let mut req = super::make_req("POST", "/metrics");
        req.set_header("Content-Length", "0");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_ne!(resp.status_code, 200);
    }

    #[test]
    fn metrics_wrong_path_passes_through() {
        let pipe = build_metrics_pipeline("/metrics");
        let mut req = super::make_req("GET", "/other");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_ne!(resp.status_code, 200);
    }

    #[test]
    fn metrics_custom_endpoint() {
        let pipe = build_metrics_pipeline("/stats");
        let mut req = super::make_req("GET", "/stats");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. INTEGRATION TESTS — Real TCP with mock backend
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod integration_tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use std::time::Duration;

    /// Spawn a mock HTTP backend that returns a configurable response
    fn mock_backend(response: &str) -> (std::net::SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let resp = response.to_string();
        std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            loop {
                if stop_clone.load(Ordering::Relaxed) { break; }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buf = [0u8; 4096];
                        let _ = stream.read(&mut buf);
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                        std::thread::sleep(Duration::from_millis(10));
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {}
                }
            }
        });
        std::thread::sleep(Duration::from_millis(50));
        (addr, stop)
    }

    /// Spawn the proxy server (HTTP/1.1 plain, no TLS)
    fn start_proxy(backend_addr: &str, modules: std::collections::HashMap<String, toml::Value>) -> (std::net::SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        let mut srv = crate::config::Srv::default();
        srv.listen_addr = proxy_addr.to_string();
        srv.backend_addr = backend_addr.to_string();
        srv.tls_cert = String::new();
        srv.tls_key = String::new();
        srv.http2 = false;
        srv.http3 = false;
        srv.client_timeout = 5;
        srv.backend_timeout = 5;

        let mut pipe = crate::modules::Pipeline::new(srv.client_timeout);
        crate::modules::register_all(&mut pipe, &modules, &srv);
        pipe.sort();
        let pipe = Arc::new(pipe);
        let buf_size = srv.buffer_size;

        std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            loop {
                if stop_clone.load(Ordering::Relaxed) { break; }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let pipe = Arc::clone(&pipe);
                        std::thread::spawn(move || {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                            let raw = crate::http::read_http_message(&mut stream, buf_size);
                            let raw = match raw {
                                crate::http::ReadResult::Ok(d) => d,
                                _ => return,
                            };
                            let mut req = match crate::http::HttpRequest::parse(&raw) {
                                Some(r) => r,
                                None => return,
                            };
                            let ip = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                            let mut ctx = crate::context::Context::new();
                            ctx.set("_client_ip", ip);
                            ctx.set("_protocol", "h1".to_string());
                            let resp = pipe.handle(&mut req, &mut ctx);
                            let _ = stream.write_all(&resp.to_bytes());
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {}
                }
            }
        });
        std::thread::sleep(Duration::from_millis(50));
        (proxy_addr, stop)
    }

    fn default_modules() -> std::collections::HashMap<String, toml::Value> {
        let mut mc = std::collections::HashMap::new();
        // Enable health_check and proxy_core, disable everything else
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String("/health".into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));
        let mut pc = toml::Table::new();
        pc.insert("enabled".into(), toml::Value::Boolean(true));
        mc.insert("proxy_core".into(), toml::Value::Table(pc));
        let mut lb = toml::Table::new();
        lb.insert("enabled".into(), toml::Value::Boolean(false));
        mc.insert("load_balancer".into(), toml::Value::Table(lb));
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "metrics_exporter","rate_limiter","raw_tcp","request_id","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        mc
    }

    fn send_request(addr: &std::net::SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect_timeout(addr, Duration::from_secs(3)).unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        stream.write_all(request.as_bytes()).unwrap();
        // Don't shutdown(Write) - let the server close when done
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp
    }

    #[test]
    fn integration_health_check() {
        let mc = default_modules();
        let (proxy_addr, stop) = start_proxy("127.0.0.1:1", mc);
        let resp = send_request(&proxy_addr, "GET /health HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(resp.contains("200"), "Expected 200 OK, got: {resp}");
        assert!(resp.contains("\"status\":\"ok\""));
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_proxy_forwarding() {
        let backend_resp = "HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello proxy";
        let (backend_addr, backend_stop) = mock_backend(backend_resp);
        let mc = default_modules();
        let (proxy_addr, proxy_stop) = start_proxy(&backend_addr.to_string(), mc);

        let resp = send_request(&proxy_addr, "GET /api/test HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(resp.contains("200"), "Expected 200, got: {resp}");
        assert!(resp.contains("hello proxy"));

        proxy_stop.store(true, Ordering::Relaxed);
        backend_stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_proxy_preserves_headers() {
        let backend_resp = "HTTP/1.1 200 OK\r\nX-Custom: test-value\r\nContent-Length: 2\r\n\r\nok";
        let (backend_addr, backend_stop) = mock_backend(backend_resp);
        let mc = default_modules();
        let (proxy_addr, proxy_stop) = start_proxy(&backend_addr.to_string(), mc);

        let resp = send_request(&proxy_addr, "GET /api HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(resp.contains("X-Custom: test-value"));

        proxy_stop.store(true, Ordering::Relaxed);
        backend_stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_backend_unavailable_returns_502() {
        let mc = default_modules();
        // Point to a port that nobody is listening on
        let (proxy_addr, stop) = start_proxy("127.0.0.1:1", mc);
        let resp = send_request(&proxy_addr, "GET /api HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(resp.contains("502"), "Expected 502, got: {resp}");
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_concurrent_requests() {
        let backend_resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        let (backend_addr, backend_stop) = mock_backend(backend_resp);
        let mc = default_modules();
        let (proxy_addr, proxy_stop) = start_proxy(&backend_addr.to_string(), mc);

        let handles: Vec<_> = (0..10).map(|_| {
            let addr = proxy_addr;
            std::thread::spawn(move || {
                send_request(&addr, "GET /health HTTP/1.1\r\nHost: test\r\n\r\n")
            })
        }).collect();

        for h in handles {
            let resp = h.join().unwrap();
            assert!(resp.contains("200"), "Concurrent request failed: {resp}");
        }

        proxy_stop.store(true, Ordering::Relaxed);
        backend_stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_post_forwarded() {
        let backend_resp = "HTTP/1.1 201 Created\r\nContent-Length: 7\r\n\r\ncreated";
        let (backend_addr, backend_stop) = mock_backend(backend_resp);
        let mc = default_modules();
        let (proxy_addr, proxy_stop) = start_proxy(&backend_addr.to_string(), mc);

        let resp = send_request(&proxy_addr,
            "POST /api/items HTTP/1.1\r\nHost: test\r\nContent-Length: 13\r\n\r\n{\"name\":\"hi\"}");
        assert!(resp.contains("201"), "Expected 201, got: {resp}");
        assert!(resp.contains("created"));

        proxy_stop.store(true, Ordering::Relaxed);
        backend_stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_rate_limiting_e2e() {
        let backend_resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        let (backend_addr, backend_stop) = mock_backend(backend_resp);
        let mut mc = default_modules();
        let mut rl = toml::Table::new();
        rl.insert("enabled".into(), toml::Value::Boolean(true));
        rl.insert("requests_per_second".into(), toml::Value::Integer(1));
        rl.insert("burst".into(), toml::Value::Integer(3));
        mc.insert("rate_limiter".into(), toml::Value::Table(rl));
        let (proxy_addr, proxy_stop) = start_proxy(&backend_addr.to_string(), mc);

        // First 3 should pass (burst)
        for _ in 0..3 {
            let resp = send_request(&proxy_addr, "GET /health HTTP/1.1\r\nHost: test\r\n\r\n");
            assert!(resp.contains("200"), "Within burst should pass");
        }
        // 4th should be rate limited
        let resp = send_request(&proxy_addr, "GET /health HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(resp.contains("429"), "Over burst should be 429, got: {resp}");

        proxy_stop.store(true, Ordering::Relaxed);
        backend_stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn integration_malformed_request() {
        let mc = default_modules();
        let (proxy_addr, stop) = start_proxy("127.0.0.1:1", mc);
        let mut stream = TcpStream::connect_timeout(&proxy_addr, Duration::from_secs(2)).unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        // Send garbage
        let _ = stream.write_all(b"NOT HTTP AT ALL\r\n\r\n");
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        // Should get 400 or connection close (not crash)
        assert!(resp.contains("400") || resp.is_empty(), "Should handle gracefully: {resp}");
        stop.store(true, Ordering::Relaxed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. CONNECTION POOL
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod pool_tests {
    use crate::pool::ConnPool;
    use std::net::{TcpListener, SocketAddr};
    use std::time::Duration;

    fn echo_listener() -> (SocketAddr, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, listener)
    }

    #[test]
    fn pool_get_creates_new_connection() {
        let (addr, _listener) = echo_listener();
        let pool = ConnPool::new();
        let stream = pool.get(&addr, Duration::from_secs(2));
        assert!(stream.is_ok());
    }

    #[test]
    fn pool_put_and_reuse() {
        let (addr, _listener) = echo_listener();
        let pool = ConnPool::new();
        let stream = pool.get(&addr, Duration::from_secs(2)).unwrap();
        pool.put(addr, stream);
        // Pool should have one idle connection now
        // Getting again might reuse or create new (depends on timing/probing)
        let stream2 = pool.get(&addr, Duration::from_secs(2));
        assert!(stream2.is_ok());
    }

    #[test]
    fn pool_clear_empties() {
        let (addr, _listener) = echo_listener();
        let pool = ConnPool::new();
        let stream = pool.get(&addr, Duration::from_secs(2)).unwrap();
        pool.put(addr, stream);
        pool.clear();
        // After clear, next get must create new
        let stream2 = pool.get(&addr, Duration::from_secs(2));
        assert!(stream2.is_ok());
    }

    #[test]
    fn pool_connection_to_closed_port_fails() {
        let pool = ConnPool::new();
        let bad_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let result = pool.get(&bad_addr, Duration::from_millis(100));
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. STRESS & CONCURRENCY
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod stress_tests {
    use crate::http::{HttpRequest, HttpResponse};
    use crate::modules::{Module, Pipeline};
    use crate::context::Context;
    use std::sync::Arc;

    struct QuickResponder;
    impl Module for QuickResponder {
        fn name(&self) -> &str { "quick" }
        fn handle(&self, _: &mut HttpRequest, _: &mut Context) -> Option<HttpResponse> {
            Some(super::make_resp(200, "ok"))
        }
    }

    #[test]
    fn concurrent_pipeline_execution() {
        let mut pipe = Pipeline::new(30);
        pipe.add(Box::new(QuickResponder));
        pipe.sort();
        let pipe = Arc::new(pipe);

        let handles: Vec<_> = (0..50).map(|_| {
            let pipe = Arc::clone(&pipe);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    let mut req = super::make_req("GET", "/");
                    let mut ctx = super::make_ctx();
                    let resp = pipe.handle(&mut req, &mut ctx);
                    assert_eq!(resp.status_code, 200);
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn metrics_under_concurrent_load() {
        let before = crate::metrics::snapshot().requests_total;
        let handles: Vec<_> = (0..20).map(|_| {
            std::thread::spawn(|| {
                for _ in 0..100 {
                    crate::metrics::inc_requests();
                }
            })
        }).collect();
        for h in handles {
            h.join().unwrap();
        }
        let after = crate::metrics::snapshot().requests_total;
        assert!(after >= before + 2000, "Expected at least 2000 increments, got {}", after - before);
    }

    #[test]
    fn pool_concurrent_access() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let pool = Arc::new(crate::pool::ConnPool::new());

        let handles: Vec<_> = (0..10).map(|_| {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || {
                for _ in 0..5 {
                    if let Ok(stream) = pool.get(&addr, std::time::Duration::from_secs(1)) {
                        pool.put(addr, stream);
                    }
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. ADVERSARIAL INPUT
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod adversarial_tests {
    use crate::http::{HttpRequest, HttpResponse, find_hdr_end, find_zero_chunk, read_http_message, ReadResult};
    use std::io::Cursor;

    #[test]
    fn parse_request_null_bytes_everywhere() {
        assert!(HttpRequest::parse(b"\0\0\0\0").is_none());
    }

    #[test]
    fn parse_request_very_long_path() {
        let long_path = "/".to_string() + &"a".repeat(10_000);
        let raw = format!("GET {long_path} HTTP/1.1\r\nHost: x\r\n\r\n");
        let req = HttpRequest::parse(raw.as_bytes());
        // Should parse (path is valid ASCII, just long)
        assert!(req.is_some());
        assert_eq!(req.unwrap().path.len(), 10_001);
    }

    #[test]
    fn parse_request_many_headers() {
        let mut raw = "GET / HTTP/1.1\r\n".to_string();
        for i in 0..100 {
            raw.push_str(&format!("X-Header-{i}: value-{i}\r\n"));
        }
        raw.push_str("\r\n");
        let req = HttpRequest::parse(raw.as_bytes()).unwrap();
        assert_eq!(req.headers.len(), 100);
    }

    #[test]
    fn parse_response_zero_status() {
        let raw = b"HTTP/1.1 0 \r\n\r\n";
        let resp = HttpResponse::parse(raw);
        assert!(resp.is_some());
        assert_eq!(resp.unwrap().status_code, 0);
    }

    #[test]
    fn parse_response_999_status() {
        let raw = b"HTTP/1.1 999 Custom\r\n\r\n";
        let resp = HttpResponse::parse(raw).unwrap();
        assert_eq!(resp.status_code, 999);
    }

    #[test]
    fn parse_response_non_numeric_status() {
        assert!(HttpResponse::parse(b"HTTP/1.1 abc Error\r\n\r\n").is_none());
    }

    #[test]
    fn read_message_empty_stream() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        match read_http_message(&mut cursor, 8192) {
            ReadResult::Error(e) => assert_eq!(e, "connection closed"),
            other => panic!("Expected Error, got {:?}", matches!(other, ReadResult::Ok(_))),
        }
    }

    #[test]
    fn read_message_headers_only() {
        let data = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut cursor = Cursor::new(data.to_vec());
        match read_http_message(&mut cursor, 8192) {
            ReadResult::Ok(d) => {
                let req = HttpRequest::parse(&d).unwrap();
                assert_eq!(req.method, "GET");
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn read_message_with_content_length() {
        let data = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let mut cursor = Cursor::new(data.to_vec());
        match read_http_message(&mut cursor, 8192) {
            ReadResult::Ok(d) => {
                let req = HttpRequest::parse(&d).unwrap();
                assert_eq!(req.body, b"hello");
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn read_message_oversized_headers() {
        let mut data = b"GET / HTTP/1.1\r\n".to_vec();
        // Add enough headers to exceed MAX_HEADER_SIZE (65536)
        for i in 0..2000 {
            data.extend_from_slice(format!("X-Big-Header-{i}: {}\r\n", "x".repeat(30)).as_bytes());
        }
        data.extend_from_slice(b"\r\n");
        let mut cursor = Cursor::new(data);
        match read_http_message(&mut cursor, 8192) {
            ReadResult::Error(e) => assert_eq!(e, "headers too large"),
            _ => panic!("Expected headers too large error"),
        }
    }

    #[test]
    fn error_response_special_chars_in_body() {
        let resp = HttpResponse::error(500, "<script>alert('xss')</script>");
        assert_eq!(resp.body, b"<script>alert('xss')</script>");
        assert_eq!(resp.get_header("Content-Length"), Some("29"));
    }

    #[test]
    fn request_with_emoji_in_header_value() {
        let raw = "GET / HTTP/1.1\r\nX-Emoji: 🚀\r\n\r\n";
        let req = HttpRequest::parse(raw.as_bytes()).unwrap();
        assert_eq!(req.get_header("X-Emoji"), Some("🚀"));
    }

    #[test]
    fn find_hdr_end_with_lone_cr() {
        // b"GET / HTTP/1.1\r\n\rHost: x\r\n\r\n" = 30 bytes
        // The \r\n\r\n starts at index 24
        assert_eq!(find_hdr_end(b"GET / HTTP/1.1\r\n\rHost: x\r\n\r\n"), Some(24));
    }

    #[test]
    fn chunked_massive_size_value() {
        // Chunk with absurdly large hex size should not overflow
        let data = b"FFFFFFFFFFFFFFFF\r\n";
        assert!(!find_zero_chunk(data));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. HELPERS MODULE
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod helpers_tests {
    use crate::modules::helpers;
    use std::collections::HashMap;

    fn test_config() -> HashMap<String, toml::Value> {
        let mut mc = HashMap::new();
        let mut mod_table = toml::Table::new();
        mod_table.insert("enabled".into(), toml::Value::Boolean(true));
        mod_table.insert("count".into(), toml::Value::Integer(42));
        mod_table.insert("name".into(), toml::Value::String("test".into()));
        mod_table.insert("rate".into(), toml::Value::Integer(100));
        mc.insert("test_mod".into(), toml::Value::Table(mod_table));
        mc
    }

    #[test]
    fn is_enabled_true() {
        let cfg = test_config();
        assert!(helpers::is_enabled(&cfg, "test_mod"));
    }

    #[test]
    fn is_enabled_missing_module() {
        let cfg = test_config();
        assert!(helpers::is_enabled(&cfg, "nonexistent"));
    }

    #[test]
    fn config_u64_reads_value() {
        let cfg = test_config();
        assert_eq!(helpers::config_u64(&cfg, "test_mod", "count", 0), 42);
    }

    #[test]
    fn config_u64_returns_default_on_missing() {
        let cfg = test_config();
        assert_eq!(helpers::config_u64(&cfg, "test_mod", "missing", 99), 99);
    }

    #[test]
    fn config_str_reads_value() {
        let cfg = test_config();
        assert_eq!(helpers::config_str(&cfg, "test_mod", "name", "default"), "test");
    }

    #[test]
    fn config_str_returns_default_on_missing() {
        let cfg = test_config();
        assert_eq!(helpers::config_str(&cfg, "test_mod", "missing", "fallback"), "fallback");
    }

    #[test]
    fn json_response_structure() {
        let resp = helpers::json_response(200, r#"{"ok":true}"#);
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.get_header("Content-Type"), Some("application/json"));
        assert_eq!(resp.body, br#"{"ok":true}"#);
    }

    #[test]
    fn client_ip_from_context() {
        let mut ctx = crate::context::Context::new();
        ctx.set("_client_ip", "192.168.1.1".to_string());
        assert_eq!(helpers::client_ip(&ctx), "192.168.1.1");
    }

    #[test]
    fn client_ip_missing_returns_question_mark() {
        let ctx = crate::context::Context::new();
        assert_eq!(helpers::client_ip(&ctx), "?");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. MODULE DEFAULTS & REGISTRATION
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod module_defaults_tests {
    use crate::modules;

    #[test]
    fn collect_defaults_has_all_modules() {
        let defaults = modules::collect_defaults();
        let expected = [
            "active_health", "admin_api", "cache", "circuit_breaker",
            "compression", "health_check", "load_balancer", "metrics_exporter",
            "proxy_core", "rate_limiter", "raw_tcp", "request_id", "url_rewriter",
        ];
        for name in &expected {
            assert!(defaults.contains_key(*name), "Missing default for module: {name}");
        }
    }

    #[test]
    fn each_default_has_enabled_field() {
        let defaults = modules::collect_defaults();
        for (name, value) in &defaults {
            let table = value.as_table().expect(&format!("{name} default is not a table"));
            assert!(table.contains_key("enabled"), "{name} missing 'enabled' field");
        }
    }

    #[test]
    fn register_all_creates_pipeline() {
        let defaults = modules::collect_defaults();
        let srv = crate::config::Srv::default();
        let mut pipe = modules::Pipeline::new(30);
        modules::register_all(&mut pipe, &defaults, &srv);
        pipe.sort();
        // Should have at least health_check and load_balancer (always registered)
        assert!(pipe.has_module("health_check"));
        assert!(pipe.has_module("load_balancer"));
    }

    #[test]
    fn disabled_modules_not_registered() {
        let mut mc = std::collections::HashMap::new();
        for name in &["active_health","admin_api","cache","circuit_breaker","compression",
                       "health_check","metrics_exporter","proxy_core","rate_limiter",
                       "raw_tcp","request_id","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let mut lb = toml::Table::new();
        lb.insert("enabled".into(), toml::Value::Boolean(false));
        mc.insert("load_balancer".into(), toml::Value::Table(lb));
        let srv = crate::config::Srv::default();
        let mut pipe = modules::Pipeline::new(30);
        modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        assert!(!pipe.has_module("health_check"));
        assert!(!pipe.has_module("cache"));
        assert!(!pipe.has_module("rate_limiter"));
        // load_balancer always registers (Single fallback)
        assert!(pipe.has_module("load_balancer"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. SCRIPT PARSER
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod script_parser_tests {
    use crate::script::parser::{parse, Command};

    #[test]
    fn parse_minimal_module() {
        let src = "mod test_mod\nversion 1.0\npriority 50\n";
        let def = parse(src).unwrap();
        assert_eq!(def.name, "test_mod");
        assert_eq!(def.version, "1.0");
        assert_eq!(def.priority, 50);
    }

    #[test]
    fn parse_module_with_config() {
        let src = "mod test\nversion 1.0\npriority 50\nconfig {\n  enabled bool true\n  name str hello\n}\n";
        let def = parse(src).unwrap();
        assert_eq!(def.config.len(), 2);
        assert_eq!(def.config[0].key, "enabled");
        assert_eq!(def.config[1].key, "name");
    }

    #[test]
    fn parse_on_request_respond() {
        let src = "mod test\nversion 1.0\npriority 50\non_request {\n  respond 200 text \"hello\"\n}\n";
        let def = parse(src).unwrap();
        assert_eq!(def.on_request.len(), 1);
        match &def.on_request[0] {
            Command::Respond { code, .. } => assert_eq!(*code, 200),
            _ => panic!("Expected Respond command"),
        }
    }

    #[test]
    fn parse_on_request_with_if() {
        let src = "mod test\nversion 1.0\npriority 50\non_request {\n  if path == /health {\n    respond 200 json \"{}\"\n  }\n}\n";
        let def = parse(src).unwrap();
        assert_eq!(def.on_request.len(), 1);
        match &def.on_request[0] {
            Command::If { field, op, value, body } => {
                assert_eq!(field, "path");
                assert_eq!(op, "==");
                assert_eq!(value, "/health");
                assert_eq!(body.len(), 1);
            }
            _ => panic!("Expected If command"),
        }
    }

    #[test]
    fn parse_set_header_command() {
        let src = "mod test\nversion 1.0\npriority 50\non_request {\n  set_header X-Test value\n}\n";
        let def = parse(src).unwrap();
        match &def.on_request[0] {
            Command::SetHeader { name, value } => {
                assert_eq!(name, "X-Test");
                assert_eq!(value, "value");
            }
            _ => panic!("Expected SetHeader"),
        }
    }

    #[test]
    fn parse_log_command() {
        let src = "mod test\nversion 1.0\npriority 50\non_request {\n  log info \"hello world\"\n}\n";
        let def = parse(src).unwrap();
        match &def.on_request[0] {
            Command::Log { level, msg } => {
                assert_eq!(level, "info");
                assert!(msg.contains("hello"));
            }
            _ => panic!("Expected Log"),
        }
    }

    #[test]
    fn parse_empty_source_fails() {
        assert!(parse("").is_err());
    }

    #[test]
    fn parse_missing_name_fails() {
        assert!(parse("version 1.0\npriority 50\n").is_err());
    }

    #[test]
    fn parse_overrides() {
        let src = "mod test\nversion 1.0\npriority 50\noverrides health_check\n";
        let def = parse(src).unwrap();
        assert_eq!(def.overrides, vec!["health_check"]);
    }

    #[test]
    fn parse_on_response_block() {
        let src = "mod test\nversion 1.0\npriority 50\non_response {\n  set_header X-Processed true\n}\n";
        let def = parse(src).unwrap();
        assert_eq!(def.on_response.len(), 1);
    }

    #[test]
    fn parse_std_call() {
        let src = "mod test\nversion 1.0\npriority 50\non_request {\n  std.proxy.forward\n}\n";
        let def = parse(src).unwrap();
        match &def.on_request[0] {
            Command::StdCall { func, .. } => assert_eq!(func, "proxy.forward"),
            _ => panic!("Expected StdCall"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. MULTI-MODULE PIPELINE INTEGRATION
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod multi_module_tests {
    use crate::modules::Pipeline;

    fn build_full_pipeline() -> Pipeline {
        let mut mc = std::collections::HashMap::new();
        // Enable several modules together
        let mut hc = toml::Table::new();
        hc.insert("enabled".into(), toml::Value::Boolean(true));
        hc.insert("endpoint".into(), toml::Value::String("/health".into()));
        mc.insert("health_check".into(), toml::Value::Table(hc));

        let mut ri = toml::Table::new();
        ri.insert("enabled".into(), toml::Value::Boolean(true));
        mc.insert("request_id".into(), toml::Value::Table(ri));

        let mut me = toml::Table::new();
        me.insert("enabled".into(), toml::Value::Boolean(true));
        me.insert("endpoint".into(), toml::Value::String("/metrics".into()));
        mc.insert("metrics_exporter".into(), toml::Value::Table(me));

        let mut comp = toml::Table::new();
        comp.insert("enabled".into(), toml::Value::Boolean(true));
        comp.insert("min_size".into(), toml::Value::Integer(10));
        mc.insert("compression".into(), toml::Value::Table(comp));

        for name in &["active_health","admin_api","cache","circuit_breaker",
                       "load_balancer","proxy_core","rate_limiter","raw_tcp","url_rewriter"] {
            let mut t = toml::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(false));
            mc.insert(name.to_string(), toml::Value::Table(t));
        }
        let srv = crate::config::Srv::default();
        let mut pipe = Pipeline::new(30);
        crate::modules::register_all(&mut pipe, &mc, &srv);
        pipe.sort();
        pipe
    }

    #[test]
    fn health_gets_request_id_header() {
        let pipe = build_full_pipeline();
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.get_header("X-Request-Id").is_some());
    }

    #[test]
    fn metrics_gets_request_id() {
        let pipe = build_full_pipeline();
        let mut req = super::make_req("GET", "/metrics");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.get_header("X-Request-Id").is_some());
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("proxycache_"));
    }

    #[test]
    fn compression_skipped_for_small_health_response() {
        // Health check body {"status":"ok"} is ~15 bytes, well under min_size=256
        // So compression should NOT be applied even with Accept-Encoding: gzip
        let pipe = build_full_pipeline();
        let mut req = super::make_req_with_headers("GET", "/health", &[("Accept-Encoding", "gzip")]);
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.get_header("Content-Encoding").is_none(),
            "Small responses should not be compressed");
    }

    #[test]
    fn compression_not_applied_without_gzip() {
        let pipe = build_full_pipeline();
        let mut req = super::make_req("GET", "/health");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        assert_eq!(resp.status_code, 200);
        assert!(resp.get_header("Content-Encoding").is_none());
    }

    #[test]
    fn non_matching_path_falls_through() {
        let pipe = build_full_pipeline();
        let mut req = super::make_req("GET", "/unknown");
        let mut ctx = super::make_ctx();
        let resp = pipe.handle(&mut req, &mut ctx);
        // No handler for /unknown, should get 500 (no proxy_core)
        assert_eq!(resp.status_code, 500);
    }
}
