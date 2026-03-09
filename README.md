# locci-proxy

A high-performance reverse proxy built on [Pingora](https://github.com/cloudflare/pingora) that runs as either a **load balancer** or an **API gateway** from a single binary. Configured via YAML, with environment variable overrides and a built-in HTTP control API.

![CI](https://github.com/MikeTeddyOmondi/locci-proxy/actions/workflows/ci.yml/badge.svg)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/docker/v/locci/proxy?label=docker)](https://hub.docker.com/r/locci/proxy)

---

## Features

- Two operation modes from one binary — `load_balancer` and `api_gateway`
- Round-robin load balancing with active health checks (HTTP or TCP)
- Path-based routing using regex patterns, longest-match-first
- TLS upstream support with configurable SNI
- Structured logging via `tracing`, level controllable from config or `RUST_LOG`
- HTTP control API for runtime status and config inspection
- `.env` file support via `dotenvy`
- Multi-platform Docker image (`linux/amd64`, `linux/arm64`)

---

## Quick start

```bash
# Build
cargo build --release

# Run with the example config
cp config.yaml my-config.yaml   # edit upstreams and mode
./target/release/locci-proxy --config my-config.yaml
```

Or with Docker:

```bash
docker run \
  -v $(pwd)/config.yaml:/app/config.yaml:ro \
  -p 8484:8484 -p 8485:8485 \
  locci/proxy:latest
```

---

## Configuration at a glance

```yaml
mode: api_gateway   # load_balancer | api_gateway

server:
  bind_address: "0.0.0.0:8484"

logging:
  level: info       # overridden by RUST_LOG

# Upstream groups — shared by both modes
upstreams:
  users_service:
    servers: ["10.0.0.1:3000", "10.0.0.2:3000"]
    strategy: round_robin
    tls: false
    health_check:
      interval_secs: 30
      timeout_secs: 5
      path: /health

# --- api_gateway mode ---
api_gateway:
  routes:
    users_api:
      path_pattern: "^/users"
      methods: [GET, POST, PUT, DELETE]
      upstream: users_service
      strip_prefix: false
      middlewares: []
  middlewares: {}

# --- load_balancer mode ---
# load_balancer:
#   upstream: users_service

control_api:
  enabled: true
  bind_address: "0.0.0.0:8485"
  api_key: "your-api-key"
```

See [GUIDE.md](GUIDE.md) for the full configuration reference.

---

## Operation modes

### `load_balancer`

Every request is forwarded to the next healthy server in the upstream group using round-robin. No path matching — pure traffic distribution.

```
client → locci-proxy:8484 → server-1:3000
                           → server-2:3000  (round-robin)
                           → server-3:3000
```

### `api_gateway`

Request path is matched against regex route patterns. Each route forwards to a named upstream group. Routes are evaluated longest-pattern-first so catch-alls never shadow specific routes.

```
/users/*    → users_service  (10.0.0.1:3000)
/products/* → products_service (10.0.0.2:3000)
/           → web_service    (10.0.0.3:8080)
```

---

## Control API

```bash
curl http://localhost:8485/api/v1/status   # running mode
curl http://localhost:8485/api/v1/config   # full config as JSON
curl http://localhost:8485/api/v1/metrics  # metrics (stub)
```

---

## Development

Requires [just](https://github.com/casey/just):

```bash
just build       # compile debug binary
just ci          # fmt + clippy + test
just demo-gateway  # start json-server upstreams + run gateway mode
just demo-lb       # start identical instances + run lb mode
just curl-lb       # fire 6 requests and watch round-robin
just stop          # kill proxy + all upstream servers
just              # list all recipes
```

See [examples/README.md](examples/README.md) for the full local demo walkthrough.

---

## Docker Compose

```bash
PROXY_HOST=proxy.locci.cloud \
docker compose up -d
```

Starts locci-proxy with Traefik handling TLS termination and automatic Let's Encrypt certificates.

---

## Releases

Pre-built binaries and Docker images are published automatically on version tags.

| Platform | Binary |
|---|---|
| Linux x64 | `locci-proxy-linux-amd64` |
| Linux ARM64 | `locci-proxy-linux-arm64` |
| macOS Apple Silicon | `locci-proxy-darwin-arm64` |

```bash
docker pull locci/proxy:1.0.0
```

---

## Documentation

- [GUIDE.md](GUIDE.md) — comprehensive guide covering all configuration options, modes, Docker, CI/CD, and the local demo
- [REFERENCE.md](REFERENCE.md) — internal code structure and type reference
- [examples/README.md](examples/README.md) — json-server demo walkthrough

---

## License

Apache 2.0 — see [LICENSE](LICENSE).
