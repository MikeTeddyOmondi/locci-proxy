// lb-upstream: minimal multi-threaded HTTP upstream for load-testing locci-proxy in lb mode.
// Serves /instance (singleton) and /items (array) — mirrors the json-server lb fixtures.
//
// Usage: go run examples/go/lb-upstream.go [port]   (default: 3001)
//
// Three instances cover the lb setup:
//   go run examples/go/lb-upstream.go 3001
//   go run examples/go/lb-upstream.go 3002
//   go run examples/go/lb-upstream.go 3003

package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strconv"
)

var items = []map[string]any{
	{"id": 1, "name": "Apple", "price": 1.99},
	{"id": 2, "name": "Banana", "price": 0.99},
	{"id": 3, "name": "Cherry", "price": 3.49},
}

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(v)
}

func main() {
	port := 3001
	if len(os.Args) > 1 {
		p, err := strconv.Atoi(os.Args[1])
		if err != nil {
			fmt.Fprintf(os.Stderr, "invalid port %q\n", os.Args[1])
			os.Exit(1)
		}
		port = p
	}

	id := port - 3000
	instance := map[string]any{
		"id":   id,
		"name": fmt.Sprintf("server-%d", id),
		"port": port,
	}

	mux := http.NewServeMux()

	mux.HandleFunc("/instance", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, instance)
	})

	mux.HandleFunc("/items", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, items)
	})

	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		http.NotFound(w, r)
	})

	addr := fmt.Sprintf(":%d", port)
	fmt.Printf("lb-upstream server-%d → %s\n", id, addr)
	if err := http.ListenAndServe(addr, mux); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
