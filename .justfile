# locci-proxy justfile  (just 1.x — https://github.com/casey/just)

# Show available recipes
default:
    @just --list

# ── Build ─────────────────────────────────────────────────────────────────────

# Compile debug binary
build:
    cargo build

# Fast type-check without linking
check:
    cargo check

# Format code with rustfmt
fmt:
    cargo fmt

# Lint with Clippy (warnings treated as errors)
lint:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test

# fmt + lint + test in one go
ci: fmt lint test

# Bump version, commit, tag, and push a release
# Usage: just release 0.2.0
release version:
    @echo "Releasing v{{version}}"
    sed -i '' 's/^version = ".*"/version = "{{version}}"/' Cargo.toml
    cargo build --release
    git add Cargo.toml Cargo.lock
    git commit -m "chore: release v{{version}}"
    git tag -a "v{{version}}" -m "Release v{{version}}"
    git push origin main
    git push origin "v{{version}}"

# ── Run proxy ─────────────────────────────────────────────────────────────────

# Run with the default config.yaml
run: build
    ./target/debug/locci-proxy --config config.yaml

# Run with the gateway example config (api_gateway mode)
run-gateway: build
    ./target/debug/locci-proxy --config examples/json-server/config-gateway.yaml

# Run with the lb example config (load_balancer mode)
run-lb: build
    ./target/debug/locci-proxy --config examples/json-server/config-lb.yaml

# Kill any running locci-proxy instance (by name and by port)
kill:
    #!/usr/bin/env bash
    pkill -f "locci-proxy" 2>/dev/null || true
    # Belt-and-suspenders: release any process still holding the proxy/control ports
    for port in 8484 8485; do
        pid=$(lsof -ti :$port 2>/dev/null) && [ -n "$pid" ] && kill $pid 2>/dev/null && echo "killed PID $pid on :$port" || true
    done
    echo "done"

# ── json-server upstreams ─────────────────────────────────────────────────────

# Start three dedicated upstream servers for gateway mode (users / products / web)
servers-gateway:
    #!/usr/bin/env bash
    json-server --port 3001 examples/json-server/db-users.json    > /tmp/json-users.log    2>&1 & echo "users    → :3001  (PID $!)"
    json-server --port 3002 examples/json-server/db-products.json > /tmp/json-products.log 2>&1 & echo "products → :3002  (PID $!)"
    json-server --port 3003 examples/json-server/db-web.json      > /tmp/json-web.log      2>&1 & echo "web      → :3003  (PID $!)"

# Start three identical server instances for lb mode (same data, different port/instance tag)
servers-lb:
    #!/usr/bin/env bash
    json-server --port 3001 examples/json-server/db-lb-1.json > /tmp/json-lb-1.log 2>&1 & echo "server-1 → :3001  (PID $!)"
    json-server --port 3002 examples/json-server/db-lb-2.json > /tmp/json-lb-2.log 2>&1 & echo "server-2 → :3002  (PID $!)"
    json-server --port 3003 examples/json-server/db-lb-3.json > /tmp/json-lb-3.log 2>&1 & echo "server-3 → :3003  (PID $!)"

# Stop all json-server instances
servers-stop:
    #!/usr/bin/env bash
    pkill -f "json-server" 2>/dev/null && echo "json-servers stopped" || echo "no json-servers running"

# ── Demo combos ───────────────────────────────────────────────────────────────

# Start gateway upstreams + launch gateway mode (Ctrl-C stops the proxy; servers keep running)
demo-gateway: servers-gateway
    @sleep 1
    just run-gateway

# Start three identical lb instances + launch lb mode (Ctrl-C stops proxy; servers keep running)
demo-lb: servers-lb
    @sleep 1
    just run-lb

# Stop proxy + all json-servers
stop: kill servers-stop

# ── Control API ───────────────────────────────────────────────────────────────

# Proxy status (mode, which service is active)
status:
    curl -s http://localhost:8485/api/v1/status | python3 -m json.tool

# Full loaded config as JSON
config:
    curl -s http://localhost:8485/api/v1/config | python3 -m json.tool

# Metrics (stub)
metrics:
    curl -s http://localhost:8485/api/v1/metrics | python3 -m json.tool

# ── Test requests (proxy + upstreams must be running) ─────────────────────────

# GET /users through the proxy
curl-users:
    curl -s http://localhost:8484/users | python3 -m json.tool

# GET /products through the proxy
curl-products:
    curl -s http://localhost:8484/products | python3 -m json.tool

# GET /pages through the proxy
curl-pages:
    curl -s http://localhost:8484/pages | python3 -m json.tool

# Hit all three gateway endpoints in sequence
curl-all: curl-users curl-products curl-pages

# Hit /instance six times to show round-robin across the three lb servers
curl-lb:
    #!/usr/bin/env bash
    echo "Firing 6 requests — watch the instance cycle: server-1 → server-2 → server-3 → ..."
    for i in 1 2 3 4 5 6; do
        echo -n "  request $i → "
        curl -s http://localhost:8484/instance | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['name'], '(:'+str(d['port'])+')')"
    done

# ── Benchmarks ────────────────────────────────────────────────────────────────
# Uses rewrk (cargo install rewrk). Always benchmarks the release binary.
# Note: json-server is single-threaded Node.js — results reflect its ceiling,
# not locci-proxy's. Use a faster upstream for a realistic proxy-overhead measurement.

# Build an optimised release binary for benchmarking
build-release:
    cargo build --release

# Benchmark an upstream directly, bypassing the proxy (requires just servers-lb)
# This establishes the ceiling that locci-proxy cannot exceed.
bench-baseline:
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "--- Baseline: direct to upstream :3001 (no proxy) ---"
    rewrk -h http://127.0.0.1:3001/instance -t "$THREADS" -c 100 -d 10s --pct

# Benchmark through the proxy in lb mode (requires just servers-lb + proxy running)
bench-lb:
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "--- LB mode: through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/instance -t "$THREADS" -c 100 -d 10s --pct

# Benchmark through the proxy in gateway mode (requires just servers-gateway + proxy running)
bench-gateway:
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "--- Gateway mode: through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/users -t "$THREADS" -c 100 -d 10s --pct

# Full automated benchmark: builds release binary, starts upstreams, runs baseline
# then proxied, prints both results side-by-side, then cleans up.
bench: build-release servers-lb
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "Threads: $THREADS   Connections: 100   Duration: 10s"
    echo ""

    # Start the release proxy in the background
    ./target/release/locci-proxy --config examples/json-server/config-lb.yaml > /tmp/proxy-bench.log 2>&1 &
    PROXY_PID=$!
    sleep 1.5   # wait for bind

    echo "=== 1/2  Baseline — direct to :3001 (upstream, no proxy) ==="
    rewrk -h http://127.0.0.1:3001/instance -t "$THREADS" -c 100 -d 10s --pct

    echo ""
    echo "=== 2/2  Proxied — through locci-proxy :8484 (lb mode) ==="
    rewrk -h http://127.0.0.1:8484/instance -t "$THREADS" -c 100 -d 10s --pct

    # Cleanup
    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "json-server" 2>/dev/null || true
    echo ""
    echo "Done. Proxy log: /tmp/proxy-bench.log"

# ── Monitoring ────────────────────────────────────────────────────────────────

# Start Prometheus + Grafana (http://localhost:3000, default login admin/admin)
monitor:
    docker compose -f monitoring/docker-compose.monitoring.yaml up -d
    @echo "Prometheus: http://localhost:9090"
    @echo "Grafana:    http://localhost:3000  (admin / admin)"

# Stop monitoring stack
monitor-stop:
    docker compose -f monitoring/docker-compose.monitoring.yaml down

# ── Logs ──────────────────────────────────────────────────────────────────────

# Tail gateway upstream logs
logs-gateway:
    tail -f /tmp/json-users.log /tmp/json-products.log /tmp/json-web.log

# Tail lb upstream logs
logs-lb:
    tail -f /tmp/json-lb-1.log /tmp/json-lb-2.log /tmp/json-lb-3.log
