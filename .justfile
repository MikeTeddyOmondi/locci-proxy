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
    ./target/debug/locci-proxy --config examples/config-gateway.yaml

# Run with the lb example config (load_balancer mode)
run-lb: build
    ./target/debug/locci-proxy --config examples/config-lb.yaml

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
bench-baseline: build-release servers-lb
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 1.5
    echo "--- Baseline: direct to upstream :3001 (no proxy) ---"
    rewrk -h http://127.0.0.1:3001/instance -t "$THREADS" -c 100 -d 10s --pct
    pkill -f "json-server" 2>/dev/null || true

# Benchmark through the proxy in lb mode using the release binary
bench-lb: build-release servers-lb
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 1.5
    ./target/release/locci-proxy --config examples/config-lb.yaml > /tmp/proxy-bench-lb.log 2>&1 &
    PROXY_PID=$!
    sleep 1.5
    echo "--- LB mode (release): through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/instance -t "$THREADS" -c 100 -d 300s --pct
    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "json-server" 2>/dev/null || true

# Benchmark through the proxy in gateway mode using the release binary
bench-gateway: build-release servers-gateway
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 1.5
    ./target/release/locci-proxy --config examples/config-gateway.yaml > /tmp/proxy-bench-gw.log 2>&1 &
    PROXY_PID=$!
    sleep 1.5
    echo "--- Gateway mode (release): through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/users -t "$THREADS" -c 100 -d 10s --pct
    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "json-server" 2>/dev/null || true

# Full automated benchmark: builds release binary, starts upstreams, runs baseline
# then proxied, prints both results side-by-side, then cleans up.
bench: build-release servers-lb
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "Threads: $THREADS   Connections: 100   Duration: 10s"
    echo ""

    # Start the release proxy in the background
    ./target/release/locci-proxy --config examples/config-lb.yaml > /tmp/proxy-bench.log 2>&1 &
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

# ── Go bench upstreams ────────────────────────────────────────────────────────
# Fast multi-threaded replacements for json-server. No dependencies beyond Go stdlib.
# Serve /instance (singleton) and /items (array) — same JSON shape as the lb fixtures.

# Start three Go bench-upstream instances (mirrors servers-lb but much faster)
servers-lb-go:
    #!/usr/bin/env bash
    go run examples/go/lb-upstream.go 3001 > /tmp/go-lb-1.log 2>&1 & echo "server-1 → :3001  (PID $!)"
    go run examples/go/lb-upstream.go 3002 > /tmp/go-lb-2.log 2>&1 & echo "server-2 → :3002  (PID $!)"
    go run examples/go/lb-upstream.go 3003 > /tmp/go-lb-3.log 2>&1 & echo "server-3 → :3003  (PID $!)"

# Stop all Go bench-upstream instances
servers-lb-go-stop:
    #!/usr/bin/env bash
    pkill -f "lb-upstream" 2>/dev/null && echo "go bench-upstreams stopped" || echo "none running"

# Baseline: direct to Go upstream :3001 (no proxy) — establishes the ceiling
bench-baseline-go: servers-lb-go
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 3
    echo "--- Baseline (Go upstream): direct to :3001 (no proxy) ---"
    rewrk -h http://127.0.0.1:3001/instance -t "$THREADS" -c 100 -d 10s --pct
    pkill -f "lb-upstream" 2>/dev/null || true

# Benchmark through the proxy in lb mode against Go upstreams using the release binary
bench-lb-go: build-release servers-lb-go
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 1.5
    ./target/release/locci-proxy --config examples/config-lb.yaml > /tmp/proxy-bench-lb-go.log 2>&1 &
    PROXY_PID=$!
    sleep 3
    echo "--- LB mode (release, Go upstreams): through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/instance -t "$THREADS" -c 100 -d 300s --pct
    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "lb-upstream" 2>/dev/null || true

# Full automated Go-upstream benchmark: baseline then proxied, side-by-side.
bench-go: build-release servers-lb-go
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "Threads: $THREADS   Connections: 100   Duration: 10s   Upstreams: Go"
    echo ""
    sleep 3   # let Go servers finish compiling / binding

    ./target/release/locci-proxy --config examples/config-lb.yaml > /tmp/proxy-bench-go.log 2>&1 &
    PROXY_PID=$!
    sleep 1.5

    echo "=== 1/2  Baseline — direct to Go upstream :3001 (no proxy) ==="
    rewrk -h http://127.0.0.1:3001/instance -t "$THREADS" -c 100 -d 10s --pct

    echo ""
    echo "=== 2/2  Proxied — through locci-proxy :8484 (lb mode, Go upstreams) ==="
    rewrk -h http://127.0.0.1:8484/instance -t "$THREADS" -c 100 -d 10s --pct

    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "lb-upstream" 2>/dev/null || true
    echo ""
    echo "Done. Proxy log: /tmp/proxy-bench-go.log"

# Start three Go gateway upstreams (users/:3001, products/:3002, web/:3003)
servers-gateway-go:
    #!/usr/bin/env bash
    go run examples/go/gateway-upstream.go users    3001 > /tmp/go-gw-users.log    2>&1 & echo "users    → :3001  (PID $!)"
    go run examples/go/gateway-upstream.go products 3002 > /tmp/go-gw-products.log 2>&1 & echo "products → :3002  (PID $!)"
    go run examples/go/gateway-upstream.go web      3003 > /tmp/go-gw-web.log      2>&1 & echo "web      → :3003  (PID $!)"

# Stop all Go gateway upstream instances
servers-gateway-go-stop:
    #!/usr/bin/env bash
    pkill -f "gateway-upstream" 2>/dev/null && echo "go gateway-upstreams stopped" || echo "none running"

# Baseline: direct to Go users upstream :3001 (no proxy)
bench-baseline-gateway-go: servers-gateway-go
    #!/usr/bin/env bash
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 3
    echo "--- Baseline (Go gateway upstream): direct to :3001/users (no proxy) ---"
    rewrk -h http://127.0.0.1:3001/users -t "$THREADS" -c 100 -d 10s --pct
    pkill -f "gateway-upstream" 2>/dev/null || true

# Benchmark through the proxy in gateway mode against Go upstreams using the release binary
bench-gateway-go: build-release servers-gateway-go
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    sleep 1.5
    ./target/release/locci-proxy --config examples/config-gateway.yaml > /tmp/proxy-bench-gw-go.log 2>&1 &
    PROXY_PID=$!
    sleep 3
    echo "--- Gateway mode (release, Go upstreams): through proxy :8484 ---"
    rewrk -h http://127.0.0.1:8484/users -t "$THREADS" -c 100 -d 300s --pct
    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "gateway-upstream" 2>/dev/null || true

# Full automated Go-upstream gateway benchmark: baseline then proxied, side-by-side.
bench-gateway-go-full: build-release servers-gateway-go
    #!/usr/bin/env bash
    set -e
    THREADS=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)
    echo "Threads: $THREADS   Connections: 100   Duration: 10s   Upstreams: Go (gateway)"
    echo ""
    sleep 3   # let Go servers finish compiling / binding

    ./target/release/locci-proxy --config examples/config-gateway.yaml > /tmp/proxy-bench-gw-go.log 2>&1 &
    PROXY_PID=$!
    sleep 1.5

    echo "=== 1/2  Baseline — direct to Go users upstream :3001 (no proxy) ==="
    rewrk -h http://127.0.0.1:3001/users -t "$THREADS" -c 100 -d 10s --pct

    echo ""
    echo "=== 2/2  Proxied — through locci-proxy :8484 (gateway mode, Go upstreams) ==="
    rewrk -h http://127.0.0.1:8484/users -t "$THREADS" -c 100 -d 10s --pct

    kill "$PROXY_PID" 2>/dev/null || true
    pkill -f "gateway-upstream" 2>/dev/null || true
    echo ""
    echo "Done. Proxy log: /tmp/proxy-bench-gw-go.log"

# ── Docker ────────────────────────────────────────────────────────────────────

# Build the Docker image (uses Dockerfile multi-stage / cargo-chef cache)
docker-build:
    docker build -t locci/proxy:latest .

# Build and tag with a specific version
# Usage: just docker-tag 0.2.0
docker-tag version:
    docker build -t locci/proxy:{{version}} -t locci/proxy:latest .

# Run the container with config.yaml (gateway mode by default)
docker-run:
    docker run --rm \
        -p 8484:8484 \
        -p 8485:8485 \
        -v "$(pwd)/config.yaml:/app/config.yaml:ro" \
        -e RUST_LOG=${RUST_LOG:-info} \
        --name locci-proxy \
        locci/proxy:latest --config /app/config.yaml

# Run the container with the lb example config
docker-run-lb:
    docker run --rm \
        -p 8484:8484 \
        -p 8485:8485 \
        -v "$(pwd)/examples/config-lb.yaml:/app/config.yaml:ro" \
        -e RUST_LOG=${RUST_LOG:-info} \
        --name locci-proxy \
        locci/proxy:latest --config /app/config.yaml

# Run the container with the gateway example config
docker-run-gateway:
    docker run --rm \
        -p 8484:8484 \
        -p 8485:8485 \
        -v "$(pwd)/examples/config-gateway.yaml:/app/config.yaml:ro" \
        -e RUST_LOG=${RUST_LOG:-info} \
        --name locci-proxy \
        locci/proxy:latest --config /app/config.yaml

# Stop and remove the running container
docker-stop:
    docker stop locci-proxy 2>/dev/null || true
    docker rm   locci-proxy 2>/dev/null || true

# Tail logs from the running container
docker-logs:
    docker logs -f locci-proxy

# Remove the built image
docker-clean:
    docker rmi locci/proxy:latest 2>/dev/null || true

# ── Docker Compose ────────────────────────────────────────────────────────────

# Build and start the proxy via compose (uses config.yaml volume mount)
compose-up:
    docker compose up -d --build
    @echo "Proxy:       http://localhost:8484"
    @echo "Control API: http://localhost:8485"

# Start without rebuilding
compose-start:
    docker compose up -d

# Stop and remove containers (keeps volumes/images)
compose-stop:
    docker compose down

# Stop and remove containers + image
compose-down:
    docker compose down --rmi local

# Tail proxy logs
compose-logs:
    docker compose logs -f locci-proxy

# Rebuild image without cache
compose-rebuild:
    docker compose build --no-cache
    docker compose up -d

# Show running compose services
compose-ps:
    docker compose ps

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
