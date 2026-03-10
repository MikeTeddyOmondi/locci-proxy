use axum::{
    Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{
    config::{ControlApiConfig, OperationMode},
    services::ServiceManager,
};

pub async fn start_control_api(manager: Arc<ServiceManager>, cfg: ControlApiConfig) {
    let app = Router::new()
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/config", get(get_config))
        .route("/api/v1/routes", post(add_route))
        .route("/api/v1/routes/:name", delete(remove_route))
        .route("/api/v1/upstreams", post(add_upstream))
        .route(
            "/api/v1/upstreams/:name/servers/:server",
            delete(remove_server),
        )
        .route("/api/v1/metrics", get(get_metrics))
        // Auth middleware uses the same Arc<ServiceManager> state as the route handlers
        // so it can read control_api.api_key without a separate state type.
        .layer(middleware::from_fn_with_state(
            manager.clone(),
            auth_middleware,
        ))
        .with_state(manager);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_address)
        .await
        .expect("control API bind failed");

    tracing::info!("Control API listening on {}", cfg.bind_address);
    axum::serve(listener, app).await.unwrap();
}

/// Bearer token middleware. Passes through when `control_api.api_key` is `None`.
/// Returns `401 Unauthorized` when the header is missing or the token does not match.
async fn auth_middleware(
    State(mgr): State<Arc<ServiceManager>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(key) = &mgr.config.control_api.api_key {
        let token = request
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        if token != Some(key.as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response();
        }
    }
    next.run(request).await
}

async fn get_status(State(mgr): State<Arc<ServiceManager>>) -> Json<Value> {
    Json(json!({
        "status": "running",
        "mode": format!("{:?}", mgr.config.mode),
        "lb_enabled":      matches!(mgr.config.mode, OperationMode::LoadBalancer),
        "gateway_enabled": matches!(mgr.config.mode, OperationMode::ApiGateway),
    }))
}

/// Returns the full config with sensitive fields redacted.
async fn get_config(State(mgr): State<Arc<ServiceManager>>) -> Json<Value> {
    let mut cfg = json!(&mgr.config);
    redact_sensitive(&mut cfg);
    Json(cfg)
}

/// Replace known sensitive leaf values with `"[redacted]"` in-place.
fn redact_sensitive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if matches!(key.as_str(), "api_key" | "jwt_secret") && val.is_string() {
                    *val = json!("[redacted]");
                } else {
                    redact_sensitive(val);
                }
            }
        }
        Value::Array(arr) => arr.iter_mut().for_each(redact_sensitive),
        _ => {}
    }
}

async fn get_metrics() -> Json<Value> {
    // TASK-007: wire up Prometheus TextEncoder here.
    Json(json!({ "note": "wire up your metrics exporter here" }))
}

async fn add_route(
    State(_mgr): State<Arc<ServiceManager>>,
    Json(_body): Json<Value>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

async fn remove_route(
    State(_mgr): State<Arc<ServiceManager>>,
    Path(_name): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

async fn add_upstream(
    State(_mgr): State<Arc<ServiceManager>>,
    Json(_body): Json<Value>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

async fn remove_server(
    State(_mgr): State<Arc<ServiceManager>>,
    Path((_name, _server)): Path<(String, String)>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
