// ANSI color codes for terminal output
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";

pub fn status_color(c: u16) -> &'static str {
    match c {
        200..=299 => GREEN,
        300..=399 => CYAN,
        400..=499 => YELLOW,
        _ => RED,
    }
}

pub const SEPARATOR: &str = "\x1b[90m──────────────────────────────────────────\x1b[0m";
