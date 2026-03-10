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
use std::{sync::Arc, time::Duration};

use crate::config::UnifiedConfig;
use crate::errors::{ProxyError, ProxyResult};

#[allow(unused_imports)]
use http;

pub struct LbProxy {
    lb: Arc<LoadBalancer<RoundRobin>>,
    upstream_name: String,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
}

#[async_trait]
impl ProxyHttp for LbProxy {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let upstream = self.lb.select(b"", 256).ok_or_else(|| {
            Error::explain(
                InternalError,
                ProxyError::NoHealthyPeers {
                    name: self.upstream_name.clone(),
                }
                .to_string(),
            )
        })?;

        let mut peer = HttpPeer::new(upstream, false, String::new());
        peer.options.connection_timeout = self.connect_timeout;
        peer.options.read_timeout = self.read_timeout;
        Ok(Box::new(peer))
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
