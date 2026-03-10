# Locci Proxy — Powered by Pingora

A unified binary that operates as a load balancer or API gateway using [Pingora](https://github.com/cloudflare/pingora). Configurable via CLI flags, YAML files, `.env`, and a runtime control API.

---

## Project Structure

```
src/
├── main.rs
├── errors.rs        # ProxyError enum + ProxyResult<T>
├── config/
│   ├── mod.rs       # Configuration types & loading
│   ├── cli.rs       # CLI argument parsing (clap)
│   └── api.rs       # Runtime API configuration (placeholder)
├── services/
│   ├── mod.rs       # ServiceManager
│   ├── lb.rs        # Load balancer service
│   └── gateway.rs   # API gateway service
└── api/
    ├── mod.rs
    └── handlers.rs  # Control API (axum)

examples/
└── json-server/
    ├── db-users.json
    ├── db-products.json
    ├── db-web.json
    ├── config-gateway.yaml
    └── config-lb.yaml
```

---

## `Cargo.toml`

```toml
[package]
name = "locci-proxy"
version = "0.1.0"
edition = "2024"

[dependencies]
pingora = { version = "0.8", features = ["lb"] }
pingora-core = "0.8"
pingora-proxy = "0.8"
pingora-load-balancing = "0.8"
pingora-http = "0.8"
http = "1"

axum = "0.7"
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
async-trait = "0.1"
anyhow = "1"
thiserror = "1"
dotenvy = "0.15"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
regex = "1"
```

---

## Configuration System

### Config structure overview

```
UnifiedConfig
├── mode              — load_balancer | api_gateway
├── server            — bind address, workers, TLS
├── logging           — level (overridden by RUST_LOG)
├── upstreams         — top-level, shared by both modes
│   └── <name>
│       ├── servers   — ["host:port", ...]
│       ├── strategy  — round_robin | weighted | consistent
│       ├── tls
│       ├── sni
│       └── health_check
├── load_balancer     — only when mode: load_balancer
│   └── upstream      — name of the upstream group to balance
├── api_gateway       — only when mode: api_gateway
│   ├── routes
│   │   └── <name>
│   │       ├── path_pattern  — regex
│   │       ├── methods
│   │       ├── upstream      — name from top-level upstreams
│   │       ├── strip_prefix
│   │       └── middlewares
│   └── middlewares
│       ├── rate_limiting
│       ├── authentication
│       └── cors
└── control_api       — axum HTTP control API
```

### `src/config/mod.rs`

```rust
pub struct UnifiedConfig {
    pub mode: OperationMode,
    pub server: ServerConfig,
    pub logging: Option<LoggingConfig>,
    pub upstreams: HashMap<String, UpstreamConfig>, // shared, top-level
    pub load_balancer: Option<LoadBalancerConfig>,
    pub api_gateway: Option<ApiGatewayConfig>,
    pub control_api: ControlApiConfig,
}

pub enum OperationMode { LoadBalancer, ApiGateway }

pub struct UpstreamConfig {
    pub servers: Vec<String>,  // "host:port" — no scheme
    pub strategy: String,
    pub tls: Option<bool>,
    pub sni: Option<String>,
    pub health_check: Option<HealthCheckConfig>,
}

pub struct LoadBalancerConfig {
    pub upstream: String,  // name of the upstream group to balance
}

pub struct ApiGatewayConfig {
    pub routes: HashMap<String, RouteConfig>,
    pub middlewares: MiddlewareConfig,
}

pub struct RouteConfig {
    pub path_pattern: String,  // regex
    pub methods: Vec<String>,
    pub upstream: String,      // name from top-level upstreams
    pub strip_prefix: Option<bool>,
    pub timeout_secs: Option<u64>,
    pub middlewares: Vec<String>,
}
```

---

## Example `config.yaml` — `api_gateway` mode

```yaml
mode: api_gateway

server:
  bind_address: "0.0.0.0:8484"
  workers: 4

logging:
  level: info   # overridden by RUST_LOG env var / .env

upstreams:
  users_server:
    servers: ["127.0.0.1:3001"]
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 10
      timeout_secs: 2
      path: /users

  products_server:
    servers: ["127.0.0.1:3002"]
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 10
      timeout_secs: 2
      path: /products

  web_server:
    servers: ["127.0.0.1:3003"]
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 10
      timeout_secs: 2
      path: /pages

api_gateway:
  routes:
    users_api:
      path_pattern: "^/users"
      methods: [GET, POST, PUT, DELETE]
      upstream: users_server
      strip_prefix: false
      middlewares: []

    products_api:
      path_pattern: "^/products"
      methods: [GET, POST, PUT, DELETE]
      upstream: products_server
      strip_prefix: false
      middlewares: []

    web_app:
      path_pattern: "^/"
      methods: [GET, POST]
      upstream: web_server
      strip_prefix: false
      middlewares: []

  middlewares: {}

control_api:
  enabled: true
  bind_address: "0.0.0.0:8485"
  api_key: "admin-key-12345"
```

## Example `config.yaml` — `load_balancer` mode

```yaml
mode: load_balancer

server:
  bind_address: "0.0.0.0:8484"
  workers: 4

logging:
  level: info

upstreams:
  all_servers:
    servers:
      - "127.0.0.1:3001"
      - "127.0.0.1:3002"
      - "127.0.0.1:3003"
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 15
      timeout_secs: 3
      path: /health

load_balancer:
  upstream: all_servers

control_api:
  enabled: true
  bind_address: "0.0.0.0:8485"
  api_key: "admin-key-12345"
```

---

## Error Handling

All errors are defined in `src/errors.rs` as a `ProxyError` enum (via `thiserror`).

- Setup/init code returns `ProxyResult<T>` (`Result<T, ProxyError>`)
- Pingora proxy trait methods return `pingora_core::Result<T>`; errors are formatted using `ProxyError`'s `Display` and wrapped with `Error::explain()`

---

## Usage

### Start the proxy

```bash
# Default config
./locci-proxy

# Custom config
./locci-proxy --config production.yaml

# Override mode or bind address
./locci-proxy --mode gateway --bind 0.0.0.0:443
```

### Environment / `.env`

```bash
RUST_LOG=debug          # overrides logging.level in config
```

### Control API

```bash
curl http://localhost:8485/api/v1/status    # mode + active service
curl http://localhost:8485/api/v1/config    # full loaded config as JSON
curl http://localhost:8485/api/v1/metrics   # (stub — wire up Prometheus)
```
