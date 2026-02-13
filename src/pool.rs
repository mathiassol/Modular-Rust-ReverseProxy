// Connection pool for backend TCP connections
use std::collections::HashMap;
use std::net::{TcpStream, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_IDLE_PER_HOST: usize = 8;
const MAX_IDLE_AGE: Duration = Duration::from_secs(30);

static GLOBAL_POOL: OnceLock<ConnPool> = OnceLock::new();

/// Global shared connection pool
pub fn global_pool() -> &'static ConnPool {
    GLOBAL_POOL.get_or_init(ConnPool::new)
}

struct Pooled {
    stream: TcpStream,
    created: Instant,
}

pub struct ConnPool {
    idle: Mutex<HashMap<SocketAddr, Vec<Pooled>>>,
}

impl ConnPool {
    pub fn new() -> Self {
        ConnPool {
            idle: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, addr: &SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
        let mut map = match self.idle.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                crate::log::warn("pool: mutex recovered after panic, purging stale connections");
                let mut inner = poisoned.into_inner();
                let now = Instant::now();
                for conns in inner.values_mut() {
                    conns.retain(|p| p.created <= now && p.created.elapsed() < MAX_IDLE_AGE);
                }
                inner.retain(|_, v| !v.is_empty());
                inner
            }
        };
        if let Some(conns) = map.get_mut(addr) {
            while let Some(pooled) = conns.pop() {
                if pooled.created.elapsed() > MAX_IDLE_AGE {
                    continue;
                }
                let stream = pooled.stream;
                if let Ok(Some(err)) = stream.take_error() {
                    crate::log::debug(&format!("pool: socket has error: {err}"));
                    continue;
                }
                if let Err(e) = stream.set_nonblocking(true) {
                    crate::log::debug(&format!("pool: set_nonblocking failed: {e}"));
                    continue;
                }
                let mut probe = [0u8; 1];
                match std::io::Read::read(&mut &stream, &mut probe) {
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if let Err(e) = stream.set_nonblocking(false) {
                            crate::log::debug(&format!("pool: set_blocking failed: {e}"));
                            continue;
                        }
                        crate::metrics::inc_pool_hits();
                        return Ok(stream);
                    }
                    Ok(0) => {
                        continue;
                    }
                    _ => continue,
                }
            }
        }
        drop(map);

        crate::metrics::inc_pool_misses();
        let stream = TcpStream::connect_timeout(addr, timeout)?;
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }

    pub fn put(&self, addr: SocketAddr, stream: TcpStream) {
        let mut map = match self.idle.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                crate::log::warn("pool: mutex recovered after panic, clearing pool");
                let mut inner = poisoned.into_inner();
                inner.clear();
                inner
            }
        };
        let conns = map.entry(addr).or_insert_with(Vec::new);
        conns.retain(|p| p.created.elapsed() < MAX_IDLE_AGE);
        if conns.len() < MAX_IDLE_PER_HOST {
            conns.push(Pooled { stream, created: Instant::now() });
        }
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        match self.idle.lock() {
            Ok(mut g) => g.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        };
    }
}
