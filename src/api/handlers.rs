use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
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
        .with_state(manager);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_address)
        .await
        .expect("control API bind failed");

    tracing::info!("Control API listening on {}", cfg.bind_address);
    axum::serve(listener, app).await.unwrap();
}

async fn get_status(State(mgr): State<Arc<ServiceManager>>) -> Json<Value> {
    Json(json!({
        "status": "running",
        "mode": format!("{:?}", mgr.config.mode),
        "lb_enabled":      matches!(mgr.config.mode, OperationMode::LoadBalancer),
        "gateway_enabled": matches!(mgr.config.mode, OperationMode::ApiGateway),
    }))
}

async fn get_config(State(mgr): State<Arc<ServiceManager>>) -> Json<Value> {
    Json(json!(&mgr.config))
}

async fn get_metrics() -> Json<Value> {
    // Integrate with pingora_core::modules::prometheus or your own counters
    Json(json!({ "note": "wire up your metrics exporter here" }))
}

async fn add_route(
    State(_mgr): State<Arc<ServiceManager>>,
    Json(_body): Json<Value>,
) -> StatusCode {
    // Hot-reload requires rebuilding GatewayProxy; simplest approach is SIGHUP
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
