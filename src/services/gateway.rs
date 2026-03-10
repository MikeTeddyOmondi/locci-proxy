use async_trait::async_trait;
use pingora_core::prelude::*;
use pingora_core::server::Server;
use pingora_http::RequestHeader;
use pingora_load_balancing::{LoadBalancer, selection::RoundRobin};
use pingora_proxy::{ProxyHttp, Session, http_proxy_service};
use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::config::{RouteConfig, UnifiedConfig};
use crate::errors::{ProxyError, ProxyResult};
use crate::services::lb::build_lb;

pub struct GatewayProxy {
    // (match_regex, strip_prefix_regex, route)
    // strip_prefix_regex is pre-compiled at startup when strip_prefix: true,
    // eliminating the runtime unwrap() on the hot path.
    routes: Vec<(regex::Regex, Option<regex::Regex>, RouteConfig)>,
    // Each upstream group has its own LoadBalancer — round-robin with health
    // check support, identical to lb mode.
    upstreams: Arc<HashMap<String, Arc<LoadBalancer<RoundRobin>>>>,
    /// Global connect timeout applied to every upstream peer.
    connect_timeout: Option<Duration>,
    /// Global read timeout fallback — overridden per route by RouteConfig.timeout_secs.
    read_timeout: Option<Duration>,
}

impl GatewayProxy {
    fn new(
        config: &UnifiedConfig,
        upstreams: HashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    ) -> ProxyResult<Self> {
        let gw_cfg =
            config
                .api_gateway
                .as_ref()
                .ok_or_else(|| ProxyError::MissingConfigSection {
                    mode: format!("{:?}", config.mode),
                    section: "api_gateway",
                })?;

        let mut routes = Vec::new();
        for route in gw_cfg.routes.values() {
            let match_re =
                regex::Regex::new(&route.path_pattern).map_err(|e| ProxyError::InvalidRegex {
                    pattern: route.path_pattern.clone(),
                    source: e,
                })?;

            // Pre-compile the strip-prefix regex so the hot path never calls
            // Regex::new at request time.
            let strip_re = if route.strip_prefix.unwrap_or(false) {
                let re = regex::Regex::new(&route.path_pattern).map_err(|e| {
                    ProxyError::InvalidRegex {
                        pattern: route.path_pattern.clone(),
                        source: e,
                    }
                })?;
                Some(re)
            } else {
                None
            };

            // Validate that the route's upstream exists in the map at startup,
            // not at request time.
            if !upstreams.contains_key(&route.upstream) {
                return Err(ProxyError::UpstreamNotFound {
                    name: route.upstream.clone(),
                });
            }

            routes.push((match_re, strip_re, route.clone()));
        }

        // Sort longest pattern first so specific routes always beat catch-alls (e.g. ^/).
        routes.sort_by(|(a, _, _), (b, _, _)| b.as_str().len().cmp(&a.as_str().len()));

        Ok(Self {
            routes,
            upstreams: Arc::new(upstreams),
            connect_timeout: config
                .server
                .upstream_connect_timeout_secs
                .map(Duration::from_secs),
            read_timeout: config
                .server
                .upstream_read_timeout_secs
                .map(Duration::from_secs),
        })
    }

    fn match_route(
        &self,
        path: &str,
    ) -> Option<&(regex::Regex, Option<regex::Regex>, RouteConfig)> {
        self.routes.iter().find(|(re, _, _)| re.is_match(path))
    }
}

#[async_trait]
impl ProxyHttp for GatewayProxy {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let path = session.req_header().uri.path().to_owned();

        let (_, _, route) = self.match_route(&path).ok_or_else(|| {
            Error::explain(
                HTTPStatus(404),
                ProxyError::NoMatchingRoute { path: path.clone() }.to_string(),
            )
        })?;

        let lb = self.upstreams.get(&route.upstream).ok_or_else(|| {
            Error::explain(
                InternalError,
                ProxyError::UpstreamNotFound {
                    name: route.upstream.clone(),
                }
                .to_string(),
            )
        })?;

        // Use the request path as selection key — for RoundRobin the key is
        // ignored, but it prepares for future consistent-hash support.
        let backend = lb.select(path.as_bytes(), 256).ok_or_else(|| {
            Error::explain(
                InternalError,
                ProxyError::NoHealthyPeers {
                    name: route.upstream.clone(),
                }
                .to_string(),
            )
        })?;

        let mut peer = HttpPeer::new(backend, false, String::new());
        peer.options.connection_timeout = self.connect_timeout;
        // Per-route timeout_secs overrides the global read timeout fallback.
        peer.options.read_timeout = route
            .timeout_secs
            .map(Duration::from_secs)
            .or(self.read_timeout);
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        let path = session.req_header().uri.path().to_owned();

        if let Some((_, Some(strip_re), _)) = self.match_route(&path) {
            let new_path = strip_re.replace(&path, "").to_string();
            let new_path = if new_path.is_empty() {
                "/".to_owned()
            } else {
                new_path
            };
            let uri = new_path.parse().map_err(|_| {
                Error::explain(
                    InternalError,
                    ProxyError::InvalidUri {
                        uri: new_path.clone(),
                    }
                    .to_string(),
                )
            })?;
            upstream_request.set_uri(uri);
        }
        Ok(())
    }
}

pub fn add_gateway_service(server: &mut Server, config: &UnifiedConfig) -> ProxyResult<()> {
    let gw_cfg = config
        .api_gateway
        .as_ref()
        .ok_or_else(|| ProxyError::MissingConfigSection {
            mode: format!("{:?}", config.mode),
            section: "api_gateway",
        })?;

    // Build one LoadBalancer per unique upstream referenced across all routes.
    // Each gets its own background health-check task registered with the server.
    let mut upstreams: HashMap<String, Arc<LoadBalancer<RoundRobin>>> = HashMap::new();
    for route in gw_cfg.routes.values() {
        if upstreams.contains_key(&route.upstream) {
            continue;
        }
        let lb_arc = build_lb(server, config, &route.upstream)?;
        upstreams.insert(route.upstream.clone(), lb_arc);
    }

    let proxy = GatewayProxy::new(config, upstreams)?;
    let addr = &config.server.bind_address;
    let mut svc = http_proxy_service(&server.configuration, proxy);
    svc.add_tcp(addr);
    server.add_service(svc);
    tracing::info!("API gateway listening on {addr}");
    Ok(())
}
