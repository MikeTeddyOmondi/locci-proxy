use async_trait::async_trait;
use pingora_core::prelude::*;
use pingora_core::server::Server;
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
}

impl LbProxy {
    pub fn new(config: &UnifiedConfig) -> ProxyResult<Self> {
        let lb_cfg =
            config
                .load_balancer
                .as_ref()
                .ok_or_else(|| ProxyError::MissingConfigSection {
                    mode: format!("{:?}", config.mode),
                    section: "load_balancer",
                })?;

        let upstream =
            config
                .upstreams
                .get(&lb_cfg.upstream)
                .ok_or_else(|| ProxyError::UpstreamNotFound {
                    name: lb_cfg.upstream.clone(),
                })?;

        if upstream.servers.is_empty() {
            return Err(ProxyError::EmptyUpstream {
                name: lb_cfg.upstream.clone(),
            });
        }

        let mut lb = LoadBalancer::try_from_iter(upstream.servers.iter().map(|s| s.as_str()))
            .map_err(|e| ProxyError::LoadBalancerBuild {
                name: lb_cfg.upstream.clone(),
                source: e,
            })?;

        if let Some(hc) = &upstream.health_check {
            let checker: Box<dyn pingora_load_balancing::health_check::HealthCheck + Send + Sync> =
                match &hc.path {
                    Some(path) => {
                        let mut http_hc = HttpHealthCheck::new(
                            &upstream.servers[0],
                            upstream.tls.unwrap_or(false),
                        );
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
        }

        Ok(Self {
            lb: Arc::new(lb),
            upstream_name: lb_cfg.upstream.clone(),
        })
    }
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

        Ok(Box::new(HttpPeer::new(upstream, false, String::new())))
    }
}

pub fn add_lb_service(server: &mut Server, config: &UnifiedConfig) -> ProxyResult<()> {
    let proxy = LbProxy::new(config)?;
    let addr = &config.server.bind_address;
    let mut svc = http_proxy_service(&server.configuration, proxy);
    svc.add_tcp(addr);
    server.add_service(svc);
    tracing::info!("Load balancer listening on {addr}");
    Ok(())
}
