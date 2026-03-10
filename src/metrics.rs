//! Global Prometheus metrics for locci-proxy.
//!
//! All metrics are registered with the default Prometheus registry via the
//! `register_*!` macros. Call [`render_metrics`] to produce the Prometheus
//! text exposition format for scraping.
//!
//! Instrumentation points:
//! - [`record_request`]       — call in `logging()` after every proxied response
//! - [`record_error`]         — call in `logging()` on proxy failure
//! - [`set_upstream_health`]  — call when a health-check result changes

use once_cell::sync::Lazy;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, TextEncoder,
    register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
};

// ── Metrics ───────────────────────────────────────────────────────────────────

/// Total proxied requests.
/// Labels: `mode` (lb | gateway), `upstream`, `status` (HTTP status code).
pub static REQUESTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        Opts::new("locci_requests_total", "Total number of proxied requests"),
        &["mode", "upstream", "status"]
    )
    .expect("failed to register locci_requests_total")
});

/// Request duration histogram.
/// Labels: `mode`, `upstream`.
/// Buckets: 1 ms → 5 s.
pub static REQUEST_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "locci_request_duration_seconds",
            "Request duration from proxy receipt to upstream response",
        )
        .buckets(vec![
            0.001, 0.005, 0.010, 0.025, 0.050, 0.100, 0.250, 0.500, 1.0, 5.0,
        ]),
        &["mode", "upstream"]
    )
    .expect("failed to register locci_request_duration_seconds")
});

/// Upstream health gauge. 1 = healthy, 0 = unhealthy.
/// Labels: `upstream` (group name), `server` (host:port).
pub static UPSTREAM_HEALTH: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec!(
        Opts::new(
            "locci_upstream_health",
            "Upstream server health (1 = healthy, 0 = unhealthy)",
        ),
        &["upstream", "server"]
    )
    .expect("failed to register locci_upstream_health")
});

/// Proxy errors broken down by type.
/// Labels: `mode`, `error_type`.
pub static ERRORS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        Opts::new("locci_errors_total", "Total number of proxy errors"),
        &["mode", "error_type"]
    )
    .expect("failed to register locci_errors_total")
});

// ── Instrumentation helpers ───────────────────────────────────────────────────

/// Record a completed proxied request.
pub fn record_request(mode: &str, upstream: &str, status: &str, duration_secs: f64) {
    REQUESTS_TOTAL
        .with_label_values(&[mode, upstream, status])
        .inc();
    REQUEST_DURATION_SECONDS
        .with_label_values(&[mode, upstream])
        .observe(duration_secs);
}

/// Record a proxy error.
pub fn record_error(mode: &str, error_type: &str) {
    ERRORS_TOTAL.with_label_values(&[mode, error_type]).inc();
}

/// Update the health gauge for one upstream server.
pub fn set_upstream_health(upstream: &str, server: &str, healthy: bool) {
    UPSTREAM_HEALTH
        .with_label_values(&[upstream, server])
        .set(if healthy { 1 } else { 0 });
}

/// Render all registered metrics in Prometheus text exposition format.
/// Gathers from the global default registry.
pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("failed to encode metrics");
    String::from_utf8(buffer).expect("metrics are valid UTF-8")
}

/// Force-initialise all metric descriptors so they appear in scrape output
/// from the first request, before any requests arrive.
pub fn init() {
    once_cell::sync::Lazy::force(&REQUESTS_TOTAL);
    once_cell::sync::Lazy::force(&REQUEST_DURATION_SECONDS);
    once_cell::sync::Lazy::force(&UPSTREAM_HEALTH);
    once_cell::sync::Lazy::force(&ERRORS_TOTAL);
}
