// Centralized logging system with timestamps and log levels
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::SystemTime;

use crate::colors::*;

static ENABLED: AtomicBool = AtomicBool::new(true);
static LOG_LEVEL: AtomicU8 = AtomicU8::new(0);

const LEVEL_DEBUG: u8 = 0;
const LEVEL_INFO: u8 = 1;
const LEVEL_WARN: u8 = 2;
const LEVEL_ERROR: u8 = 3;

pub fn init(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_level(level: &str) {
    let l = match level.to_lowercase().as_str() {
        "debug" => LEVEL_DEBUG,
        "info" => LEVEL_INFO,
        "warn" | "warning" => LEVEL_WARN,
        "error" => LEVEL_ERROR,
        _ => LEVEL_INFO,
    };
    LOG_LEVEL.store(l, Ordering::Relaxed);
}

fn active() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn above_level(level: u8) -> bool {
    level >= LOG_LEVEL.load(Ordering::Relaxed)
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let total_days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    let (year, month, day) = days_to_ymd(total_days);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Convert days since epoch to (year, month, day)
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = is_leap(y);
    let months: [u64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 0;
    for &dm in &months {
        if remaining < dm { break; }
        remaining -= dm;
        mo += 1;
    }
    (y, mo + 1, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[allow(dead_code)]
pub fn debug(msg: &str) {
    if !active() || !above_level(LEVEL_DEBUG) { return; }
    let ts = timestamp();
    let _ = writeln!(io::stdout(), "{DIM}{ts}{RESET} {DIM}DBG{RESET} {msg}");
    let _ = io::stdout().flush();
}

pub fn info(msg: &str) {
    if !active() || !above_level(LEVEL_INFO) { return; }
    let ts = timestamp();
    let _ = writeln!(io::stdout(), "{DIM}{ts}{RESET} {BOLD}{CYAN}{msg}{RESET}");
    let _ = io::stdout().flush();
}

pub fn warn(msg: &str) {
    if !active() || !above_level(LEVEL_WARN) { return; }
    let ts = timestamp();
    let _ = writeln!(io::stderr(), "{DIM}{ts}{RESET} {YELLOW}⚠ {msg}{RESET}");
    let _ = io::stderr().flush();
}

pub fn error(msg: &str) {
    if !active() { return; }
    let ts = timestamp();
    let _ = writeln!(io::stderr(), "{DIM}{ts}{RESET} {RED}✗ {msg}{RESET}");
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
    if !active() || !above_level(LEVEL_INFO) { return; }
    let ts = timestamp();
    let _ = writeln!(io::stdout(), "{DIM}{ts}{RESET} {YELLOW}→{RESET} {BOLD}{method}{RESET} {path} from {ip}");
    let _ = io::stdout().flush();
}

pub fn response(status: u16, ms: u128, is_cache_hit: bool) {
    if !active() || !above_level(LEVEL_INFO) { return; }
    let ts = timestamp();
    let col = status_color(status);
    let source = if is_cache_hit { format!(" {CYAN}[CACHE HIT]{RESET}") } else { String::new() };
    let _ = writeln!(io::stdout(), "{DIM}{ts}{RESET} {GREEN}←{RESET} {BOLD}{col}{status}{RESET} ({ms}ms){source}");
    let _ = io::stdout().flush();
}

pub fn separator() {
    if !active() { return; }
    let _ = writeln!(io::stdout(), "{SEPARATOR}");
    let _ = io::stdout().flush();
}
