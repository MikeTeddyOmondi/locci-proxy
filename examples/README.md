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

Three dedicated services, one resource type each:

| Port | Data file          | Resource    |
|------|--------------------|-------------|
| 3001 | `db-users.json`    | `/users`    |
| 3002 | `db-products.json` | `/products` |
| 3003 | `db-web.json`      | `/pages`    |

```bash
# or: just servers-gateway
json-server --port 3001 examples/json-server/db-users.json
json-server --port 3002 examples/json-server/db-products.json
json-server --port 3003 examples/json-server/db-web.json
```

### Upstream servers — lb mode

Three **identical** instances of the same service. Each serves the same `items` list but has a
unique `instance` object so round-robin is clearly visible in responses:

| Port | Data file      | Instance tag |
|------|----------------|--------------|
| 3001 | `db-lb-1.json` | `server-1`   |
| 3002 | `db-lb-2.json` | `server-2`   |
| 3003 | `db-lb-3.json` | `server-3`   |

```bash
# or: just servers-lb
json-server --port 3001 examples/json-server/db-lb-1.json
json-server --port 3002 examples/json-server/db-lb-2.json
json-server --port 3003 examples/json-server/db-lb-3.json
```

---

### Mode 1 — `api_gateway`

Routes each request to a dedicated upstream based on the request path.

```bash
just demo-gateway
# or manually: just servers-gateway && just run-gateway
```

| Request | Matched route | Upstream | Server |
|---|---|---|---|
| `GET /users` | `^/users` | `users_server` | 127.0.0.1:3001 |
| `GET /products` | `^/products` | `products_server` | 127.0.0.1:3002 |
| `GET /pages` | `^/` (catch-all) | `web_server` | 127.0.0.1:3003 |

```bash
curl http://localhost:8484/users      # → 3001 — users data
curl http://localhost:8484/products   # → 3002 — products data
curl http://localhost:8484/pages      # → 3003 — web data
```

---

### Mode 2 — `load_balancer`

No path awareness. Round-robins every request across all three identical instances.
Hit `/instance` to see exactly which server handled each request.

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

Available in both modes on port `8485`:

```bash
curl http://localhost:8485/api/v1/status   # running mode + which service is active
curl http://localhost:8485/api/v1/config   # full loaded config as JSON
```
