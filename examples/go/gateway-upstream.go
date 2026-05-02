// gateway-upstream: minimal multi-threaded HTTP upstream for gateway-mode load-testing.
// Mirrors the three json-server gateway fixtures (users / products / web).
//
// Usage: go run examples/go/gateway-upstream.go <service> [port]
//
//   service: users | products | web
//   port:    default 3001
//
// Three instances cover the gateway setup:
//   go run examples/go/gateway-upstream.go users    3001
//   go run examples/go/gateway-upstream.go products 3002
//   go run examples/go/gateway-upstream.go web      3003

package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strconv"
	"strings"
)

// ── Data fixtures ─────────────────────────────────────────────────────────────

var users = []map[string]any{
	{"id": "1", "name": "Alice", "email": "alice@example.com"},
	{"id": "2", "name": "Bob", "email": "bob@example.com"},
	{"id": "3", "name": "Carol", "email": "carol@example.com"},
	{"id": "0607", "name": "Dave", "email": "dave@example.com"},
}

var products = []map[string]any{
	{"id": "1", "name": "Widget", "price": 9.99},
	{"id": "2", "name": "Gadget", "price": 24.99},
	{"id": "3", "name": "Doohick", "price": 4.99},
	{"id": "adb9", "name": "Thingamajig", "price": 14.99},
}

var pages = []map[string]any{
	{"id": 1, "slug": "home", "title": "Home"},
	{"id": 2, "slug": "about", "title": "About"},
}

// ── Helpers ───────────────────────────────────────────────────────────────────

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

// findByID returns the first element whose "id" field matches id, or nil.
func findByID(collection []map[string]any, id string) map[string]any {
	for _, item := range collection {
		switch v := item["id"].(type) {
		case string:
			if v == id {
				return item
			}
		case int:
			if strconv.Itoa(v) == id {
				return item
			}
		}
	}
	return nil
}

// collectionHandler returns an http.HandlerFunc for a named collection.
// GET /collection       → full array
// GET /collection/:id   → single item or 404
func collectionHandler(name string, collection []map[string]any) http.HandlerFunc {
	prefix := "/" + name + "/"
	base := "/" + name
	return func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path
		if path == base || path == base+"/" {
			writeJSON(w, http.StatusOK, collection)
			return
		}
		if strings.HasPrefix(path, prefix) {
			id := strings.TrimPrefix(path, prefix)
			if item := findByID(collection, id); item != nil {
				writeJSON(w, http.StatusOK, item)
				return
			}
		}
		http.NotFound(w, r)
	}
}

// ── Main ─────────────────────────────────────────────────────────────────────

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: gateway-upstream <users|products|web> [port]")
		os.Exit(1)
	}

	service := os.Args[1]
	port := 3001
	if len(os.Args) > 2 {
		p, err := strconv.Atoi(os.Args[2])
		if err != nil {
			fmt.Fprintf(os.Stderr, "invalid port %q\n", os.Args[2])
			os.Exit(1)
		}
		port = p
	}

	mux := http.NewServeMux()

	switch service {
	case "users":
		mux.HandleFunc("/users", collectionHandler("users", users))
		mux.HandleFunc("/users/", collectionHandler("users", users))
	case "products":
		mux.HandleFunc("/products", collectionHandler("products", products))
		mux.HandleFunc("/products/", collectionHandler("products", products))
	case "web":
		mux.HandleFunc("/pages", collectionHandler("pages", pages))
		mux.HandleFunc("/pages/", collectionHandler("pages", pages))
	default:
		fmt.Fprintf(os.Stderr, "unknown service %q — use: users | products | web\n", service)
		os.Exit(1)
	}

	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		http.NotFound(w, r)
	})

	addr := fmt.Sprintf(":%d", port)
	fmt.Printf("gateway-upstream %s → %s\n", service, addr)
	if err := http.ListenAndServe(addr, mux); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
