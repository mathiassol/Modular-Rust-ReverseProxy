// TCP/TLS server with HTTP/1.1, HTTP/2 (ALPN), and HTTP/3 (QUIC) support
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, Shutdown};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::config::Srv;
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse, ReadResult};
use crate::modules::Pipeline;

pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE_CONNS: AtomicUsize = AtomicUsize::new(0);

pub enum ClientStream {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ServerConnection, TcpStream>),
}

impl ClientStream {
    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            ClientStream::Plain(s) => s.peer_addr(),
            ClientStream::Tls(s) => s.sock.peer_addr(),
        }
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            ClientStream::Plain(s) => s.set_read_timeout(dur),
            ClientStream::Tls(s) => s.sock.set_read_timeout(dur),
        }
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            ClientStream::Plain(s) => s.set_write_timeout(dur),
            ClientStream::Tls(s) => s.sock.set_write_timeout(dur),
        }
    }

    pub fn set_nodelay(&self, nodelay: bool) -> std::io::Result<()> {
        match self {
            ClientStream::Plain(s) => s.set_nodelay(nodelay),
            ClientStream::Tls(s) => s.sock.set_nodelay(nodelay),
        }
    }

    pub fn shutdown(&self, how: Shutdown) -> std::io::Result<()> {
        match self {
            ClientStream::Plain(s) => s.shutdown(how),
            ClientStream::Tls(s) => s.sock.shutdown(how),
        }
    }

    pub fn into_tcp_stream(self) -> Option<TcpStream> {
        match self {
            ClientStream::Plain(s) => Some(s),
            ClientStream::Tls(_) => None,
        }
    }

    pub fn tls_version(&self) -> Option<&'static str> {
        match self {
            ClientStream::Tls(s) => s.conn.protocol_version().map(|v| match v {
                rustls::ProtocolVersion::TLSv1_2 => "TLSv1.2",
                rustls::ProtocolVersion::TLSv1_3 => "TLSv1.3",
                _ => "unknown",
            }),
            _ => None,
        }
    }
}

impl Read for ClientStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ClientStream::Plain(s) => s.read(buf),
            ClientStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for ClientStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            ClientStream::Plain(s) => s.write(buf),
            ClientStream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            ClientStream::Plain(s) => s.flush(),
            ClientStream::Tls(s) => s.flush(),
        }
    }
}

pub fn active_connections() -> usize {
    ACTIVE_CONNS.load(Ordering::Acquire)
}

pub fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::Release);
}

struct ThreadPool {
    sender: Option<mpsc::SyncSender<ClientStream>>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl ThreadPool {
    fn new(
        size: usize,
        pipe: Arc<Pipeline>,
        buf_size: usize,
        write_timeout: u64,
        alt_svc: Option<String>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ClientStream>(size * 2);
        let rx = Arc::new(Mutex::new(rx));
        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            let rx = Arc::clone(&rx);
            let pipe = Arc::clone(&pipe);
            let alt = alt_svc.clone();
            workers.push(thread::spawn(move || {
                loop {
                    let stream = {
                        let lock = match rx.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        lock.recv()
                    };
                    match stream {
                        Ok(s) => {
                            let _guard = ConnGuard::new();
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                handle_h1(s, &pipe, buf_size, write_timeout, alt.as_deref());
                            }));
                            if result.is_err() {
                                crate::log::error("Panic in handler (recovered)");
                            }
                        }
                        Err(_) => break,
                    }
                }
            }));
        }

        ThreadPool { sender: Some(tx), workers }
    }

    fn dispatch(&self, stream: ClientStream) -> Result<(), ClientStream> {
        match &self.sender {
            Some(tx) => tx.try_send(stream).map_err(|e| match e {
                mpsc::TrySendError::Full(s) | mpsc::TrySendError::Disconnected(s) => s,
            }),
            None => Err(stream),
        }
    }

    fn clone_sender(&self) -> Option<mpsc::SyncSender<ClientStream>> {
        self.sender.as_ref().cloned()
    }

    fn shutdown(&mut self) {
        self.sender.take();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

struct ConnGuard;

impl ConnGuard {
    #[allow(clippy::new_without_default)]
    fn new() -> Self {
        ACTIVE_CONNS.fetch_add(1, Ordering::AcqRel);
        ConnGuard
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        ACTIVE_CONNS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct TlsAssets {
    config: Arc<rustls::ServerConfig>,
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
}

pub struct Server {
    cfg: Srv,
    pipe: Arc<Pipeline>,
}

impl Server {
    pub fn new(c: Srv, p: Pipeline) -> Self {
        Server { cfg: c, pipe: Arc::new(p) }
    }

    pub fn run(&self) -> std::io::Result<()> {
        let tls_assets = build_tls_assets(&self.cfg);
        let tls_enabled = tls_assets.is_some();

        let num_workers = if self.cfg.worker_threads > 0 {
            self.cfg.worker_threads
        } else {
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4) * 2
        };

        let alt_svc = if self.cfg.http3 && tls_enabled {
            let h3_port = if self.cfg.h3_port > 0 {
                self.cfg.h3_port
            } else {
                self.cfg.listen_addr.rsplit_once(':')
                    .and_then(|(_, p)| p.parse().ok())
                    .unwrap_or(443)
            };
            Some(format!("h3=\":{h3_port}\"; ma=86400"))
        } else {
            None
        };

        let mut protos = vec!["HTTP/1.1"];
        if tls_enabled && self.cfg.http2 { protos.push("HTTP/2"); }
        if tls_enabled && self.cfg.http3 { protos.push("HTTP/3"); }
        let proto_str = protos.join(", ");
        let scheme = if tls_enabled { "https" } else { "http" };

        crate::log::info(&format!("Listening on {} ({scheme}) [{proto_str}]", self.cfg.listen_addr));
        crate::log::info(&format!("Workers: {num_workers} | Max connections: {}", self.cfg.max_connections));
        crate::log::separator();

        install_shutdown_handler(&self.cfg.listen_addr);
        let mut pool = ThreadPool::new(
            num_workers,
            Arc::clone(&self.pipe),
            self.cfg.buffer_size,
            self.cfg.client_timeout,
            alt_svc.clone(),
        );

        let max_conns = self.cfg.max_connections;

        if let Some(assets) = tls_assets {
            self.run_tls(assets, &pool, max_conns, num_workers, alt_svc)?;
        } else {
            self.run_plain(&pool, max_conns)?;
        }

        crate::log::info("Shutting down...");
        let timeout_secs = self.cfg.shutdown_timeout;
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        crate::log::info("Draining connections...");
        let mut last_logged = 0usize;
        loop {
            let active = ACTIVE_CONNS.load(Ordering::Acquire);
            if active == 0 {
                crate::log::info("All connections drained");
                break;
            }
            if std::time::Instant::now() > deadline {
                crate::log::warn(&format!("Forcing shutdown with {active} active connections (timeout {timeout_secs}s)"));
                break;
            }
            if active != last_logged {
                crate::log::info(&format!("Waiting for {active} connection(s) to finish..."));
                last_logged = active;
            }
            thread::sleep(Duration::from_millis(100));
        }
        pool.shutdown();
        crate::log::info("Server stopped.");
        Ok(())
    }

    fn run_plain(&self, pool: &ThreadPool, max_conns: usize) -> std::io::Result<()> {
        let listener = TcpListener::bind(&self.cfg.listen_addr)?;
        listener.set_nonblocking(true)?;

        loop {
            if SHUTDOWN.load(Ordering::Acquire) { break; }

            match listener.accept() {
                Ok((stream, _)) => {
                    if ACTIVE_CONNS.load(Ordering::Acquire) >= max_conns {
                        reject_overloaded(ClientStream::Plain(stream));
                        continue;
                    }
                    if let Err(s) = pool.dispatch(ClientStream::Plain(stream)) {
                        reject_overloaded(s);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                           || e.kind() == std::io::ErrorKind::TimedOut => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    if !SHUTDOWN.load(Ordering::Acquire) {
                        crate::log::error(&format!("Accept error: {e}"));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
        Ok(())
    }

    fn run_tls(
        &self,
        assets: TlsAssets,
        pool: &ThreadPool,
        max_conns: usize,
        num_workers: usize,
        alt_svc: Option<String>,
    ) -> std::io::Result<()> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(num_workers.min(4).max(2))
            .enable_all()
            .build()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let pipeline = Arc::clone(&self.pipe);
        let pool_sender = pool.clone_sender();
        let listen_addr = self.cfg.listen_addr.clone();
        let http2_enabled = self.cfg.http2;
        let http3_enabled = self.cfg.http3;
        let h3_port = self.cfg.h3_port;
        let _buf_size = self.cfg.buffer_size;
        let _write_timeout = self.cfg.client_timeout;
        let tls_config = assets.config.clone();

        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);

            if http3_enabled {
                match build_h3_endpoint(&assets.certs, &assets.key, &listen_addr, h3_port) {
                    Ok(endpoint) => {
                        let h3_pipe = Arc::clone(&pipeline);
                        tokio::spawn(async move {
                            crate::h3_handler::run_h3_server(endpoint, h3_pipe).await;
                        });
                    }
                    Err(e) => {
                        crate::log::error(&format!("Failed to start HTTP/3: {e}"));
                    }
                }
            }

            loop {
                if SHUTDOWN.load(Ordering::Acquire) { break; }

                tokio::select! {
                    result = listener.accept() => {
                        let (tcp, addr) = match result {
                            Ok(r) => r,
                            Err(e) => {
                                if !SHUTDOWN.load(Ordering::Acquire) {
                                    crate::log::error(&format!("Accept error: {e}"));
                                }
                                continue;
                            }
                        };

                        if ACTIVE_CONNS.load(Ordering::Acquire) >= max_conns {
                            drop(tcp);
                            crate::metrics::inc_requests_err();
                            continue;
                        }

                        let acceptor = acceptor.clone();
                        let pipeline = Arc::clone(&pipeline);
                        let sender = pool_sender.clone();
                        let peer_ip = addr.ip().to_string();
                        let alt = alt_svc.clone();

                        tokio::spawn(async move {
                            let tls = match tokio::time::timeout(
                                Duration::from_secs(10),
                                acceptor.accept(tcp),
                            ).await {
                                Ok(Ok(tls)) => tls,
                                Ok(Err(e)) => {
                                    crate::log::debug(&format!("TLS handshake failed from {peer_ip}: {e}"));
                                    return;
                                }
                                Err(_) => {
                                    crate::log::debug(&format!("TLS handshake timeout from {peer_ip}"));
                                    return;
                                }
                            };

                            let alpn = tls.get_ref().1.alpn_protocol().map(|p| p.to_vec());

                            if http2_enabled && alpn.as_deref() == Some(b"h2") {
                                ACTIVE_CONNS.fetch_add(1, Ordering::AcqRel);
                                crate::metrics::inc_connections();
                                crate::h2_handler::handle_connection(tls, pipeline, peer_ip, alt).await;
                                ACTIVE_CONNS.fetch_sub(1, Ordering::AcqRel);
                            } else {
                                let (tokio_tcp, server_conn) = tls.into_inner();
                                match tokio_tcp.into_std() {
                                    Ok(std_tcp) => {
                                        let _ = std_tcp.set_nonblocking(false);
                                        let sync_stream = rustls::StreamOwned::new(server_conn, std_tcp);
                                        if let Some(ref s) = sender {
                                            if s.try_send(ClientStream::Tls(sync_stream)).is_err() {
                                                crate::log::warn("Thread pool full, dropping TLS connection");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        crate::log::warn(&format!("TcpStream conversion failed: {e}"));
                                    }
                                }
                            }
                        });
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }

            Ok::<(), std::io::Error>(())
        })?;

        Ok(())
    }
}

fn reject_overloaded(mut s: ClientStream) {
    crate::metrics::inc_requests_err();
    let resp = HttpResponse::error(503, "Server overloaded");
    let _ = s.write_all(&resp.to_bytes());
    let _ = s.shutdown(Shutdown::Both);
}

fn handle_h1(mut c: ClientStream, p: &Pipeline, buf_size: usize, write_timeout: u64, alt_svc: Option<&str>) {
    crate::metrics::inc_connections();

    if let Some(rh) = p.raw_handler() {
        match c.into_tcp_stream() {
            Some(tcp) => {
                rh.handle_raw(tcp);
                return;
            }
            None => {
                crate::log::warn("Raw TCP handler is incompatible with TLS mode");
                return;
            }
        }
    }

    let ip = c.peer_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "?".into());
    let tls_ver = c.tls_version();

    let timeout = Some(Duration::from_secs(p.timeout()));
    let _ = c.set_read_timeout(timeout);
    let _ = c.set_write_timeout(Some(Duration::from_secs(write_timeout)));
    let _ = c.set_nodelay(true);

    let raw = match crate::http::read_http_message(&mut c, buf_size) {
        ReadResult::Ok(d) => d,
        ReadResult::TimedOut => return,
        ReadResult::Error(e) => {
            if e == "headers too large" {
                let _ = c.write_all(&HttpResponse::error(431, "Request Header Fields Too Large").to_bytes());
            } else if e == "body too large" {
                let _ = c.write_all(&HttpResponse::error(413, "Payload Too Large").to_bytes());
            } else {
                let _ = c.write_all(&HttpResponse::error(400, "Bad Request").to_bytes());
            }
            crate::metrics::inc_requests_err();
            return;
        }
    };

    crate::metrics::add_bytes_in(raw.len() as u64);
    crate::metrics::inc_requests();
    let mut req = match HttpRequest::parse(&raw) {
        Some(r) => r,
        None => {
            let _ = c.write_all(&HttpResponse::error(400, "Bad Request").to_bytes());
            crate::metrics::inc_requests_err();
            return;
        }
    };

    if matches!(req.method.as_str(), "POST" | "PUT" | "PATCH")
        && req.get_header("Content-Length").is_none()
        && req.get_header("Transfer-Encoding").is_none()
    {
        let _ = c.write_all(&HttpResponse::error(411, "Length Required").to_bytes());
        crate::metrics::inc_requests_err();
        return;
    }

    crate::log::request(&req.method, &req.path, &ip);

    let mut ctx = Context::new();
    ctx.set("_client_ip", ip);
    ctx.set("_protocol", "h1".to_string());
    if let Some(ver) = tls_ver {
        ctx.set("_tls_version", ver.to_string());
    }
    let mut resp = p.handle(&mut req, &mut ctx);
    let latency = ctx.elapsed_ms() as u64;
    crate::metrics::record_latency(latency);
    if resp.status_code < 400 {
        crate::metrics::inc_requests_ok();
    } else {
        crate::metrics::inc_requests_err();
    }

    if let Some(alt) = alt_svc {
        resp.set_header("Alt-Svc", alt);
    }

    let is_cache_hit = resp.get_header("X-Cache").map(|v| v == "HIT").unwrap_or(false);
    crate::log::response(resp.status_code, ctx.elapsed_ms(), is_cache_hit);

    let out = resp.to_bytes();
    crate::metrics::add_bytes_out(out.len() as u64);
    if c.write_all(&out).is_err() {
        crate::log::warn("Failed to write response to client");
    }
    let _ = c.shutdown(Shutdown::Write);
    crate::log::separator();
}

fn build_tls_assets(cfg: &Srv) -> Option<TlsAssets> {
    if cfg.tls_cert.is_empty() || cfg.tls_key.is_empty() {
        return None;
    }

    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap_or_else(|_| {});

    let certs = load_certs(&cfg.tls_cert)?;
    let key = load_private_key(&cfg.tls_key)?;

    let mut config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs.clone(), key.clone_key())
    {
        Ok(c) => c,
        Err(e) => {
            crate::log::error(&format!("TLS config error: {e}"));
            return None;
        }
    };

    if cfg.http2 {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    } else {
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }

    config.session_storage = rustls::server::ServerSessionMemoryCache::new(2048);

    crate::log::info("TLS enabled");
    Some(TlsAssets {
        config: Arc::new(config),
        certs,
        key,
    })
}

fn load_certs(path: &str) -> Option<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            crate::log::error(&format!("Failed to open TLS cert {path}: {e}"));
            return None;
        }
    };
    let mut reader = std::io::BufReader::new(file);
    let mut certs = Vec::new();
    loop {
        match rustls_pemfile::read_one(&mut reader) {
            Ok(Some(rustls_pemfile::Item::X509Certificate(cert))) => certs.push(cert),
            Ok(None) => break,
            Ok(Some(_)) => continue,
            Err(e) => {
                crate::log::error(&format!("Failed to parse TLS cert: {e}"));
                return None;
            }
        }
    }
    if certs.is_empty() {
        crate::log::error("No certificates found in TLS cert file");
        return None;
    }
    Some(certs)
}

fn load_private_key(path: &str) -> Option<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            crate::log::error(&format!("Failed to open TLS key {path}: {e}"));
            return None;
        }
    };
    let mut reader = std::io::BufReader::new(file);
    loop {
        match rustls_pemfile::read_one(&mut reader) {
            Ok(Some(rustls_pemfile::Item::Pkcs1Key(k))) =>
                return Some(rustls::pki_types::PrivateKeyDer::Pkcs1(k)),
            Ok(Some(rustls_pemfile::Item::Pkcs8Key(k))) =>
                return Some(rustls::pki_types::PrivateKeyDer::Pkcs8(k)),
            Ok(Some(rustls_pemfile::Item::Sec1Key(k))) =>
                return Some(rustls::pki_types::PrivateKeyDer::Sec1(k)),
            Ok(None) => break,
            Ok(Some(_)) => continue,
            Err(e) => {
                crate::log::error(&format!("Failed to parse TLS key: {e}"));
                return None;
            }
        }
    }
    crate::log::error("No private key found in TLS key file");
    None
}

fn build_h3_endpoint(
    certs: &[rustls::pki_types::CertificateDer<'static>],
    key: &rustls::pki_types::PrivateKeyDer<'static>,
    listen_addr: &str,
    h3_port: u16,
) -> Result<quinn::Endpoint, Box<dyn std::error::Error + Send + Sync>> {
    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs.to_vec(), key.clone_key())?;
    crypto.alpn_protocols = vec![b"h3".to_vec()];

    let quic_config = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));

    let addr: std::net::SocketAddr = if h3_port > 0 {
        let base: std::net::SocketAddr = listen_addr.parse()
            .map_err(|e| format!("bad listen_addr: {e}"))?;
        std::net::SocketAddr::new(base.ip(), h3_port)
    } else {
        listen_addr.parse::<std::net::SocketAddr>()
            .map_err(|e| format!("bad listen_addr: {e}"))?
    };

    let endpoint = quinn::Endpoint::server(server_config, addr)?;
    Ok(endpoint)
}

fn install_shutdown_handler(listen_addr: &str) {
    let addr = listen_addr.to_string();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(200));
            if SHUTDOWN.load(Ordering::Acquire) {
                if let Ok(sa) = addr.parse::<std::net::SocketAddr>() {
                    let _ = TcpStream::connect_timeout(&sa, Duration::from_millis(100));
                }
                break;
            }
        }
    });

    #[cfg(windows)]
    {
        extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
            SHUTDOWN.store(true, Ordering::Release);
            1
        }
        extern "system" {
            fn SetConsoleCtrlHandler(
                handler: extern "system" fn(u32) -> i32,
                add: i32,
            ) -> i32;
        }
        unsafe { SetConsoleCtrlHandler(ctrl_handler, 1); }
    }

    #[cfg(unix)]
    {
        use std::sync::atomic::AtomicI32;

        static PIPE_WR: AtomicI32 = AtomicI32::new(-1);

        extern "C" fn sig_handler(_sig: libc::c_int) {
            SHUTDOWN.store(true, Ordering::Release);
            let fd = PIPE_WR.load(Ordering::Relaxed);
            if fd >= 0 {
                unsafe { libc::write(fd, b"x".as_ptr() as *const libc::c_void, 1); }
            }
        }

        unsafe {
            let mut fds = [0i32; 2];
            if libc::pipe(fds.as_mut_ptr()) == 0 {
                PIPE_WR.store(fds[1], Ordering::Relaxed);
                libc::signal(libc::SIGTERM, sig_handler as libc::sighandler_t);
                libc::signal(libc::SIGINT, sig_handler as libc::sighandler_t);
            }
        }
    }
}
