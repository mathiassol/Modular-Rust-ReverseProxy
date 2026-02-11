// Centralized logging system
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::colors::*;

static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn init(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn active() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn info(msg: &str) {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "{BOLD}{CYAN}{msg}{RESET}");
    let _ = io::stdout().flush();
}

pub fn warn(msg: &str) {
    if !active() { return; }
    let _ = writeln!(io::stderr(), "{YELLOW}⚠ {msg}{RESET}");
    let _ = io::stderr().flush();
}

pub fn error(msg: &str) {
    let _ = writeln!(io::stderr(), "{RED}✗ {msg}{RESET}");
    let _ = io::stderr().flush();
}

pub fn module_loaded(name: &str) {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "  {GREEN}✓{RESET} {name}");
    let _ = io::stdout().flush();
}

pub fn module_skipped(name: &str) {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "  {YELLOW}⊘ {name} [overridden]{RESET}");
    let _ = io::stdout().flush();
}

pub fn request(method: &str, path: &str, ip: &str) {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "{YELLOW}→{RESET} {BOLD}{method}{RESET} {path} from {ip}");
    let _ = io::stdout().flush();
}

pub fn response(status: u16, ms: u128, is_cache_hit: bool) {
    if !active() { return; }
    let col = status_color(status);
    let source = if is_cache_hit {
        format!(" {CYAN}[CACHE HIT]{RESET}")
    } else {
        format!(" {YELLOW}[BACKEND]{RESET}")
    };
    let _ = writeln!(io::stdout(), "{GREEN}←{RESET} {BOLD}{col}{status}{RESET} ({ms}ms){source}");
    let _ = io::stdout().flush();
}

pub fn separator() {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "{SEPARATOR}");
    let _ = io::stdout().flush();
}
