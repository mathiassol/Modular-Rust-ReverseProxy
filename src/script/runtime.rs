// Script command runtime - evaluates commands against HTTP request/response
use super::parser::Command;
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use std::collections::HashMap;

/// Evaluate on_request commands, return Some(response) to short-circuit pipeline
pub fn exec_request(
    cmds: &[Command],
    req: &mut HttpRequest,
    ctx: &mut Context,
    config: &HashMap<String, String>,
) -> Option<HttpResponse> {
    for cmd in cmds {
        if let Some(resp) = exec_request_cmd(cmd, req, ctx, config) {
            return Some(resp);
        }
    }
    None
}

/// Evaluate on_response commands, modifying the response in-place
pub fn exec_response(
    cmds: &[Command],
    req: &HttpRequest,
    resp: &mut HttpResponse,
    ctx: &mut Context,
    config: &HashMap<String, String>,
) {
    for cmd in cmds {
        exec_response_cmd(cmd, req, resp, ctx, config);
    }
}

fn exec_request_cmd(
    cmd: &Command,
    req: &mut HttpRequest,
    ctx: &mut Context,
    config: &HashMap<String, String>,
) -> Option<HttpResponse> {
    match cmd {
        Command::If { field, op, value, body } => {
            let left = resolve_field(field, req, None, ctx, config);
            let right = resolve_value(value, config);
            if eval_cond(&left, op, &right) {
                return exec_request(body, req, ctx, config);
            }
        }
        Command::Respond { code, content_type, body } => {
            let resolved_body = resolve_value(body, config);
            let resp = HttpResponse {
                version: "HTTP/1.1".to_string(),
                status_code: *code,
                status_text: status_text(*code).to_string(),
                headers: vec![
                    ("Content-Type".to_string(), content_type.clone()),
                    ("Content-Length".to_string(), resolved_body.len().to_string()),
                ],
                body: resolved_body.into_bytes(),
            };
            return Some(resp);
        }
        Command::SetHeader { name, value } => {
            let val = resolve_value(value, config);
            req.set_header(name, &val);
        }
        Command::Log { level, msg } => {
            let resolved = resolve_value(msg, config);
            match level.as_str() {
                "debug" => crate::log::debug(&resolved),
                "info" => crate::log::info(&resolved),
                "warn" => crate::log::warn(&resolved),
                "error" => crate::log::error(&resolved),
                _ => crate::log::info(&resolved),
            }
        }
        Command::SetCtx { key, value } => {
            let val = resolve_value(value, config);
            ctx.set(key, val);
        }
        Command::StdCall { func, args } => {
            let resolved_args: Vec<String> = args.iter()
                .map(|a| resolve_value(a, config))
                .collect();
            return super::stdlib::call_request(func, &resolved_args, req, ctx, config);
        }
    }
    None
}

fn exec_response_cmd(
    cmd: &Command,
    req: &HttpRequest,
    resp: &mut HttpResponse,
    ctx: &mut Context,
    config: &HashMap<String, String>,
) {
    match cmd {
        Command::If { field, op, value, body } => {
            let left = resolve_field(field, req, Some(resp), ctx, config);
            let right = resolve_value(value, config);
            if eval_cond(&left, op, &right) {
                exec_response(body, req, resp, ctx, config);
            }
        }
        Command::SetHeader { name, value } => {
            let val = resolve_value(value, config);
            resp.set_header(name, &val);
        }
        Command::Log { level, msg } => {
            let resolved = resolve_value(msg, config);
            match level.as_str() {
                "debug" => crate::log::debug(&resolved),
                "warn" => crate::log::warn(&resolved),
                "error" => crate::log::error(&resolved),
                _ => crate::log::info(&resolved),
            }
        }
        Command::SetCtx { key, value } => {
            let val = resolve_value(value, config);
            ctx.set(key, val);
        }
        Command::StdCall { func, args } => {
            let resolved_args: Vec<String> = args.iter()
                .map(|a| resolve_value(a, config))
                .collect();
            super::stdlib::call_response(func, &resolved_args, req, resp, ctx, config);
        }
        _ => {}
    }
}

fn resolve_field(
    field: &str,
    req: &HttpRequest,
    resp: Option<&HttpResponse>,
    ctx: &Context,
    config: &HashMap<String, String>,
) -> String {
    match field {
        "path" => req.path.clone(),
        "method" => req.method.clone(),
        "version" => req.version.clone(),
        "status" => resp.map(|r| r.status_code.to_string()).unwrap_or_default(),
        "client_ip" => ctx.get("_client_ip").unwrap_or("?").to_string(),
        f if f.starts_with("header.") => {
            let name = &f[7..];
            req.get_header(name).unwrap_or("").to_string()
        }
        f if f.starts_with('$') => {
            let key = &f[1..];
            config.get(key).cloned().unwrap_or_default()
        }
        _ => field.to_string(),
    }
}

fn resolve_value(value: &str, config: &HashMap<String, String>) -> String {
    if let Some(key) = value.strip_prefix('$') {
        config.get(key).cloned().unwrap_or_default()
    } else {
        value.trim_matches('"').to_string()
    }
}

fn eval_cond(left: &str, op: &str, right: &str) -> bool {
    match op {
        "==" => left == right,
        "!=" => left != right,
        "contains" => left.contains(right),
        _ => false,
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}
