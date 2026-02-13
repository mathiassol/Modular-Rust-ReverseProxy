// .pcmod file parser
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ScriptDef {
    pub name: String,
    pub version: String,
    pub priority: i32,
    pub overrides: Vec<String>,
    pub config: Vec<ConfigField>,
    pub on_request: Vec<Command>,
    pub on_response: Vec<Command>,
    pub on_init: Vec<Command>,
}

#[derive(Debug, Clone)]
pub struct ConfigField {
    pub key: String,
    #[allow(dead_code)]
    pub typ: FieldType,
    pub default: FieldValue,
}

#[derive(Debug, Clone)]
pub enum FieldType { Bool, Int, Str, List }

#[derive(Debug, Clone)]
pub enum FieldValue {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum Command {
    If { field: String, op: String, value: String, body: Vec<Command> },
    Respond { code: u16, content_type: String, body: String },
    SetHeader { name: String, value: String },
    Log { level: String, msg: String },
    SetCtx { key: String, value: String },
    StdCall { func: String, args: Vec<String> },
}

impl FieldValue {
    pub fn to_toml(&self) -> toml::Value {
        match self {
            FieldValue::Bool(b) => toml::Value::Boolean(*b),
            FieldValue::Int(i) => toml::Value::Integer(*i),
            FieldValue::Str(s) => toml::Value::String(s.clone()),
            FieldValue::List(v) => toml::Value::Array(
                v.iter().map(|s| toml::Value::String(s.clone())).collect()
            ),
        }
    }
}

pub fn parse(src: &str) -> Result<ScriptDef, String> {
    let mut def = ScriptDef {
        name: String::new(),
        version: "1.0".into(),
        priority: 75,
        overrides: Vec::new(),
        config: Vec::new(),
        on_request: Vec::new(),
        on_response: Vec::new(),
        on_init: Vec::new(),
    };

    let lines: Vec<&str> = src.lines().collect();
    if lines.len() > 10_000 {
        return Err("script too large (>10,000 lines)".into());
    }
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        if line.is_empty() || line.starts_with('#') { continue; }

        if let Some(rest) = line.strip_prefix("mod ") {
            def.name = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("version ") {
            def.version = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("priority ") {
            def.priority = rest.trim().parse().unwrap_or(75);
        } else if let Some(rest) = line.strip_prefix("overrides ") {
            let inner = rest.trim().trim_start_matches('[').trim_end_matches(']');
            def.overrides = inner.split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if line == "config {" {
            while i < lines.len() {
                let cl = lines[i].trim();
                i += 1;
                if cl == "}" { break; }
                if cl.is_empty() || cl.starts_with('#') { continue; }
                if let Some(field) = parse_config_field(cl) {
                    def.config.push(field);
                }
            }
        } else if line == "on_request {" {
            let (cmds, new_i) = parse_block(&lines, i);
            def.on_request = cmds;
            i = new_i;
        } else if line == "on_response {" {
            let (cmds, new_i) = parse_block(&lines, i);
            def.on_response = cmds;
            i = new_i;
        } else if line == "on_init {" {
            let (cmds, new_i) = parse_block(&lines, i);
            def.on_init = cmds;
            i = new_i;
        }
    }

    if def.name.is_empty() {
        return Err("missing 'mod' declaration".into());
    }
    Ok(def)
}

fn parse_config_field(line: &str) -> Option<ConfigField> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() < 3 { return None; }
    let key = parts[0].to_string();
    let typ = match parts[1] {
        "bool" => FieldType::Bool,
        "int" => FieldType::Int,
        "str" => FieldType::Str,
        "list" => FieldType::List,
        _ => return None,
    };
    let val_str = parts[2];
    let default = match &typ {
        FieldType::Bool => FieldValue::Bool(val_str == "true"),
        FieldType::Int => FieldValue::Int(val_str.parse().unwrap_or(0)),
        FieldType::Str => FieldValue::Str(val_str.trim_matches('"').to_string()),
        FieldType::List => {
            let inner = val_str.trim_start_matches('[').trim_end_matches(']');
            if inner.trim().is_empty() {
                FieldValue::List(Vec::new())
            } else {
                FieldValue::List(inner.split(',')
                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                    .collect())
            }
        }
    };
    Some(ConfigField { key, typ, default })
}

fn parse_block(lines: &[&str], start: usize) -> (Vec<Command>, usize) {
    let mut cmds = Vec::new();
    let mut i = start;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        if line == "}" { break; }
        if line.is_empty() || line.starts_with('#') { continue; }

        if let Some(cmd) = parse_command(line, lines, &mut i) {
            cmds.push(cmd);
        }
    }
    (cmds, i)
}

fn parse_command(line: &str, lines: &[&str], i: &mut usize) -> Option<Command> {
    // if <field> <op> <value> {
    if line.starts_with("if ") {
        let rest = &line[3..];
        let rest = rest.trim_end_matches('{').trim();
        let (field, op, value) = parse_condition(rest)?;
        let (body, new_i) = parse_block(lines, *i);
        *i = new_i;
        return Some(Command::If { field, op, value, body });
    }

    // respond <code> <type> <body>
    if line.starts_with("respond ") {
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        if parts.len() >= 4 {
            let code: u16 = parts[1].parse().unwrap_or(200);
            let ct = match parts[2] {
                "json" => "application/json",
                "text" => "text/plain",
                _ => "text/plain",
            };
            let body = parts[3].to_string();
            return Some(Command::Respond {
                code,
                content_type: ct.to_string(),
                body,
            });
        }
    }

    // set_header <name> <value>
    if line.starts_with("set_header ") {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 3 {
            return Some(Command::SetHeader {
                name: parts[1].to_string(),
                value: parts[2].to_string(),
            });
        }
    }

    // log <level> <msg>
    if line.starts_with("log ") {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 3 {
            return Some(Command::Log {
                level: parts[1].to_string(),
                msg: parts[2].to_string(),
            });
        }
    }

    // set <key> <value>
    if line.starts_with("set ") {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 3 {
            return Some(Command::SetCtx {
                key: parts[1].to_string(),
                value: parts[2].to_string(),
            });
        }
    }

    // std.<func> [args...]
    if line.starts_with("std.") {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let func = parts[0].strip_prefix("std.").unwrap_or("").to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        return Some(Command::StdCall { func, args });
    }

    None
}

fn parse_condition(s: &str) -> Option<(String, String, String)> {
    for op in &["==", "!=", "contains"] {
        if let Some(idx) = s.find(op) {
            let field = s[..idx].trim().to_string();
            let value = s[idx + op.len()..].trim().trim_matches('"').to_string();
            return Some((field, op.to_string(), value));
        }
    }
    None
}

pub fn default_config_table(def: &ScriptDef) -> toml::map::Map<String, toml::Value> {
    let mut t = toml::map::Map::new();
    for field in &def.config {
        t.insert(field.key.clone(), field.default.to_toml());
    }
    t
}

/// Get all module names that a ScriptDef defines
pub fn resolve_config(def: &ScriptDef, cfg: &HashMap<String, toml::Value>) -> HashMap<String, String> {
    let mut resolved = HashMap::new();
    let mod_cfg = cfg.get(&def.name);

    for field in &def.config {
        let val = mod_cfg
            .and_then(|v| v.get(&field.key))
            .map(|v| match v {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                toml::Value::Array(a) => {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                }
                other => other.to_string(),
            })
            .unwrap_or_else(|| match &field.default {
                FieldValue::Bool(b) => b.to_string(),
                FieldValue::Int(i) => i.to_string(),
                FieldValue::Str(s) => s.clone(),
                FieldValue::List(l) => l.join(","),
            });
        resolved.insert(field.key.clone(), val);
    }
    resolved
}
