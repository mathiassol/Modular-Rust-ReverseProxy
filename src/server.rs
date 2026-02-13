// TCP/TLS server with connection handling and graceful shutdown
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

    /// Extract the inner TcpStream (for raw handler). Returns None for TLS streams.
    pub fn into_tcp_stream(self) -> Option<TcpStream> {
        match self {
            ClientStream::Plain(s) => Some(s),
            ClientStream::Tls(_) => None,
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
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ClientStream>(size * 2);
        let rx = Arc::new(Mutex::new(rx));
        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            let rx = Arc::clone(&rx);
            let pipe = Arc::clone(&pipe);
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
                                handle(s, &pipe, buf_size, write_timeout);
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

    /// Drop the sender to signal workers, then join all threads.
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
        ACTIVE_CONNS.fetch_add(1, Ordering::Release);
        ConnGuard
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        ACTIVE_CONNS.fetch_sub(1, Ordering::Release);
    }
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
        let listener = TcpListener::bind(&self.cfg.listen_addr)?;

        let tls_config = build_tls_config(&self.cfg);
        let tls_enabled = tls_config.is_some();

        let num_workers = if self.cfg.worker_threads > 0 {
            self.cfg.worker_threads
        } else {
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4) * 2
        };

        let proto = if tls_enabled { "https" } else { "http" };
        crate::log::info(&format!("Listening on {} ({})", self.cfg.listen_addr, proto));
        crate::log::info(&format!("Workers: {num_workers} | Max connections: {}", self.cfg.max_connections));
        crate::log::separator();

        install_shutdown_handler(&self.cfg.listen_addr);
        let mut pool = ThreadPool::new(
            num_workers,
            Arc::clone(&self.pipe),
            self.cfg.buffer_size,
            self.cfg.client_timeout,
        );

        let max_conns = self.cfg.max_connections;

        #[cfg(windows)]
        {
            listener.set_nonblocking(true)?;
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::io::AsRawFd;
            let fd = listener.as_raw_fd();
            let tv = libc::timeval { tv_sec: 0, tv_usec: 100_000 };
            unsafe {
                libc::setsockopt(
                    fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO,
                    &tv as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                );
            }
        }

        loop {
            if SHUTDOWN.load(Ordering::Acquire) {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    let active = ACTIVE_CONNS.load(Ordering::Acquire);
                    if active >= max_conns {
                        let _ = reject_overloaded(ClientStream::Plain(stream));
                        continue;
                    }
                    let client = if let Some(ref tls_cfg) = tls_config {
                        match rustls::ServerConnection::new(Arc::clone(tls_cfg)) {
                            Ok(conn) => ClientStream::Tls(rustls::StreamOwned::new(conn, stream)),
                            Err(e) => {
                                crate::log::error(&format!("TLS session init failed: {e}"));
                                continue;
                            }
                        }
                    } else {
                        ClientStream::Plain(stream)
                    };
                    if let Err(s) = pool.dispatch(client) {
                        let _ = reject_overloaded(s);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                           || e.kind() == std::io::ErrorKind::TimedOut => {
                    #[cfg(windows)]
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
}

fn reject_overloaded(mut s: ClientStream) {
    crate::metrics::inc_requests_err();
    let resp = HttpResponse::error(503, "Server overloaded");
    let _ = s.write_all(&resp.to_bytes());
    let _ = s.shutdown(Shutdown::Both);
}

fn handle(mut c: ClientStream, p: &Pipeline, buf_size: usize, write_timeout: u64) {
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
    let resp = p.handle(&mut req, &mut ctx);
    let latency = ctx.elapsed_ms() as u64;
    crate::metrics::record_latency(latency);
    if resp.status_code < 400 {
        crate::metrics::inc_requests_ok();
    } else {
        crate::metrics::inc_requests_err();
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

/// Build TLS ServerConfig from cert/key paths. Returns None if TLS is not configured.
fn build_tls_config(cfg: &Srv) -> Option<Arc<rustls::ServerConfig>> {
    if cfg.tls_cert.is_empty() || cfg.tls_key.is_empty() {
        return None;
    }

    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap_or_else(|_| {});

    let cert_path = &cfg.tls_cert;
    let key_path = &cfg.tls_key;

    let cert_file = match std::fs::File::open(cert_path) {
        Ok(f) => f,
        Err(e) => {
            crate::log::error(&format!("Failed to open TLS cert {cert_path}: {e}"));
            return None;
        }
    };
    let key_file = match std::fs::File::open(key_path) {
        Ok(f) => f,
        Err(e) => {
            crate::log::error(&format!("Failed to open TLS key {key_path}: {e}"));
            return None;
        }
    };

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let mut reader = std::io::BufReader::new(cert_file);
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
        certs
    };

    if certs.is_empty() {
        crate::log::error("No certificates found in TLS cert file");
        return None;
    }

    let private_key: rustls::pki_types::PrivateKeyDer<'static> = {
        let mut reader = std::io::BufReader::new(key_file);
        let mut key = None;
        loop {
            match rustls_pemfile::read_one(&mut reader) {
                Ok(Some(rustls_pemfile::Item::Pkcs1Key(k))) => {
                    key = Some(rustls::pki_types::PrivateKeyDer::Pkcs1(k));
                    break;
                }
                Ok(Some(rustls_pemfile::Item::Pkcs8Key(k))) => {
                    key = Some(rustls::pki_types::PrivateKeyDer::Pkcs8(k));
                    break;
                }
                Ok(Some(rustls_pemfile::Item::Sec1Key(k))) => {
                    key = Some(rustls::pki_types::PrivateKeyDer::Sec1(k));
                    break;
                }
                Ok(None) => break,
                Ok(Some(_)) => continue,
                Err(e) => {
                    crate::log::error(&format!("Failed to parse TLS key: {e}"));
                    return None;
                }
            }
        }
        match key {
            Some(k) => k,
            None => {
                crate::log::error("No private key found in TLS key file");
                return None;
            }
        }
    };

    let config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
    {
        Ok(c) => c,
        Err(e) => {
            crate::log::error(&format!("TLS config error: {e}"));
            return None;
        }
    };

    crate::log::info("TLS enabled");
    Some(Arc::new(config))
}

fn install_shutdown_handler(listen_addr: &str) {
    let addr = listen_addr.to_string();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(200));
            if SHUTDOWN.load(Ordering::Acquire) {
                let _ = TcpStream::connect_timeout(
                    &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
                    Duration::from_millis(100),
                );
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
