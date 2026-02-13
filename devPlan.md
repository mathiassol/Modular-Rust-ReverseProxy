# Proxycache Development Plan
*Generated: 2026-02-12*
*Last audit: 2026-02-12 — HTTP/2 + HTTP/3 support added*

---

## 📊 PROJECT HEALTH

### ✅ What's Good
- **Clean build**: 0 errors, 0 warnings
- **Multi-protocol**: HTTP/1.1, HTTP/2 (ALPN on TLS), HTTP/3 (QUIC) all supported
- **TLS**: Proper ALPN negotiation, session resumption cache (2048), 10s handshake timeout
- **Test coverage**: 22 unit tests all passing (HTTP parsing, chunked encoding, config, metrics)
- **Script module system**: Fully functional with STD library, priority system, and 13 example modules
- **Security improvements**: Fixed request smuggling, overflow bugs, thread exhaustion, mutex poisoning recovery
- **Documentation**: Complete README with architecture, module guide, CLI reference
- **Concurrency**: Solid Arc/Atomic patterns, proper shutdown signal propagation (SHUTDOWN flag)
- **Error handling**: Most panics eliminated, mutex poison recovery implemented across codebase

### ⚠️ What's Bad
- **Zero integration tests**: No end-to-end proxy behavior tests
- **No benchmarks**: Unknown performance characteristics under load
- **Missing production features**: No metrics persistence, no hot reload testing, no TLS backend connections
- **Low-priority tech debt**: Platform-specific socket options, thread join visibility, config file race conditions

### 🗺️ Short-Term Roadmap
1. ~~**HTTP/2 + HTTP/3 support**~~ ✅ DONE
2. **Integration test suite** (build server, send real HTTP, verify caching/rate-limiting)
3. **Benchmarking harness** (wrk/ab tests with metrics collection)
4. **Hot config reload** without restart

---

## 1. 🚨 CRITICAL ISSUES

### 1.1 ✅ FIXED — Load Balancer Division by Zero (load_balancer.rs + stdlib.rs)
**Fix:** Added `len == 0` guard in `RoundRobin::handle()` that returns 503 error response. stdlib.rs already had `if backends.is_empty() { return; }` guard. Registration already falls back to `Single` when backends list is empty.

---

### 1.2 ✅ FIXED — Bidirectional Streaming Thread Resource Leak (helpers.rs)
**Fix:** Added 120s read timeouts on both streams before spawning threads. Both threads now joined with panic logging. Clone failures logged instead of silently returning.

---

### 1.3 ✅ FIXED — Socket Reuse Without Validation (pool.rs)
**Fix:** Added `take_error()` SO_ERROR check before probe. Added explicit `Ok(0)` (EOF/remote close) case that skips the socket instead of reusing it.

---

### 1.4 ✅ FIXED — Slow Client HTTP Body Read DOS (http/mod.rs)
**Fix:** Added `MAX_READ_CALLS = 500` counter in `read_http_message()`. Returns error after 500 read syscalls to prevent CPU exhaustion from slow-drip clients.

---

### 1.5 ✅ FIXED — API Key Timing Attack (admin_api.rs)
**Fix:** Replaced `!=` comparison with `constant_time_eq()` using XOR accumulator. Different-length keys still do full-length work to prevent length leaking.

---

## 2. 🔧 CORE SYSTEM IMPROVEMENTS

### 2.1 ✅ FIXED — Mutex Poison Recovery With State Validation
**Severity:** HIGH  
**Status:** FIXED  
**Files:** cache.rs, stdlib.rs, pool.rs, rate_limiter.rs

**Fix:** All mutex poison recovery paths now validate state after recovery:
- **Pool**: Purges stale connections after poison (validates `created <= now`, removes aged entries)
- **Pool put()**: Clears entire pool on poison recovery (safest — don't reuse connections from corrupt state)
- **Cache**: Clears entire cache on poison recovery in `on_response()` and eviction thread
- **Rate limiter**: Clears all buckets on poison recovery (forces re-creation of token buckets)
- **Circuit breaker**: Validates `Instant` is not in future after poison; resets to `Duration::ZERO` if corrupt

---

### 2.2 ✅ FIXED — Unbounded Memory Growth Under Attack
**Severity:** HIGH  
**Status:** FIXED  
**Files:** rate_limiter.rs, cache.rs

**Fix:**
- **Rate limiter**: Hard cap `MAX_BUCKETS = 50,000`. When reached, first evicts stale entries (>300s), then force-evicts oldest 25% if still at cap. Replaces old threshold-only cleanup.
- **Cache**: `on_response()` now uses `while m.len() >= max` loop instead of single `if`, ensuring insertions never exceed max even when multiple entries need eviction.

---

### 2.3 ✅ FIXED — Script Parser Memory Limits
**Severity:** MEDIUM  
**Status:** FIXED  
**Files:** parser.rs, loader.rs

**Fix:**
- **Loader**: Added 1MB file size check (`metadata().len() > 1_048_576`) before `read_to_string()` — rejects oversized .pcmod files with warning.
- **Parser**: Added 10,000 line limit after `lines()` — returns error for scripts exceeding limit. Both limits applied in `collect_script_defaults()` and `load_script_modules()`.

---

### 2.4 ✅ FIXED — Admin API Dynamic Buffer Reading
**Severity:** MEDIUM  
**Status:** FIXED  
**File:** admin_api.rs

**Fix:** Replaced fixed `[0u8; 4096]` buffer with dynamic read loop:
- Reads in a loop until headers complete (`\r\n\r\n` found)
- Parses `Content-Length` from headers to determine body size needed
- Grows buffer to fit, capped at `MAX_ADMIN_REQUEST = 64KB`
- Returns 413 "request too large" if Content-Length or total exceeds cap
- Handles `WouldBlock`/`TimedOut` gracefully (treats as end of data)

---

### 2.5 ✅ FIXED — Config Reload Validates Invalid Fields
**Severity:** MEDIUM  
**Status:** FIXED  
**File:** config.rs

**Fix:** `load_config()` now fully rejects invalid configurations:
- Invalid `listen_addr` → falls back to `127.0.0.1:3000` with warning
- Invalid `backend_addr` → falls back to `127.0.0.1:8080` with warning
- Invalid TLS config (missing cert or key, or files not found) → clears both fields, disables TLS with warning
- All fallbacks logged explicitly so operator sees what changed

---

## 3. 🚀 MISSING FEATURES (Production Essentials)

### 3.1 Integration Test Suite
**Status:** MISSING  
**Priority:** HIGH

**Need:**
- Start real server on ephemeral port
- Send HTTP requests via `reqwest` or `hyper` client
- Verify: caching (cache hit headers), rate limiting (429 responses), compression (gzip encoding), load balancing (request distribution)
- Verify graceful shutdown doesn't drop in-flight requests

---

### 3.2 Benchmarking & Load Testing
**Status:** MISSING  
**Priority:** HIGH

**Need:**
- `wrk` or `ab` integration with metrics collection
- Measure: requests/sec, latency p50/p95/p99, memory growth over time
- Compare: with/without modules enabled, Rust vs compiled .pcmod scripts
- Detect performance regressions in CI

---

### 3.3 Hot Config Reload
**Status:** PARTIAL (code exists but untested)  
**Priority:** MEDIUM

**Need:**
- Test reload endpoint actually reloads modules
- Validate no dropped connections during reload
- Rollback mechanism if new config is invalid
- Signal modules to refresh their config (cache TTL, rate limits, etc.)

---

### 3.4 Metrics Persistence & Prometheus Integration
**Status:** PARTIAL (Prometheus endpoint exists)  
**Priority:** MEDIUM

**Need:**
- Persist metrics across restarts (write to disk periodically)
- Histogram support (latency distribution, not just max)
- Grafana dashboard templates
- Alerting rules for high error rate / circuit breaker trips

---

### 3.5 TLS Backend Connections
**Status:** MISSING  
**Priority:** MEDIUM

**Need:**
- HTTPS support for backend servers (currently only HTTP)
- SNI configuration for multiple backends
- Certificate validation (or optional skip for testing)

---

## 4. 📝 DOCUMENTATION & TOOLING

### 4.1 Performance Tuning Guide
**Status:** MISSING

**Need:**
- Recommended buffer sizes for different workloads
- Connection pool sizing guidelines
- Module priority ordering best practices
- Profiling instructions

---

### 4.2 Security Hardening Guide
**Status:** MISSING

**Need:**
- TLS cipher suite recommendations
- Rate limiting configuration for production
- API key management best practices
- Network segmentation examples (admin API on separate interface)

---

### 4.3 Module Development Tutorial
**Status:** PARTIAL (README has basics)

**Need:**
- Step-by-step .pcmod creation walkthrough
- STD library reference with all functions documented
- Common patterns (authentication, request transformation, logging)
- Debugging techniques for script modules

---

## 5. 🧹 TECHNICAL DEBT

### 5.1 ✅ FIXED — Socket Options Platform Portability
**Severity:** LOW  

**Fix:** Removed platform-specific `setsockopt` with `libc::timeval` struct. Unified to use Rust's `set_nonblocking(true)` + 50ms poll sleep on all platforms. The unix-only `#[cfg(not(windows))]` block with unsafe `setsockopt` is gone. Unix signal handling still uses libc (necessary).

---

### 5.2 ✅ FIXED — Thread Join Failure Handling
**Severity:** LOW  
**Files:** cache.rs, active_health.rs

**Fix:** Both background threads (cache eviction, active health) now have a lightweight monitor thread that calls `handle.join()`. If the worker panics, the monitor logs the panic via `crate::log::error()`. This surfaces previously-silent panics without storing JoinHandles in module state.

---

### 5.3 ✅ FIXED — Config File Race Conditions
**Severity:** LOW  
**File:** config.rs

**Fix:** Added `atomic_write()` function that writes to `{path}.tmp` then renames to `{path}`. This prevents corruption from crashes mid-write. All config writes (generation, update) now use `atomic_write()` instead of direct `fs::write()`.

---

### 5.4 ✅ FIXED — Type Conversions Overhead
**Severity:** LOW  
**Files:** request_id.rs, helpers.rs

**Fix:**
- `config_u64()` and `config_usize()` now use `u64::try_from()` / `usize::try_from()` instead of `as` casts — negative TOML values now fall back to defaults instead of wrapping silently
- `COUNTER` wrapping behavior documented explicitly in comment
- Header name `"X-Request-Id"` extracted to `const HDR_REQUEST_ID` to avoid repeated literal allocations

---

## 6. ✅ RECENTLY FIXED (Reference Only)

- **1.x Active Health Check**: Added shutdown signal, moved I/O outside write lock, validated backends
- **1.x HTTP Chunked Encoding**: Fixed operator precedence, added checked arithmetic, bounds validation
- **1.x TLS Config Validation**: Fixed OR→AND logic, added write error handling
- **1.x Server Atomics**: Changed to AcqRel ordering, removed unwrap in shutdown
- **1.x Admin API**: Added connection limit (16 max), shutdown-aware accept loop
- **1.x Load Balancer**: Added division-by-zero guard, returns 503 on empty backends
- **1.x Bidirectional Streaming**: Added 120s read timeouts, panic-safe joins
- **1.x Socket Pool**: Added SO_ERROR check, explicit EOF detection
- **1.x Slow Client DOS**: Added 500 read-call limit in HTTP parser
- **1.x API Key Security**: Constant-time comparison to prevent timing attacks
- **2.x Silent Errors**: Added logging for pool/helpers/config errors
- **2.x Metrics Overflow**: Capped latency at 600s
- **2.x Log Consistency**: error() now checks active() before logging
- **5.x Cache Max Size**: Enforces limit with LRU eviction, uses configured backend
- **5.x Socket Portability**: Removed unsafe setsockopt, unified to cross-platform non-blocking accept
- **5.x Thread Join Handling**: Cache eviction + active health threads now monitored, panics logged
- **5.x Config Atomic Write**: Write-to-tmp + rename prevents crash corruption
- **5.x Type Safety**: Safe try_from casts, documented counter wrapping, header constant extracted
- **2.x Mutex Poison Recovery**: All poison recovery paths validate state, clear corrupt data
- **2.x Unbounded Memory**: Rate limiter capped at 50K buckets with force-eviction, cache enforces max in loop
- **2.x Script Parser Limits**: 1MB file size + 10K line cap in parser and loader
- **2.x Admin API Buffer**: Dynamic read loop with Content-Length parsing, 64KB cap, 413 on overflow
- **2.x Config Reload**: Invalid addresses/TLS fall back to safe defaults with explicit warnings
- **Streamlining**: Removed duplicate ConnPool in proxy_core (uses global_pool()), eliminated unused COUNTERS HashMap in metrics, unified snapshot() for prometheus/json, fixed dead TLS validation logic, simplified config path parser

---

## 📊 ISSUE STATISTICS

| Severity | Count | Fixed | Remaining |
|----------|-------|-------|-----------|
| CRITICAL | 9 | **9** | **0** |
| HIGH | 9 | **9** | **0** |
| MEDIUM | 5 | **5** | **0** |
| LOW | 4 | **4** | **0** |
| **TOTAL** | **27** | **27** | **0** |

**Test Coverage:** 22 unit tests (HTTP, chunked, config, metrics)  
**Build Status:** ✅ 0 errors, 0 warnings  
**Documentation:** ✅ README complete

###