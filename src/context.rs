// Per-request context for pipeline state management
use std::any::Any;
use std::collections::HashMap;
use std::time::Instant;

#[allow(dead_code)]
pub struct Context {
    strings: HashMap<String, String>,
    state: HashMap<String, Box<dyn Any + Send + Sync>>,
    pub started_at: Instant,
}

#[allow(dead_code)]
impl Context {
    pub fn new() -> Self {
        Context {
            strings: HashMap::new(),
            state: HashMap::new(),
            started_at: Instant::now(),
        }
    }

    pub fn set(&mut self, k: &str, v: String) {
        self.strings.insert(k.to_string(), v);
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.strings.get(k).map(|v| v.as_str())
    }

    pub fn put<T: Any + Send + Sync>(&mut self, k: &str, v: T) {
        self.state.insert(k.to_string(), Box::new(v));
    }

    pub fn take<T: Any + Send + Sync>(&self, k: &str) -> Option<&T> {
        self.state.get(k).and_then(|v| v.downcast_ref::<T>())
    }

    pub fn elapsed_ms(&self) -> u128 {
        self.started_at.elapsed().as_millis()
    }
}
