// Connection pool for backend TCP connections
use std::collections::HashMap;
use std::net::{TcpStream, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_IDLE_PER_HOST: usize = 8;
const MAX_IDLE_AGE: Duration = Duration::from_secs(30);

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
        if let Ok(mut map) = self.idle.lock() {
            if let Some(conns) = map.get_mut(addr) {
                while let Some(pooled) = conns.pop() {
                    if pooled.created.elapsed() > MAX_IDLE_AGE {
                        continue;
                    }
                    if let Ok(s) = pooled.stream.try_clone() {
                        let _ = s.set_nonblocking(true);
                        let mut probe = [0u8; 1];
                        match std::io::Read::read(&mut &s, &mut probe) {
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                let _ = pooled.stream.set_nonblocking(false);
                                crate::metrics::inc_pool_hits();
                                return Ok(pooled.stream);
                            }
                            _ => continue,
                        }
                    }
                }
            }
        }

        crate::metrics::inc_pool_misses();
        let stream = TcpStream::connect_timeout(addr, timeout)?;
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }

    pub fn put(&self, addr: SocketAddr, stream: TcpStream) {
        if let Ok(mut map) = self.idle.lock() {
            let conns = map.entry(addr).or_insert_with(Vec::new);
            conns.retain(|p| p.created.elapsed() < MAX_IDLE_AGE);
            if conns.len() < MAX_IDLE_PER_HOST {
                conns.push(Pooled { stream, created: Instant::now() });
            }
        }
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Ok(mut map) = self.idle.lock() {
            map.clear();
        }
    }
}
