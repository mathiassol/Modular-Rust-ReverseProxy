# Proxycache

A high-performance, modular HTTP reverse proxy and cache written in Rust.

## Features

- **HTTP/1.1 reverse proxy** with connection pooling and keep-alive
- **TLS termination** via rustls (no OpenSSL dependency)
- **Script module system** — extend with `.pcmod` scripts, no compilation needed
- **Built-in modules**: rate limiting, caching, compression, circuit breaker, health checks, load balancing, metrics, request ID injection, URL rewriting, admin API
- **Priority-based pipeline** — modules execute in configurable order
- **Admin API** — runtime stats, module listing, config verify/repair
- **CLI tool** (Go) — status, reload, stats, module management

## Quick Start

```bash
# Build
cargo build --release

# Run with default config
./target/release/proxycache

# Or specify a config file
./target/release/proxycache --config my-config.toml
```

## Configuration

All settings live in `config.toml`. On first run, missing module sections are auto-populated with defaults.

```toml
[server]
listen_addr = "0.0.0.0:3000"
backend_addr = "127.0.0.1:8080"
max_connections = 1000
buffer_size = 8192
log_level = "info"

[cache]
enabled = true
ttl_seconds = 300
max_size = 100

[rate_limiter]
enabled = true
requests_per_second = 100
burst = 200

[compression]
enabled = true
min_size = 256
```

## Module System

### Script Modules (.pcmod)

Drop `.pcmod` files in the `mods/` directory — they're loaded at runtime without recompilation.

```pcmod
mod my_module
version 1.0
priority 75

config {
  enabled bool true
  greeting str "hello"
}

on_request {
  if path == "/hello" {
    set_header "X-Greeting" $greeting
    respond 200 text "Hello, World!"
  }
}
```

See `mods/examples/` for 1:1 script equivalents of every built-in module.

### STD Library

Script modules call into a rich standard library:

| Function | Description |
|---|---|
| `std.rate_limit` | Token-bucket rate limiting |
| `std.cache.check` / `std.cache.store` | Response caching |
| `std.circuit_breaker.check` / `.record` | Circuit breaker pattern |
| `std.compress.check` / `.apply` | Gzip compression |
| `std.request_id.inject` | Add X-Request-ID header |
| `std.url_rewrite` | Path rewriting |
| `std.load_balance` | Round-robin backend selection |
| `std.proxy.forward` | Forward request to backend |
| `std.active_health` | Background health monitoring |
| `std.metrics.prometheus` | Prometheus metrics endpoint |

### Rust Import Modules

For advanced users who need full Rust access, place `.rs` files in `imports/`:

```bash
imports/my_custom.rs   # Auto-detected by build.rs
cargo build            # Recompile to include
```

The file must implement the `Module` trait. See `imports/README.md` for details.

## CLI

```bash
cd cli && go build -o proxycache-cli .

proxycache-cli status          # Server status
proxycache-cli stats           # Traffic metrics
proxycache-cli reload          # Reload configuration
proxycache-cli mods            # List loaded modules
proxycache-cli verify          # Verify config integrity
proxycache-cli repair          # Auto-repair config
proxycache-cli help            # Show all commands
```

## Architecture

```
src/
├── main.rs            # Entry point, pipeline setup
├── server.rs          # TCP/TLS listener, thread pool, shutdown
├── config.rs          # TOML config loading + validation
├── context.rs         # Per-request context
├── pool.rs            # Connection pool with idle eviction
├── metrics.rs         # Atomic counter metrics
├── log.rs             # Leveled logging with colors
├── colors.rs          # ANSI color codes
├── http/              # HTTP parsing (request, response, chunked)
├── script/            # Script module engine
│   ├── parser.rs      # .pcmod format parser
│   ├── runtime.rs     # Command execution engine
│   ├── stdlib.rs      # STD library (all proxy functions)
│   └── loader.rs      # Module loader with priority support
└── modules/           # Built-in Rust modules (auto-generated mod.rs)

build.rs               # Code generation: Module trait, Pipeline, registration
mods/                  # User script modules (.pcmod)
imports/               # User Rust modules (.rs, needs rebuild)
cli/                   # Go CLI tool
```

## Admin API

When `admin_api` is enabled, endpoints are available on the configured port:

| Endpoint | Description |
|---|---|
| `GET /status` | Server uptime, connections, version |
| `GET /stats` | Request/response counters, latency, pool stats |
| `GET /mods` | List all loaded modules with metadata |
| `GET /config/verify` | Check config for missing/invalid sections |
| `POST /config/repair` | Auto-add missing module defaults |
| `POST /reload` | Reload configuration |

Protect with `api_key` in config:
```toml
[admin_api]
enabled = true
listen_addr = "127.0.0.1:9090"
api_key = "your-secret-key"
```

## Building

**Requirements**: Rust 1.70+ (edition 2021)

```bash
cargo build --release
```

**CLI**: Go 1.21+

```bash
cd cli && go build -o proxycache-cli .
```
