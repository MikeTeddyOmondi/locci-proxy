# Locci Proxy — Guide

Locci Proxy is a single binary that runs as either a **load balancer** or an **API gateway**, built on top of Cloudflare's [Pingora](https://github.com/cloudflare/pingora) framework. It is configured through a YAML file, environment variables, and optional CLI flags. A built-in HTTP control API allows runtime inspection.

---

## Table of Contents

1. [How it works](#1-how-it-works)
2. [Installation](#2-installation)
3. [Configuration](#3-configuration)
4. [Operation modes](#4-operation-modes)
5. [Upstreams](#5-upstreams)
6. [Load balancer mode](#6-load-balancer-mode)
7. [API gateway mode](#7-api-gateway-mode)
8. [Logging](#8-logging)
9. [Control API](#9-control-api)
10. [CLI reference](#10-cli-reference)
11. [Docker](#11-docker)
12. [Development workflow](#12-development-workflow)
13. [CI/CD](#13-cicd)
14. [Examples — json-server demo](#14-examples--json-server-demo)

---

## 1. How it works

Locci Proxy is built on Pingora, which owns its own async runtime. The startup sequence is:

1. Load `.env` (if present) so environment variables like `RUST_LOG` are available before anything else runs.
2. Parse the config file (path from CLI `--config`, default `config.yaml`).
3. Initialise the tracing subscriber using the resolved log level.
4. Bootstrap a Pingora `Server`.
5. Depending on `mode`, register either the load balancer service or the gateway service with the Pingora server.
6. If `control_api.enabled`, start the axum HTTP control API in a background thread on its own Tokio runtime.
7. Call `server.run_forever()` — Pingora takes over from here.

Both services resolve upstreams from the top-level `upstreams` map, which is shared across modes. The gateway additionally uses route definitions to decide which upstream to target per request.

---

## 2. Installation

### From source

Requirements: Rust 1.85+ (edition 2024), pkg-config, libssl-dev (Linux).

```bash
git clone <repo>
cd locci-proxy
cargo build --release
./target/release/locci-proxy --config config.yaml
```

### Pre-built binaries

Download from the GitHub releases page. Binaries are provided for:

| Platform | File |
|---|---|
| Linux x64 | `locci-proxy-linux-amd64` |
| Linux ARM64 | `locci-proxy-linux-arm64` |
| macOS Apple Silicon | `locci-proxy-darwin-arm64` |

```bash
chmod +x locci-proxy-linux-amd64
./locci-proxy-linux-amd64 --config config.yaml
```

### Docker

```bash
docker pull locci/proxy:latest
docker run -v $(pwd)/config.yaml:/app/config.yaml locci/proxy:latest
```

---

## 3. Configuration

All configuration lives in a single YAML file. The top-level structure is:

```yaml
mode: api_gateway          # required — load_balancer | api_gateway

server:                    # required
  bind_address: "0.0.0.0:8484"
  workers: 4               # optional — Pingora worker threads
  tls:                     # optional
    cert_path: ./certs/cert.pem
    key_path:  ./certs/key.pem

logging:                   # optional
  level: info              # trace | debug | info | warn | error

upstreams:                 # required — shared by both modes
  <name>:
    servers: [...]
    strategy: round_robin
    tls: false
    health_check: ...      # optional

load_balancer:             # required when mode: load_balancer
  upstream: <name>

api_gateway:               # required when mode: api_gateway
  routes: ...
  middlewares: ...

control_api:               # required
  enabled: true
  bind_address: "0.0.0.0:8485"
  api_key: "secret"        # optional
```

### Configuration loading priority

For any value that can come from multiple places, the priority from highest to lowest is:

```
CLI flag  >  RUST_LOG env var  >  config.yaml  >  built-in default
```

The config file path itself is resolved as: `--config` flag > default (`config.yaml`).

---

## 4. Operation modes

Locci Proxy has two mutually exclusive modes set by the top-level `mode` field.

### `load_balancer`

Distributes every incoming request across the servers in a single named upstream group using round-robin. There is no path-based routing — every request, regardless of URL, is forwarded to the next server in the rotation.

Use this when you want transparent traffic distribution in front of identical application instances.

### `api_gateway`

Matches the request path against a set of regex route patterns and forwards each request to the upstream group assigned to the first matching route. Routes are evaluated longest-pattern-first to prevent catch-all patterns from shadowing more specific ones.

Use this when different URL paths need to reach different backend services.

### Choosing between modes

| Requirement | Mode |
|---|---|
| All requests go to the same fleet of servers | `load_balancer` |
| Different paths go to different services | `api_gateway` |
| TLS termination with path routing | `api_gateway` |
| Simple TCP-level balancing, no HTTP awareness | `load_balancer` |

Both modes share the same `upstreams` definition. The `load_balancer` section and `api_gateway` section are independent — only the one matching `mode` is used at runtime.

---

## 5. Upstreams

Upstreams are defined at the top level of the config file and are referenced by name from both modes. This means you define your server pools once and reference them from whichever mode you use.

```yaml
upstreams:
  my_servers:
    servers:
      - "10.0.0.1:3000"
      - "10.0.0.2:3000"
      - "10.0.0.3:3000"
    strategy: round_robin
    tls: false
    sni: ""                  # optional — SNI hostname for TLS upstreams
    health_check:            # optional
      interval_secs: 30
      timeout_secs: 5
      path: /health          # omit for TCP-only checks
```

### Upstream addresses

Addresses are `host:port` strings — no URL scheme. Pingora operates at the TCP layer and constructs the HTTP connection itself.

```yaml
# Correct
servers:
  - "10.0.0.1:3000"
  - "api.internal:8080"

# Wrong — do not include a scheme
servers:
  - "http://10.0.0.1:3000"
```

### Strategy

The `strategy` field is recorded in config but the current implementation uses round-robin for all upstream groups. The field is reserved for future weighted and consistent-hash selection support.

### Health checks

When a `health_check` block is present, Pingora runs background health checks against each server at the configured interval.

- If `path` is set, an HTTP GET is made to that path. A `2xx` response marks the server healthy.
- If `path` is omitted, a TCP connection attempt is used.

Unhealthy servers are removed from the rotation until they recover.

```yaml
health_check:
  interval_secs: 30    # how often to check
  timeout_secs: 5      # per-check connection timeout
  path: /health        # optional HTTP path; omit for TCP-only
```

### TLS upstreams

Set `tls: true` to connect to upstream servers over TLS. Use `sni` to set the Server Name Indication hostname when it differs from the server address.

```yaml
upstreams:
  secure_backend:
    servers:
      - "backend.internal:443"
    strategy: round_robin
    tls: true
    sni: "backend.internal"
```

---

## 6. Load balancer mode

In `load_balancer` mode, the `load_balancer` section names which upstream group to distribute traffic across.

```yaml
mode: load_balancer

upstreams:
  app_fleet:
    servers:
      - "10.0.1.10:8080"
      - "10.0.1.11:8080"
      - "10.0.1.12:8080"
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 30
      timeout_secs: 5
      path: /health

load_balancer:
  upstream: app_fleet
```

Every request that arrives on `server.bind_address` is forwarded to the next healthy server in `app_fleet` in round-robin order.

There is no path matching, no middleware processing, and no strip-prefix logic. The raw HTTP request is proxied as-is.

---

## 7. API gateway mode

In `api_gateway` mode, the `api_gateway` section defines named routes and optional middlewares.

### Routes

Each route has a name (used for identification), a regex pattern matched against the request path, a list of allowed methods, and the name of the upstream group to forward to.

```yaml
api_gateway:
  routes:
    users_api:
      path_pattern: "^/api/users"   # Go-flavoured regex matched against the path
      methods: [GET, POST, PUT, DELETE]
      upstream: users_service        # must exist in top-level upstreams
      strip_prefix: false            # if true, the matched portion is removed before forwarding
      timeout_secs: 30               # optional per-route timeout
      middlewares: []                # middleware names to apply (future)
```

### Route matching order

Routes are matched in order of **pattern length, longest first**. This ensures specific patterns like `^/api/users` are evaluated before catch-alls like `^/`. If no route matches, the proxy returns a 404 to the client.

For example, given these routes:

```yaml
routes:
  users_api:
    path_pattern: "^/api/users"
    upstream: users_service

  web_app:
    path_pattern: "^/"
    upstream: web_service
```

A request to `/api/users/123` will match `users_api` (pattern length 11) before `web_app` (pattern length 2), regardless of the order they appear in the YAML file.

### Strip prefix

When `strip_prefix: true`, the portion of the path matched by `path_pattern` is removed before the request is forwarded upstream.

```yaml
routes:
  api:
    path_pattern: "^/api"
    upstream: backend
    strip_prefix: true
```

A request to `/api/users` becomes `/users` when it reaches the backend. If stripping leaves an empty path, `/` is used.

### Middlewares

Middleware names can be listed per route and are declared globally under `api_gateway.middlewares`. The middleware system is currently a configuration stub — the fields are parsed and stored but middleware logic is not yet executed at request time.

Declared middleware config fields:

```yaml
api_gateway:
  middlewares:
    rate_limiting:
      requests_per_minute: 1000
      burst_size: 100
      key_header: X-API-Key

    authentication:
      jwt_secret: "your-secret"
      excluded_paths: [/health, /metrics]

    cors:
      allowed_origins: ["*"]
      allowed_methods: [GET, POST, PUT, DELETE]
      allowed_headers: ["*"]
```

---

## 8. Logging

Log level is resolved in the following order:

1. `RUST_LOG` environment variable (set in shell or `.env` file)
2. `logging.level` in `config.yaml`
3. Built-in default: `info`

```yaml
logging:
  level: debug   # trace | debug | info | warn | error
```

The `.env` file at the project root is loaded automatically at startup before the tracing subscriber is initialised. This means `RUST_LOG` set in `.env` takes effect without any shell configuration.

```bash
# .env
RUST_LOG=debug
```

To silence Pingora's own verbose output and only show application logs:

```bash
RUST_LOG=locci_proxy=debug,pingora_core=warn
```

---

## 9. Control API

The control API is a separate HTTP server (axum) that starts on `control_api.bind_address` when `control_api.enabled: true`. It runs on its own Tokio runtime in a background thread, independent of the Pingora proxy.

### Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/status` | Running mode and which service is active |
| GET | `/api/v1/config` | Full loaded config serialised to JSON |
| GET | `/api/v1/metrics` | Metrics stub (not yet wired to Prometheus) |
| POST | `/api/v1/routes` | Add a route (not implemented) |
| DELETE | `/api/v1/routes/:name` | Remove a route (not implemented) |
| POST | `/api/v1/upstreams` | Add an upstream (not implemented) |
| DELETE | `/api/v1/upstreams/:name/servers/:server` | Remove a server (not implemented) |

### Status response

```json
{
  "status": "running",
  "mode": "ApiGateway",
  "lb_enabled": false,
  "gateway_enabled": true
}
```

### Authentication

Set `api_key` in the config to require a key. The current implementation stores the key in config but bearer token validation at the handler level is not yet enforced — this is a placeholder for production auth.

### Hot reload

The hot-reload endpoints (routes and upstreams) are stubbed and return `501 Not Implemented`. The intended approach for runtime reconfiguration is to send `SIGHUP` to the process, which triggers a config reload and service rebuild.

---

## 10. CLI reference

```
locci-proxy [OPTIONS] [COMMAND]

Options:
  -c, --config <FILE>   Path to YAML config file [default: config.yaml]
      --mode <MODE>     Override mode: lb | gateway
      --bind <ADDR>     Override server.bind_address, e.g. 0.0.0.0:443
  -h, --help
  -V, --version

Commands:
  start           Start the proxy (default behaviour)
  status          Query status via the control API
  add-route       Add a gateway route
  remove-route    Remove a gateway route
  add-upstream    Add a server to an upstream group
  remove-upstream Remove a server from an upstream group
```

CLI flags override the corresponding values in `config.yaml`. For example:

```bash
# Use a different config file
./locci-proxy --config /etc/locci/proxy.yaml

# Force gateway mode regardless of config
./locci-proxy --mode gateway

# Run on a different port without editing the config
./locci-proxy --bind 0.0.0.0:443
```

---

## 11. Docker

### Building the image

The `Dockerfile` uses a multi-stage build with `cargo-chef` for layer caching and a distroless Debian runtime for the smallest possible image.

```bash
docker build -t locci/proxy:latest .
```

### Running with Docker

The binary expects the config file at `/app/config.yaml` inside the container. Mount your config as a volume:

```bash
docker run \
  -v $(pwd)/config.yaml:/app/config.yaml:ro \
  -p 8484:8484 \
  -p 8485:8485 \
  -e RUST_LOG=info \
  locci/proxy:latest
```

### Docker Compose

The included `compose.yaml` runs locci-proxy alongside Traefik for TLS termination and automatic Let's Encrypt certificates.

Required environment variables:

| Variable | Description |
|---|---|
| `ACME_EMAIL` | Email for Let's Encrypt registration |
| `PROXY_HOST` | Public hostname for the proxy (default: `proxy.locci.cloud`) |
| `CONTROL_HOST` | Public hostname for the control API (default: `control.proxy.locci.cloud`) |
| `RUST_LOG` | Log level override (default: `info`) |

```bash
ACME_EMAIL=ops@example.com PROXY_HOST=proxy.example.com docker compose up -d
```

Traefik handles ports 80 and 443 and proxies:
- `PROXY_HOST` — to `locci-proxy:8484` (with HTTP→HTTPS redirect)
- `CONTROL_HOST` — to `locci-proxy:8485` (HTTPS only)

The Traefik dashboard is available at `:8080` — remove that port mapping in production.

---

## 12. Development workflow

### Prerequisites

- Rust 1.85+
- [just](https://github.com/casey/just) — task runner (`brew install just`)
- Bun with json-server — for running the local examples (`bun add -g json-server`)

### Common tasks

```bash
just build          # cargo build
just check          # cargo check (fast, no linking)
just fmt            # cargo fmt
just lint           # cargo clippy -- -D warnings
just test           # cargo test
just ci             # fmt + lint + test in one shot
```

### Running locally

```bash
# With the default config.yaml
just run

# With the gateway example config
just run-gateway

# With the lb example config
just run-lb
```

### Killing the proxy

```bash
just kill
```

The `kill` recipe terminates all processes matching `locci-proxy` by name and also releases ports 8484 and 8485 by PID, covering cases where a release binary or a process from a previous session is still running.

### Full recipe list

```bash
just          # shows all available recipes
```

---

## 13. CI/CD

### CI workflow (`.github/workflows/ci.yml`)

Runs on every push and pull request to `main`. Steps:

1. `cargo fmt --check` — fails if code is not formatted
2. `cargo clippy -- -D warnings` — fails on any lint warning
3. `cargo test` — runs the test suite

### Release workflow (`.github/workflows/release.yml`)

Triggered by a version tag (`v*`) or manually via `workflow_dispatch`.

**Binary build jobs** run in parallel on platform-native runners:

| Target | Runner | Method |
|---|---|---|
| `x86_64-unknown-linux-gnu` | ubuntu-latest | native |
| `aarch64-unknown-linux-gnu` | ubuntu-latest | `cross` |
| `aarch64-apple-darwin` | macos-latest | native |

**Docker build job** uses QEMU + Buildx to produce a multi-platform image (`linux/amd64`, `linux/arm64`) and pushes to Docker Hub as `locci/proxy:<version>`.

**Release job** downloads all binary artifacts, attaches them to a GitHub Release, and generates the release notes.

### Required secrets

| Secret | Purpose |
|---|---|
| `DOCKERHUB_USERNAME` | Docker Hub username |
| `DOCKERHUB_TOKEN` | Docker Hub access token (not password) |

`GITHUB_TOKEN` is provided automatically by GitHub Actions for creating releases.

### Tagging a release

```bash
git tag v1.0.0
git push origin v1.0.0
```

This triggers the release workflow. Pre-release tags (containing a hyphen, e.g. `v1.0.0-beta.1`) are automatically marked as pre-releases on GitHub.

---

## 14. Examples — json-server demo

The `examples/json-server/` directory contains data files and configs for testing both modes locally using [json-server](https://github.com/typicode/json-server).

### Gateway demo

Three separate services, each serving a distinct resource type:

```bash
just demo-gateway
```

This starts the upstream servers and the proxy, then you can test routing:

```bash
curl http://localhost:8484/users      # → port 3001 (db-users.json)
curl http://localhost:8484/products   # → port 3002 (db-products.json)
curl http://localhost:8484/pages      # → port 3003 (db-web.json)
```

Each request is routed to the correct upstream based on the path pattern defined in `config-gateway.yaml`.

### Load balancer demo

Three identical instances of the same service, each tagged with an instance identifier so round-robin is observable:

```bash
just demo-lb
```

Hit the `/instance` endpoint repeatedly and watch the server cycle:

```bash
just curl-lb
```

Output:

```
Firing 6 requests — watch the instance cycle: server-1 → server-2 → server-3 → ...
  request 1 → server-1 (:3001)
  request 2 → server-2 (:3002)
  request 3 → server-3 (:3003)
  request 4 → server-1 (:3001)
  request 5 → server-2 (:3002)
  request 6 → server-3 (:3003)
```

The data is identical across all three instances — only the `instance` object differs, making the round-robin pattern clear.

### Stopping

```bash
just stop           # kills the proxy and all json-server instances
```

### Control API during the demo

While either demo is running, the control API is available on port 8485:

```bash
just status         # { "status": "running", "mode": "...", ... }
just config         # full config as JSON
```
