# Examples

Local examples for testing and understanding the two operation modes of locci-proxy.

## Prerequisites

- [Bun](https://bun.sh) with `json-server` installed globally:
  ```bash
  bun add -g json-server
  ```
- locci-proxy built:
  ```bash
  cargo build
  ```

---

## json-server

### Upstream servers — gateway mode

Three dedicated services, one resource type each. Health checks are enabled on the resource path
so the proxy removes a server from rotation if json-server goes down.

| Port | Data file          | Resource    | Health-check path |
|------|--------------------|-------------|-------------------|
| 3001 | `db-users.json`    | `/users`    | `GET /users`      |
| 3002 | `db-products.json` | `/products` | `GET /products`   |
| 3003 | `db-web.json`      | `/pages`    | `GET /pages`      |

```bash
# or: just servers-gateway
json-server --port 3001 examples/json-server/db-users.json
json-server --port 3002 examples/json-server/db-products.json
json-server --port 3003 examples/json-server/db-web.json
```

### Upstream servers — lb mode

Three **identical** instances of the same service. Each serves the same `items` list but has a
unique `instance` object so round-robin is clearly visible in responses.
Health checks run against `/instance` on each server.

| Port | Data file      | Instance tag | Health-check path |
|------|----------------|--------------|-------------------|
| 3001 | `db-lb-1.json` | `server-1`   | `GET /instance`   |
| 3002 | `db-lb-2.json` | `server-2`   | `GET /instance`   |
| 3003 | `db-lb-3.json` | `server-3`   | `GET /instance`   |

```bash
# or: just servers-lb
json-server --port 3001 examples/json-server/db-lb-1.json
json-server --port 3002 examples/json-server/db-lb-2.json
json-server --port 3003 examples/json-server/db-lb-3.json
```

---

### Mode 1 — `api_gateway`

Routes each request to a dedicated upstream based on the request path. Each upstream has an
HTTP health check configured — unhealthy servers are automatically removed from rotation.

```bash
just demo-gateway
# or manually: just servers-gateway && just run-gateway
```

| Request | Matched route | Upstream | Server | Health check |
|---|---|---|---|---|
| `GET /users` | `^/users` | `users_server` | 127.0.0.1:3001 | `GET /users` every 10s |
| `GET /products` | `^/products` | `products_server` | 127.0.0.1:3002 | `GET /products` every 10s |
| `GET /pages` | `^/` (catch-all) | `web_server` | 127.0.0.1:3003 | `GET /pages` every 10s |

```bash
curl http://localhost:8484/users      # → 3001 — users data
curl http://localhost:8484/products   # → 3002 — products data
curl http://localhost:8484/pages      # → 3003 — web data
```

---

### Mode 2 — `load_balancer`

No path awareness. Round-robins every request across all three identical instances.
Health checks run against `/instance` — stop a json-server instance and the proxy
stops routing to it within the next check interval (10 s).

```bash
just demo-lb
# or manually: just servers-lb && just run-lb
```

```bash
curl http://localhost:8484/instance   # → { name: "server-1", port: 3001 }
curl http://localhost:8484/instance   # → { name: "server-2", port: 3002 }
curl http://localhost:8484/instance   # → { name: "server-3", port: 3003 }
curl http://localhost:8484/instance   # → { name: "server-1", port: 3001 } ← repeats
```

Fire 6 requests and print the cycle inline:

```bash
just curl-lb
```

---

### Control API

Available in both modes on port `8485` (requires `Authorization: Bearer admin-key-12345`):

```bash
curl -H "Authorization: Bearer admin-key-12345" http://localhost:8485/api/v1/status
curl -H "Authorization: Bearer admin-key-12345" http://localhost:8485/api/v1/config
curl -H "Authorization: Bearer admin-key-12345" http://localhost:8485/api/v1/metrics
```

The `/metrics` endpoint returns Prometheus text format. Upstream health gauges
(`locci_upstream_health`) reflect the latest health-check results and update every
`interval_secs` seconds.
