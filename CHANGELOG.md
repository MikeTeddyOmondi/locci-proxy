# Changelog

All notable changes to locci-proxy are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- TASK-002: Request timeout enforcement — `upstream_connect_timeout_secs` and `upstream_read_timeout_secs` added to `ServerConfig`; per-route `timeout_secs` overrides the global read timeout in gateway mode ([#15](https://github.com/MikeTeddyOmondi/locci-proxy/pull/15))
- TASK-001: Gateway upstream load balancing — each upstream group in `api_gateway` now gets its own `LoadBalancer<RoundRobin>` with health-check background task; `upstream_peer` calls `lb.select()` instead of `servers.first()` ([#14](https://github.com/MikeTeddyOmondi/locci-proxy/pull/14))

### Fixed
- TASK-004: Pre-compile strip-prefix regex at startup — eliminates runtime `unwrap()` on the hot path ([#12](https://github.com/MikeTeddyOmondi/locci-proxy/pull/12))
- TASK-008: Health check background task now registered with Pingora server — checks were configured but never ran ([#13](https://github.com/MikeTeddyOmondi/locci-proxy/pull/13))

### Planned (see TASKS.md)
- TASK-001: Load balancing within gateway upstream groups (round-robin + failover)
- TASK-002: Per-route and global upstream timeout enforcement
- TASK-003: Control API bearer token authentication + config response redaction
- TASK-004: Pre-compile strip-prefix regex at startup (remove hot-path unwrap)
- TASK-005: Middleware engine — rate limiting, JWT auth, CORS
- TASK-006: TLS termination at the listener
- TASK-007: Prometheus metrics + Grafana dashboard
- TASK-008: Health check background task (fix: checks are configured but never run)
- TASK-009: Hot reload via SIGHUP without dropping connections
- TASK-010: Connection pool configuration
- TASK-011: Structured per-request logging with request ID propagation

---

## [0.1.0] — 2026-03-09

Initial release.

### Added

#### Core
- Single binary operating in two exclusive modes: `load_balancer` and `api_gateway`
- Powered by Cloudflare's [Pingora](https://github.com/cloudflare/pingora) 0.8 framework
- `fn main()` entry point (no `#[tokio::main]`) — Pingora owns the async runtime

#### Configuration system
- YAML configuration file with full type-safe deserialization via `serde_yaml`
- Top-level `upstreams` section shared between both modes — upstream groups are defined once and referenced by name
- `load_balancer` section names the upstream group to balance; `api_gateway` section defines routes and middlewares
- `logging.level` field with priority chain: `RUST_LOG` env var > `config.yaml` > `"info"` built-in default
- `.env` file support via `dotenvy` — loaded before tracing init so `RUST_LOG` in `.env` takes effect
- CLI overrides: `--config`, `--mode`, `--bind` flags via `clap`
- `OperationMode` enum: `load_balancer` | `api_gateway` (hybrid mode removed as redundant)

#### Load balancer mode
- Round-robin distribution across all servers in the named upstream group
- Active health checks: HTTP (configurable path) or TCP fallback
- Configurable health check interval and timeout per upstream group
- TLS upstream support with optional SNI configuration

#### API gateway mode
- Path-based routing using per-route regex patterns
- Longest-pattern-first matching — specific routes always beat catch-alls (e.g. `^/users` wins over `^/`)
- `strip_prefix` support: matched path portion removed before forwarding upstream
- Per-route `methods` allowlist, `timeout_secs`, and `middlewares` list (middleware execution stubbed)
- Middleware config schema: `rate_limiting`, `authentication` (JWT), `cors` — parsed and stored, not yet enforced

#### Error handling
- Central `ProxyError` enum in `src/errors.rs` via `thiserror`
- `ProxyResult<T>` type alias used throughout setup and init code
- Pingora proxy trait methods use `Error::explain()` with `ProxyError` display strings for structured error messages
- Clear error messages for missing config sections, invalid regex, empty upstreams, bad URIs

#### Control API
- Separate axum HTTP server on a configurable bind address (default `:8485`)
- Runs on its own Tokio runtime in a background thread, independent of the Pingora proxy
- `GET /api/v1/status` — returns running mode and active service flags
- `GET /api/v1/config` — returns full loaded config serialised to JSON
- `GET /api/v1/metrics` — stub (returns a note; not yet wired to Prometheus)
- `POST /api/v1/routes`, `DELETE /api/v1/routes/:name` — stub (`501 Not Implemented`)
- `POST /api/v1/upstreams`, `DELETE /api/v1/upstreams/:name/servers/:server` — stub

#### Observability
- Structured logging via `tracing` and `tracing-subscriber` with `EnvFilter`
- Log level configurable from config file or environment variable

#### Docker
- Multi-stage `Dockerfile` using `cargo-chef` for dependency layer caching
- Distroless Debian 12 runtime image (`gcr.io/distroless/cc-debian12`)
- Multi-platform build: `linux/amd64` and `linux/arm64` via QEMU + Buildx
- Image published to Docker Hub as `locci/proxy`
- `compose.yaml` with Traefik for TLS termination and automatic Let's Encrypt certificates

#### CI/CD
- GitHub Actions CI workflow: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` on every push and pull request to `main`
- GitHub Actions Release workflow triggered on version tags (`v*`) or `workflow_dispatch`:
  - Binary builds: `linux/amd64`, `linux/arm64` (via `cross`), `darwin/arm64`
  - Docker image push to `locci/proxy:<version>` on Docker Hub
  - GitHub Release created with binaries attached and formatted release notes

#### Developer experience
- `just` task runner with recipes for build, lint, test, run, demo, benchmark, and release
- `just ci` — fmt + clippy + test in one command
- `just demo-gateway` / `just demo-lb` — starts json-server upstreams and the proxy in the correct mode
- `just bench` — automated benchmark using `rewrk`: baseline (direct to upstream) vs proxied, release binary, all platforms
- `just release <version>` — bumps `Cargo.toml`, commits, tags, and pushes
- `.http` file with ready-to-run requests for VS Code REST Client and JetBrains HTTP Client
- `examples/json-server/` — three-server gateway demo and three-instance LB round-robin demo

#### Documentation
- `README.md` — project overview, quick start, mode comparison, links
- `GUIDE.md` — comprehensive guide covering configuration, modes, upstreams, logging, control API, Docker, CI/CD, and examples
- `REFERENCE.md` — internal code structure and type reference
- `TASKS.md` — production readiness task list with branch names and work items
- `examples/README.md` — json-server demo walkthrough for both modes
- `LICENSE` — Apache 2.0, Copyright 2025 Locci Cloud

### Known limitations (addressed in upcoming tasks)
- Gateway routes pick `servers.first()` — no load balancing or failover within a gateway upstream group (TASK-001)
- Health check loop is never started — checks are configured but do not run (TASK-008)
- Middleware config is parsed but not enforced — routes with `rate_limit` or `auth` are unprotected (TASK-005)
- Control API has no authentication enforcement — `api_key` is stored but not checked (TASK-003)
- `timeout_secs` in route config is not wired to Pingora (TASK-002)
- Strip-prefix uses a runtime `unwrap()` on regex in the request hot path (TASK-004)
- No Prometheus metrics or structured per-request logging (TASK-007, TASK-011)

---

[Unreleased]: https://github.com/MikeTeddyOmondi/locci-proxy/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/MikeTeddyOmondi/locci-proxy/releases/tag/v0.1.0
