use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::{ProxyError, ProxyResult};

pub mod api;
pub mod cli;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedConfig {
    pub mode: OperationMode,
    pub server: ServerConfig,
    pub logging: Option<LoggingConfig>,
    /// Upstream groups shared by both modes.
    pub upstreams: HashMap<String, UpstreamConfig>,
    /// Required when mode = load_balancer.
    pub load_balancer: Option<LoadBalancerConfig>,
    /// Required when mode = api_gateway.
    pub api_gateway: Option<ApiGatewayConfig>,
    pub control_api: ControlApiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    LoadBalancer,
    ApiGateway,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: trace | debug | info | warn | error
    /// Overridden by the RUST_LOG environment variable when set.
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub bind_address: String,
    pub workers: Option<usize>,
    pub tls: Option<TlsConfig>,
    /// Seconds to wait for a TCP connection to an upstream server. Default: no timeout.
    pub upstream_connect_timeout_secs: Option<u64>,
    /// Seconds to wait for the upstream to send a response after the request is sent.
    /// Overridden per route by `RouteConfig.timeout_secs` in api_gateway mode.
    /// Default: no timeout.
    pub upstream_read_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    /// List of "host:port" strings. Pingora works at the TCP layer — no scheme prefix.
    pub servers: Vec<String>,
    pub strategy: String, // "round_robin" | "weighted" | "consistent"
    pub tls: Option<bool>,
    pub sni: Option<String>,
    pub health_check: Option<HealthCheckConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub interval_secs: u64,
    pub timeout_secs: u64,
    /// HTTP path for active health checks; omit for TCP-only checks.
    pub path: Option<String>,
}

/// Load-balancer mode config — only names which upstream group to balance across.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadBalancerConfig {
    /// Name of the upstream group (defined in top-level `upstreams`) to distribute traffic across.
    pub upstream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiGatewayConfig {
    pub routes: HashMap<String, RouteConfig>,
    pub middlewares: MiddlewareConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub path_pattern: String,
    pub methods: Vec<String>,
    /// Name of the upstream group (defined in top-level `upstreams`) to forward to.
    pub upstream: String,
    pub strip_prefix: Option<bool>,
    pub timeout_secs: Option<u64>,
    pub middlewares: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MiddlewareConfig {
    pub rate_limiting: Option<RateLimitConfig>,
    pub authentication: Option<AuthConfig>,
    pub cors: Option<CorsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: Option<u32>,
    pub key_header: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: Option<String>,
    pub excluded_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlApiConfig {
    pub enabled: bool,
    pub bind_address: String,
    pub api_key: Option<String>,
}

impl UnifiedConfig {
    pub fn load_from_file(path: &str) -> ProxyResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| ProxyError::ConfigFileRead {
            path: path.to_owned(),
            source: e,
        })?;
        let config: UnifiedConfig =
            serde_yaml::from_str(&content).map_err(|e| ProxyError::ConfigFileParse {
                path: path.to_owned(),
                source: e,
            })?;
        Ok(config)
    }
}
