use async_trait::async_trait;
use pingora_core::prelude::*;
use pingora_core::server::Server;
use pingora_core::services::background::background_service;
use pingora_load_balancing::{
    LoadBalancer,
    health_check::{HttpHealthCheck, TcpHealthCheck},
    selection::RoundRobin,
};
use pingora_proxy::{ProxyHttp, Session, http_proxy_service};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::config::UnifiedConfig;
use crate::errors::{ProxyError, ProxyResult};
use crate::metrics;

#[allow(unused_imports)]
use http;
use pingora_http::RequestHeader;

pub struct LbProxy {
    lb: Arc<LoadBalancer<RoundRobin>>,
    upstream_name: String,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
}

/// Per-request context for the LB proxy.
pub struct LbCtx {
    pub start: Instant,
    pub request_id: String,
    pub server: String,
}

#[async_trait]
impl ProxyHttp for LbProxy {
    type CTX = LbCtx;
    fn new_ctx(&self) -> Self::CTX {
        LbCtx {
            start: Instant::now(),
            request_id: super::next_request_id(),
            server: String::new(),
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let backend = self.lb.select(b"", 256).ok_or_else(|| {
            Error::explain(
                InternalError,
                ProxyError::NoHealthyPeers {
                    name: self.upstream_name.clone(),
                }
                .to_string(),
            )
        })?;

        ctx.server = backend.addr.to_string();
        let mut peer = HttpPeer::new(backend, false, String::new());
        peer.options.connection_timeout = self.connect_timeout;
        peer.options.read_timeout = self.read_timeout;
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request
            .insert_header("x-request-id", ctx.request_id.as_str())
            .or_err(InternalError, "insert x-request-id")?;
        Ok(())
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        let duration = ctx.start.elapsed();
        let method = session.req_header().method.as_str().to_owned();
        let path = session.req_header().uri.path().to_owned();
        let status = session
            .response_written()
            .map(|r| r.status.as_u16())
            .unwrap_or(0);

        if let Some(err) = e {
            tracing::warn!(
                request_id = %ctx.request_id,
                path = %path,
                method = %method,
                upstream = %self.upstream_name,
                server = %ctx.server,
                error_type = err.etype().as_str(),
                "upstream error"
            );
            metrics::record_error("lb", "upstream_error");
        } else {
            tracing::info!(
                request_id = %ctx.request_id,
                path = %path,
                method = %method,
                upstream = %self.upstream_name,
                server = %ctx.server,
                status = status,
                duration_ms = duration.as_millis(),
                "request"
            );
            metrics::record_request(
                "lb",
                &self.upstream_name,
                &status.to_string(),
                duration.as_secs_f64(),
            );
        }
    }
}

/// Build a [`LoadBalancer`] for the named upstream group and register a background
/// health-check service with the Pingora server. Returns the shared [`Arc`] so the
/// proxy and the background task operate on the same instance.
pub(crate) fn build_lb(
    server: &mut Server,
    config: &UnifiedConfig,
    upstream_name: &str,
) -> ProxyResult<Arc<LoadBalancer<RoundRobin>>> {
    let upstream =
        config
            .upstreams
            .get(upstream_name)
            .ok_or_else(|| ProxyError::UpstreamNotFound {
                name: upstream_name.to_owned(),
            })?;

    if upstream.servers.is_empty() {
        return Err(ProxyError::EmptyUpstream {
            name: upstream_name.to_owned(),
        });
    }

    // Initialise the health gauge for every server in this upstream group.
    // Default to healthy (1) — health checks will update the gauge over time
    // when a health_check block is configured.
    for server_addr in &upstream.servers {
        metrics::set_upstream_health(upstream_name, server_addr, true);
    }

    let mut lb =
        LoadBalancer::try_from_iter(upstream.servers.iter().map(|s| s.as_str())).map_err(|e| {
            ProxyError::LoadBalancerBuild {
                name: upstream_name.to_owned(),
                source: e,
            }
        })?;

    if let Some(hc) = &upstream.health_check {
        let checker: Box<dyn pingora_load_balancing::health_check::HealthCheck + Send + Sync> =
            match &hc.path {
                Some(path) => {
                    let mut http_hc =
                        HttpHealthCheck::new(&upstream.servers[0], upstream.tls.unwrap_or(false));
                    http_hc.req.set_uri(path.parse::<http::Uri>().map_err(|e| {
                        ProxyError::InvalidHealthCheckUri {
                            uri: path.clone(),
                            reason: e.to_string(),
                        }
                    })?);
                    Box::new(http_hc)
                }
                None => TcpHealthCheck::new(),
            };

        lb.set_health_check(checker);
        lb.health_check_frequency = Some(Duration::from_secs(hc.interval_secs));

        // Wrap in a background service so Pingora calls lb.update() and runs
        // health checks at health_check_frequency. bg.task() returns the shared Arc.
        let bg = background_service(&format!("health check: {upstream_name}"), lb);
        let lb_arc = bg.task();
        server.add_service(bg);

        tracing::info!(
            upstream = upstream_name,
            interval_secs = hc.interval_secs,
            "health check background task registered"
        );

        return Ok(lb_arc);
    }

    // No health check configured — wrap anyway so the LoadBalancer runs its
    // initial update() to populate the backend selection index.
    let bg = background_service(&format!("lb: {upstream_name}"), lb);
    let lb_arc = bg.task();
    server.add_service(bg);

    Ok(lb_arc)
}

pub fn add_lb_service(server: &mut Server, config: &UnifiedConfig) -> ProxyResult<()> {
    let lb_cfg = config
        .load_balancer
        .as_ref()
        .ok_or_else(|| ProxyError::MissingConfigSection {
            mode: format!("{:?}", config.mode),
            section: "load_balancer",
        })?;

    let lb_arc = build_lb(server, config, &lb_cfg.upstream)?;

    let proxy = LbProxy {
        lb: lb_arc,
        upstream_name: lb_cfg.upstream.clone(),
        connect_timeout: config
            .server
            .upstream_connect_timeout_secs
            .map(Duration::from_secs),
        read_timeout: config
            .server
            .upstream_read_timeout_secs
            .map(Duration::from_secs),
    };

    let addr = &config.server.bind_address;
    let mut svc = http_proxy_service(&server.configuration, proxy);
    svc.add_tcp(addr);
    server.add_service(svc);
    tracing::info!("Load balancer listening on {addr}");
    Ok(())
}
