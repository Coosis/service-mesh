use std::error::Error;
use std::ops::ControlFlow;
use std::str::FromStr;
use std::sync::Arc;
use hyper_util::server::graceful::GracefulShutdown;
use http::uri::Authority;
use http_body_util::combinators::BoxBody;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use opentelemetry_http::Bytes;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tower::{buffer::BufferLayer, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer, ServiceBuilder};
use tracing::warn;
use hyper::{body::Incoming, server::conn::http1, Request};

use crate::admin::admin_handler;
use crate::graceful::run_graceful;
use crate::hash;
use crate::load_balance::Cluster;
use crate::load_balance::Endpoint;
use crate::router::PerRouteTimeoutLayer;

pub struct Shutdown {
    tx: tokio::sync::watch::Sender<bool>,
}

impl Shutdown {
    pub fn new() -> (Self, tokio::sync::watch::Receiver<bool>) {
        let (tx, rx) = tokio::sync::watch::channel(false);
        (Shutdown { tx }, rx)
    }

    pub fn trigger(&self) {
        let _ = self.tx.send(true);
    }
}

pub struct Instance {
    shutdown: Shutdown,
    join: JoinHandle<crate::Result<()>>,
}

impl Instance {
    pub fn request_shutdown(&self) {
        self.shutdown.trigger();
    }
    pub async fn join_with_deadline(self, deadline: std::time::Duration) {
        match tokio::time::timeout(deadline, self.join).await {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                tracing::error!("Instance task failed: {}", e);
            }
            Err(_) => {
                tracing::error!("Instance shutdown timed out");
            }
        }
    }
}

pub fn start_instance(
    tls_acceptor: tokio_rustls::TlsAcceptor,
    client: Client<HttpConnector, BoxBody<Bytes, crate::error::ProxyError>>,
    cfg: crate::config::ProxyConfig,
) ->crate::Result<Instance> {
    let (shutdown, rx) = Shutdown::new();
    let rx_admin = rx.clone();
    let join = tokio::spawn(async move {
        let endpoints: Vec<Arc<Endpoint>> = cfg.cluster.iter()
            .flat_map(|(_, addrs)| addrs.iter())
            .map(|authority| Arc::new(Endpoint::new(Authority::from_str(authority).expect("valid authority"))))
            .collect();
        let cluster = Arc::new(Cluster::new(
                client.clone(),
                endpoints,
        ));
        cluster.clone().spawn_active_health();

        let secret: [u8; 32] = *b"abcdabcdabcdabcdabcdabcdabcdabcd";
        let mut ring = hash::ring::HashRing::new(secret);
        ring.build_mp(
            cluster.endpoints
            .iter()
            .enumerate()
            .map(|(i, e)| (e.authority.as_str(), i))
        );
        let ring = Arc::new(ring);

        let admin_listener = tokio::net::TcpListener::bind("0.0.0.0:15000").await?;
        let config_clone = cfg.clone();
        let cluster_clone = cluster.clone();
        let admin_handle = tokio::spawn(
            run_graceful(admin_listener, rx_admin, async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
                let io = TokioIo::new(stream);
                let svc = tower::service_fn({
                    let cluster_clone = cluster_clone.clone();
                    let config_clone = config_clone.clone();
                    move |r: Request<Incoming>| {
                        admin_handler(r, cluster_clone.clone(), config_clone.clone())
                    }
                });
                let svc = TowerToHyperService::new(svc);
                let fut = graceful.watch(http1::Builder::new().serve_connection(io, svc));
                tasks.spawn(async move {
                    if let Err(err) = fut.await {
                        if let Some(e) = err.source() {
                            warn!("Error serving admin connection: {}", e);
                        } else {
                            warn!("Error serving admin connection: {}", err);
                        }
                    }
                });
                ControlFlow::Continue(())
            })
        );

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8333").await?;
        run_graceful(listener, rx, async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
            let stream = match tls_acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("TLS accept error: {}", e);
                    return ControlFlow::Continue(());
                }
            };
            let io = TokioIo::new(stream);

            let svc = tower::service_fn({
                let client_clone = client.clone();
                let cluster_clone = cluster.clone();
                let ring_clone = ring.clone();
                let strat = cfg.strategy.clone();
                move |r: Request<Incoming>| {
                    crate::forward(
                        r, 
                        client_clone.clone(),
                        cluster_clone.clone(),
                        strat.clone(),
                        Some(ring_clone.clone())
                    )
                }
            });
            let svc = ServiceBuilder::new()
                .layer(ConcurrencyLimitLayer::new(512))
                .layer(BufferLayer::new(512))
                .layer(PerRouteTimeoutLayer::new(
                        cfg.route_timeout.clone().iter()
                        .map(|(k, v)| (k.clone(), std::time::Duration::from_millis(*v)))
                        .collect(),
                        std::time::Duration::from_secs(3),
                ))
                .layer(TimeoutLayer::new(std::time::Duration::from_secs(10)))
                .service(svc);
            let svc = TowerToHyperService::new(svc);
            let fut = graceful.watch(http1::Builder::new().serve_connection(io, svc));
            tasks.spawn(async move {
                if let Err(err) = fut.await {
                    if let Some(e) = err.source() {
                        warn!("Error serving connection: {}", e);
                    } else {
                        warn!("Error serving connection: {}", err);
                    }
                }
            });
            ControlFlow::Continue(())
        }).await?;

        match admin_handle.await {
            Ok(Ok(())) => {},
            Ok(Err(e)) => {
                warn!("Admin task failed: {}", e);
            }
            Err(e) => {
                warn!("Admin task join failed: {}", e);
            }
        }

        Ok(())
    });
    let instance = Instance { shutdown, join };
    Ok(instance)
}
