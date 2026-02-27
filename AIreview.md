# Proxycache Code Review

## Project Overview
Proxycache is a high-performance HTTP reverse proxy and caching server written in Rust. It supports HTTP/1.1, HTTP/2 (h2), and HTTP/3 (QUIC) protocols with TLS termination. The system uses a modular pipeline architecture where requests pass through configurable modules in priority order before reaching the backend.

## Architecture

### Entry Point (main.rs)
- Initializes metrics, loads modules and configuration
- Collects default configurations from built-in modules and script modules
- Creates a Pipeline and registers all modules in priority order
- Starts the server with the configured pipeline
- Loads user-defined script modules (.pcmod files) at runtime

### Server (server.rs)
- TCP listener binding to configured address
- Supports both plain and TLS connections via ClientStream enum
- Manages connection lifecycle with read/write timeouts and TCP_NODELAY
- Handles graceful shutdown via SHUTDOWN atomic flag
- Tracks active connection count

### Configuration (config.rs)
- Loads TOML configuration file
- Defines server settings: listen address, backend address, buffer size, timeouts, TLS paths
- Validates socket addresses, buffer sizes, timeouts
- Provides default values for all configuration options
- Auto-repairs/populates missing module sections

### Context (context.rs)
- Per-request state container storing strings and type-erased values
- Tracks request start time for latency measurement
- Provides get/set/take methods for context variables

### HTTP Handling (http/)
- **request.rs**: Parses HTTP requests, validates method/version, extracts headers and body
- **response.rs**: Parses HTTP responses, builds error responses with appropriate status codes
- **mod.rs**: Helper functions for header lookup, chunk detection, HTTP message reading

### Connection Pool (pool.rs)
- Global shared connection pool (OnceLock pattern)
- Maintains idle TCP connections per backend address
- Max 8 idle connections per host with 30-second eviction
- Probes connections with non-blocking read to detect broken connections
- Tracks pool hits/misses via metrics

### Metrics (metrics.rs)
- Lock-free atomic counters for requests, bytes, latency, connections
- Snapshot generation for JSON and Prometheus output
- Averages latency across total requests
- Tracks circuit breaker trips and rejects

### Logging (log.rs)
- Centralized logging system with atomic level control
- Four log levels: DEBUG, INFO, WARN, ERROR
- Custom timestamp formatting (YYYY-MM-DD HH:MM:SS.mmm)
- Date calculation without external libraries (days_to_ymd function)
- Color-coded output via ANSI codes

### Module System (modules/)
- **mod.rs**: Defines Module trait (handle, on_response) and Pipeline
- Pipeline executes modules in priority order until one returns a response
- Supports module overrides via overrides() trait method
- Default priorities: active_health(10), request_id(20), rate_limiter(30), circuit_breaker(40), health_check(50), metrics_exporter(60), admin_api(70), cache(80), url_rewriter(90), compression(100), load_balancer(110), proxy_core(120), raw_tcp(130)

### Built-in Modules

#### proxy_core.rs
- Forwards requests to backend using connection pool
- Reads response from backend
- Handles connection keep-alive based on HTTP version and Connection header
- Returns 502 errors for connection failures, 504 for timeouts

#### cache.rs
- In-memory HashMap cache with TTL
- Eviction thread runs every 30 seconds
- Supports cache warming with configurable URLs
- Configurable max size and TTL

#### rate_limiter.rs
- Token-bucket algorithm per client IP
- Maintains up to 50,000 buckets with 300-second stale timeout
- Configurable requests per second and burst allowance
- Returns 429 status when rate limit exceeded

#### circuit_breaker.rs
- Three-state machine: CLOSED, OPEN, HALF_OPEN
- Opens after failure threshold reached
- Attempts recovery after timeout period
- Records failures and rejects via metrics

#### compression.rs
- Gzip compression for responses
- Respects Accept-Encoding header
- Configurable minimum response size threshold

#### request_id.rs
- Generates unique X-Request-ID headers
- UUID-based identifier per request

#### url_rewriter.rs
- Path and query string rewriting based on configuration rules

#### health_check.rs
- Passive health monitoring (per-response)
- Tracks backend availability

#### active_health.rs
- Background health check thread
- Periodic probes to backend

#### load_balancer.rs
- Round-robin backend selection
- Supports multiple backend addresses

#### metrics_exporter.rs
- Exposes metrics via /metrics endpoint
- Supports both JSON and Prometheus formats

#### admin_api.rs
- Separate TCP listener on configurable address
- Endpoints: /status, /stats, /mods, /config/verify, /config/repair, /reload
- Optional API key protection
- Thread pool for handling concurrent requests (max 16 connections)

#### raw_tcp.rs
- Passthrough for non-HTTP protocols

### Script Module System (script/)
- **parser.rs**: Parses .pcmod script format
  - Defines ScriptDef with name, version, priority, config, commands
  - Command types: If, Respond, SetHeader, Log, SetCtx, StdCall
  - Supports boolean, integer, string, and list config field types
  - Enforces max 10,000 line limit per script

- **runtime.rs**: Executes parsed script commands
  - Variable substitution with $ prefix
  - Command execution engine for if/respond/header/log operations

- **stdlib.rs**: Standard library exposed to scripts
  - Functions: std.rate_limit, std.cache.check/store, std.circuit_breaker.check/record, std.compress.check/apply, std.request_id.inject, std.url_rewrite, std.load_balance, std.proxy.forward, std.active_health, std.metrics.prometheus

- **loader.rs**: Loads .pcmod files from mods/ directory at startup
  - Generates config tables for each script module
  - Registers script modules with pipeline

### Protocol Support
- **h2_handler.rs**: HTTP/2 connection handling via h2 crate
  - Async connection handler with stream multiplexing
  - Converts h2 frames to HttpRequest/HttpResponse

- **h3_handler.rs**: HTTP/3 connection handling via quinn + h3
  - QUIC connection support
  - Configurable port separate from main listener

## Data Flow

1. Client connects to TCP listener (plain or TLS)
2. HTTP message parsed into HttpRequest or h2/h3 frames
3. Per-request Context created (started_at, empty strings/state maps)
4. Pipeline.handle() executes modules in priority order:
   - Each module can inspect request, modify headers, return response
   - First module to return response wins
   - All modules up to the responding module get on_response hook
5. Response sent back to client
6. Metrics updated (latency, bytes, status counts)

## Configuration Hierarchy
1. Command-line --config argument (default: config.toml)
2. Default values from built-in modules
3. Script module defaults merged in
4. TOML file values override defaults
5. Validation applied to server config
6. Auto-repair adds missing module sections with defaults

## Concurrency Model
- Main thread: configuration loading, server setup
- Listener thread: accepts connections
- Worker threads: handle individual connections (tokio runtime)
- Background threads: cache eviction, health checks, admin API, metrics collection
- Lock-free metrics via atomic counters
- Mutex for connection pool, cache, rate limiter buckets
- Atomic flags for shutdown coordination

## Error Handling
- Invalid configuration: logged and uses defaults
- Connection errors: 502 Bad Gateway
- Timeout errors: 504 Gateway Timeout
- Rate limit exceeded: 429 Too Many Requests
- Parse failures: 400 Bad Request or 431 Request Header Fields Too Large
- Oversized bodies: 413 Payload Too Large

## Feature Capabilities
- Request/response header manipulation
- Backend connection pooling with keep-alive
- TLS termination with configurable certificates
- Protocol upgrade detection (ALPN)
- Client IP extraction from X-Forwarded-For or peer address
- Response body modification (compression)
- Request path rewriting
- Backend health monitoring (active and passive)
- Traffic shaping (rate limiting)
- Failure protection (circuit breaker)
- Request correlation (request ID)
- Performance observability (metrics, logging)
- Runtime configuration reload
