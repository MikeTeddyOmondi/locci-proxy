# Production Readiness Tasks

Each task maps to a dedicated branch. Work items are ordered by priority — hard blockers first, then missing-but-manageable gaps.

---

## Hard Blockers

---

### TASK-001 — Gateway upstream load balancing
**Branch:** `feat/gateway-upstream-lb`

**Problem:**
`GatewayProxy::upstream_peer` always calls `servers.first()`. If that server is down every request to that route fails. There is no distribution or failover within a gateway upstream group.

**Work items:**
- Replace the `HashMap<String, Vec<String>>` upstream map in `GatewayProxy` with `HashMap<String, Arc<LoadBalancer<RoundRobin>>>` (same as `LbProxy`)
- Build a `LoadBalancer` per upstream group in `GatewayProxy::new`, reusing the health check setup from `lb.rs`
- Call `lb.select(b"", 256)` in `upstream_peer` instead of `servers.first()`
- Start a background health check task for each upstream group
- Update config examples and tests

---

### TASK-002 — Request timeout enforcement
**Branch:** `feat/request-timeouts`

**Problem:**
`RouteConfig.timeout_secs` is parsed and stored but never passed to Pingora. Slow upstreams hold connections open indefinitely.

**Work items:**
- Wire `timeout_secs` from `RouteConfig` into Pingora's upstream connection options via `upstream_connect_timeout` and `upstream_read_timeout` in the `ProxyHttp` trait
- Add a global `server.upstream_timeout_secs` fallback in `ServerConfig` for modes without per-route config
- Apply the fallback timeout in `LbProxy` as well
- Document the timeout fields in `GUIDE.md`

---

### TASK-003 — Control API authentication
**Branch:** `feat/control-api-auth`

**Problem:**
`api_key` is stored in `ControlApiConfig` but never checked. Any client that can reach port 8485 can dump the full config — including the JWT secret and API key — via `GET /api/v1/config`.

**Work items:**
- Add an axum middleware layer that reads the `Authorization: Bearer <key>` header on every request
- Return `401 Unauthorized` if the key is missing or does not match `control_api.api_key`
- Skip auth check when `api_key` is `None` (opt-in, useful for local dev)
- Redact sensitive fields (`jwt_secret`, `api_key`) from the `/api/v1/config` response
- Add integration test for auth enforcement

---

### TASK-004 — Remove hot-path unwrap in strip-prefix
**Branch:** `fix/strip-prefix-unwrap`

**Problem:**
`upstream_request_filter` in `gateway.rs` calls `regex::Regex::new(&route.path_pattern).unwrap()` on every request where `strip_prefix: true`. A malformed pattern stored in memory panics the Pingora worker thread.

**Work items:**
- Pre-compile and cache a second regex in `GatewayProxy` specifically for strip-prefix replacement (same pattern, compiled once at startup alongside the match regex)
- Store it as `Vec<(Regex, Option<Regex>, RouteConfig)>` — `None` when `strip_prefix` is false
- Remove the runtime `unwrap()` entirely
- Add a test with a route that uses `strip_prefix: true`

---

### TASK-005 — Middleware implementation
**Branch:** `feat/middleware-engine`

**Problem:**
`rate_limiting`, `authentication`, and `cors` are parsed from config and referenced by route but never executed. Routes marked `middlewares: [rate_limit, auth]` are completely unprotected.

**Work items:**

#### Rate limiting
- Implement a token-bucket rate limiter using `dashmap` for per-key counters
- Extract the key from the header named in `RateLimitConfig.key_header` (fall back to client IP)
- Return `429 Too Many Requests` when the bucket is exhausted
- Respect `requests_per_minute` and `burst_size`

#### JWT authentication
- Validate `Authorization: Bearer <token>` using the `jsonwebtoken` crate
- Skip validation for paths listed in `AuthConfig.excluded_paths`
- Return `401 Unauthorized` on missing or invalid token

#### CORS
- Inject `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers` response headers from `CorsConfig`
- Handle `OPTIONS` preflight requests with `204 No Content`

#### Engine
- Implement a middleware chain that runs enabled middlewares in order before `upstream_peer`
- Hook into `request_filter` in `ProxyHttp`

---

### TASK-006 — TLS termination
**Branch:** `feat/tls-termination`

**Problem:**
`server.tls` (`cert_path`, `key_path`) is parsed from config but never passed to Pingora's listener. The proxy only listens on plaintext HTTP.

**Work items:**
- Pass the TLS config to `svc.add_tls(addr, cert_path, key_path)` instead of `svc.add_tcp(addr)` when `server.tls` is `Some`
- Validate that the cert and key files exist at startup and surface a clear `ProxyError`
- Update `compose.yaml` notes — in production, Traefik handles TLS termination so this is for deployments without Traefik
- Add a self-signed cert generation recipe to the justfile for local TLS testing

---

## Missing but Manageable

---

### TASK-007 — Prometheus metrics
**Branch:** `feat/prometheus-metrics`

**Problem:**
`GET /api/v1/metrics` returns a hardcoded stub. There is no instrumentation anywhere in the request path.

**Work items:**

#### Instrumentation
- Add `prometheus` and `lazy_static` (or `once_cell`) as dependencies
- Define a global `MetricsRegistry` in a new `src/metrics.rs` module with the following counters and histograms:
  - `locci_requests_total{mode, route, upstream, status}` — counter
  - `locci_request_duration_seconds{mode, route, upstream}` — histogram (buckets: 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 5s)
  - `locci_upstream_health{upstream, server}` — gauge (1 = healthy, 0 = unhealthy)
  - `locci_active_connections{upstream}` — gauge
  - `locci_errors_total{mode, error_type}` — counter

#### Request instrumentation
- Record start time at `request_filter` entry
- Increment `locci_requests_total` and observe `locci_request_duration_seconds` in `logging_filter` (called after the response is sent)
- Increment `locci_errors_total` on `fail_to_proxy`

#### Health check instrumentation
- Update `locci_upstream_health` gauge whenever a health check result changes

#### Metrics endpoint
- Replace the stub in `handlers.rs` with Prometheus text-format output via `prometheus::TextEncoder`
- Expose on `GET /api/v1/metrics` (existing route) and optionally on a dedicated `GET /metrics` path

#### Grafana dashboard
- Add `monitoring/` directory with a `docker-compose.monitoring.yaml` that spins up Prometheus (scraping `:8485/api/v1/metrics`) and Grafana
- Add a pre-built Grafana dashboard JSON for the metrics above

---

### TASK-008 — Health check background task
**Branch:** `fix/health-check-task`

**Problem:**
`lb.health_check_frequency` is set on the `LoadBalancer` struct but `LoadBalancer::update()` is never called in a background loop. Health checks do not run.

**Work items:**
- Spawn a `pingora_core::services::background::background_service` that calls `lb.update().await` on each upstream group at the configured interval
- Register the background service with the Pingora server via `server.add_service`
- Verify that unhealthy servers are removed from the rotation in a test with a mock upstream that returns errors
- Connect health status changes to `TASK-007` metrics (`locci_upstream_health` gauge)

---

### TASK-009 — Hot reload via SIGHUP
**Branch:** `feat/hot-reload`

**Problem:**
Config changes require a process restart. The control API hot-reload endpoints return `501 Not Implemented`.

**Work items:**
- Install a `SIGHUP` handler using `tokio::signal::unix`
- On signal: reload the config file from disk, validate it, and atomically swap the active config via `Arc<ArcSwap<UnifiedConfig>>`
- Rebuild affected services (gateway routes, upstream pools) without dropping existing connections — Pingora supports graceful handoff via `Server::run_once`
- Implement `POST /api/v1/routes` and `DELETE /api/v1/routes/:name` as in-memory mutations that update the swapped config
- Log a clear message on successful reload and on validation failure (keep old config on error)

---

### TASK-010 — Connection pool configuration
**Branch:** `feat/connection-pool-config`

**Problem:**
Pingora's connection pool settings (pool size, idle timeout, connection timeout) are not exposed in the config.

**Work items:**
- Add `ConnectionPoolConfig` to `ServerConfig`:
  ```yaml
  server:
    connection_pool:
      max_idle: 128
      idle_timeout_secs: 60
      connect_timeout_secs: 10
  ```
- Wire these values into Pingora's `HttpPeer` options in both `LbProxy` and `GatewayProxy`
- Document in `GUIDE.md`

---

### TASK-011 — Observability: structured request logging
**Branch:** `feat/structured-logging`

**Problem:**
There is no per-request logging. Debugging a live proxy means guessing which upstream received a request and why.

**Work items:**
- Implement `logging_filter` in both `LbProxy` and `GatewayProxy` to emit a structured log line per request:
  ```
  INFO request path=/users method=GET upstream=users_server server=127.0.0.1:3001 status=200 duration_ms=3
  ```
- Include a request ID (generate a UUID in `new_ctx` and propagate via the `CTX` type)
- Forward the request ID downstream as `X-Request-Id` header
- Log upstream errors with `WARN` level including the error type from `ProxyError`

---

## Tracking

| Task | Branch | Status |
|---|---|---|
| TASK-001 | `feat/gateway-upstream-lb` | Open |
| TASK-002 | `feat/request-timeouts` | Open |
| TASK-003 | `feat/control-api-auth` | Open |
| TASK-004 | `fix/strip-prefix-unwrap` | Open |
| TASK-005 | `feat/middleware-engine` | Open |
| TASK-006 | `feat/tls-termination` | Open |
| TASK-007 | `feat/prometheus-metrics` | Open |
| TASK-008 | `fix/health-check-task` | Open |
| TASK-009 | `feat/hot-reload` | Open |
| TASK-010 | `feat/connection-pool-config` | Open |
| TASK-011 | `feat/structured-logging` | Open |
