use http::uri::Authority;
use hyper::{Request, body::Incoming, server::conn::http1};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioIo;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::{CertificateDer, pem::PemObject};
use std::error::Error;
use std::ops::ControlFlow;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tower::{
    ServiceBuilder, buffer::BufferLayer, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer,
};
use tracing::{info, warn};

use mesh_core::config::ProxyConfig;

use crate::MtlsClient;
use crate::admin::admin_handler;
use crate::forwarder::forward;
use crate::forwarder::forward_mesh;
use crate::graceful::run_graceful;
use crate::layer::PerRouteTimeoutLayer;
use crate::layer::path::{PathFragment, PathFragments, fragments_from_str};
use crate::layer::{AclLayer, ErrorToHttpLayer};
use crate::load_balance::{Cluster, Endpoint};
use crate::resolver::Resolver;
use crate::{hash, tls};

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
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::error!("Instance task failed: {}", e);
            }
            Err(_) => {
                tracing::error!("Instance shutdown timed out");
            }
        }
    }
}

pub fn start_instance(cfg: ProxyConfig, client: crate::HttpClient) -> crate::Result<Instance> {
    let (shutdown, rx) = Shutdown::new();
    let rx_admin = rx.clone();
    let tls_acceptor = if let Some((cert, key)) = cfg.tls.cert_and_key() {
        info!("TLS cert and key configured, enabling TLS for client ingress");
        let tls_config = tls::get_server_config(&cert, &key).or_else(|e| {
            Err(crate::error::ProxyError::SomeError(format!(
                "Failed to load TLS config: {}",
                e
            )))
        })?;
        Some(tokio_rustls::TlsAcceptor::from(tls_config))
    } else {
        warn!("TLS cert or key not configured, disabling TLS for client ingress");
        None
    };

    let join = tokio::spawn(async move {
        let endpoints: Vec<Arc<Endpoint>> = cfg
            .cluster
            .iter()
            .flat_map(|(_, addrs)| addrs.iter())
            .map(|authority| {
                Arc::new(Endpoint::new(
                    Authority::from_str(authority).expect("valid authority"),
                ))
            })
            .collect();
        let cluster = Arc::new(Cluster::new(client.clone(), endpoints));
        cluster.clone().spawn_active_health();

        let secret: [u8; 32] = *b"abcdabcdabcdabcdabcdabcdabcdabcd";
        let mut ring = hash::ring::HashRing::new(secret);
        ring.build_mp(
            cluster
                .endpoints
                .iter()
                .enumerate()
                .map(|(i, e)| (e.authority.as_str(), i)),
        );
        let ring = Arc::new(ring);

        let admin_listener = tokio::net::TcpListener::bind(&cfg.admin_bind).await?;
        let config_clone = cfg.clone();
        let cluster_clone = cluster.clone();
        let admin_handle = tokio::spawn(run_graceful(
            admin_listener,
            rx_admin,
            async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
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
            },
        ));

        let egress_handle = if let Some(rootca) = cfg.root_ca
            && let Some(egress) = &cfg.egress
        {
            let mut ca = RootCertStore::empty();
            let pem = CertificateDer::from_pem_file(rootca)?;
            ca.add(pem.clone())
                .map_err(|e| {
                    println!("Failed to add CA certificate: {:?}", e);
                })
                .unwrap();
            let client_crt = CertificateDer::from_pem_file(&egress.tls_client_crt)?;
            let client_cfg = ClientConfig::builder()
                .with_root_certificates(ca)
                .with_client_auth_cert(
                    vec![client_crt],
                    PrivateKeyDer::from_pem_file(&egress.tls_client_key)?,
                )?;
            let resolver = Resolver::new(None);
            let mut mtls_http_connector = HttpConnector::new_with_resolver(resolver);
            mtls_http_connector.enforce_http(false);
            let mtls_connector = hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(client_cfg)
                .https_only()
                .enable_http1()
                .wrap_connector(mtls_http_connector);
            let mtls_client: MtlsClient =
                hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                    .build(mtls_connector);

            let rx_mtls_egress = rx.clone();
            let mtls_egress_listener =
                tokio::net::TcpListener::bind(&egress.mesh_egress_bind).await?;
            let mtls_egress_handle = tokio::spawn(run_graceful(
                mtls_egress_listener,
                rx_mtls_egress,
                async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
                    let io = TokioIo::new(stream);

                    let svc = tower::service_fn({
                        let mtls_client = mtls_client.clone();
                        move |r: Request<Incoming>| forward_mesh(r, mtls_client.clone())
                    });
                    let svc = TowerToHyperService::new(svc);
                    let fut = graceful.watch(http1::Builder::new().serve_connection(io, svc));
                    tasks.spawn(async move {
                        if let Err(err) = fut.await {
                            let mut idx = 0;
                            let mut cur: &dyn std::error::Error = &err;
                            loop {
                                warn!("mTLS egress error[{}]: {}", idx, cur);
                                if let Some(src) = cur.source() {
                                    idx += 1;
                                    cur = src;
                                } else {
                                    break;
                                }
                            }
                        }
                    });
                    ControlFlow::Continue(())
                },
            ));
            info!("mTLS egress listening on {}", &egress.mesh_egress_bind);
            Some(mtls_egress_handle)
        } else {
            warn!("either root_ca or egress config missing, mTLS egress disabled");
            None
        };

        let route_timeout_clone = cfg.route_timeout.clone();
        let svc = tower::service_fn(move |r: Request<Incoming>| {
            let rules: Vec<(PathFragments, Duration)> = route_timeout_clone
                .iter()
                .map(|(k, v)| (k.clone(), Duration::from_millis(*v)))
                .map(|(k, v)| (fragments_from_str(&k), v))
                .collect();
            forward(
                r,
                client.clone(),
                cluster.clone(),
                rules.clone(), // route timeouts
                10000,         // default timeout ms if none matched
                cfg.strategy.clone(),
                Some(ring.clone()),
            )
        });

        let svc = ServiceBuilder::new()
            .layer(ErrorToHttpLayer::new())
            .layer(ConcurrencyLimitLayer::new(512))
            .layer(BufferLayer::new(512))
            .layer(AclLayer::new(
                cfg.acl.clone(),
                mesh_core::acl::Listener::Mesh,
            ))
            .layer(PerRouteTimeoutLayer::new(
                cfg.route_timeout
                    .iter()
                    .map(|(k, v)| (k.clone(), std::time::Duration::from_millis(*v)))
                    .collect(),
                std::time::Duration::from_secs(3),
            ))
            .layer(TimeoutLayer::new(std::time::Duration::from_secs(10)))
            .service(svc);

        let ingress_handle = if let Some(ingress_bind) = cfg.tls.mesh_ingress_bind()
            && let Some(tls_acceptor) = tls_acceptor.clone()
        {
            let mtls_ingress_listener = tokio::net::TcpListener::bind(&ingress_bind).await?;
            let svc_clone = svc.clone();
            let rx_mtls_ingress = rx.clone();
            let mtls_ingress_handle = tokio::spawn(run_graceful(
                mtls_ingress_listener,
                rx_mtls_ingress,
                async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
                    let stream = match tls_acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("mTLS accept error: {}", e);
                            return ControlFlow::Continue(());
                        }
                    };
                    let io = TokioIo::new(stream);

                    let svc = svc_clone.clone();
                    let svc = TowerToHyperService::new(svc);
                    let fut = graceful.watch(http1::Builder::new().serve_connection(io, svc));
                    tasks.spawn(async move {
                        if let Err(err) = fut.await {
                            if let Some(e) = err.source() {
                                warn!("Error serving mTLS connection: {}", e);
                            } else {
                                warn!("Error serving mTLS connection: {}", err);
                            }
                        }
                    });
                    ControlFlow::Continue(())
                },
            ));
            info!("mTLS ingress listening on {}", &ingress_bind);
            Some(mtls_ingress_handle)
        } else {
            warn!("mTLS ingress bind not configured, mTLS ingress disabled");
            None
        };

        let listener = tokio::net::TcpListener::bind(&cfg.ingress_bind).await?;
        let acl = cfg.acl.clone();
        run_graceful(
            listener,
            rx,
            async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
                let svc = svc.clone();
                let svc = ServiceBuilder::new()
                    .layer(ErrorToHttpLayer::new())
                    .layer(ConcurrencyLimitLayer::new(512))
                    .layer(BufferLayer::new(512))
                    .layer(AclLayer::new(
                        acl.clone(),
                        mesh_core::acl::Listener::Ingress,
                    ))
                    .layer(PerRouteTimeoutLayer::new(
                        cfg.route_timeout
                            .iter()
                            .map(|(k, v)| (k.clone(), std::time::Duration::from_millis(*v)))
                            .collect(),
                        std::time::Duration::from_secs(3),
                    ))
                    .layer(TimeoutLayer::new(std::time::Duration::from_secs(10)))
                    .service(svc);
                let svc = TowerToHyperService::new(svc);
                if let Some(tls_acceptor) = tls_acceptor.clone() {
                    // tls
                    let stream = match tls_acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("mTLS accept error: {}", e);
                            return ControlFlow::Continue(());
                        }
                    };
                    let io = TokioIo::new(stream);
                    let fut = graceful.watch(http1::Builder::new().serve_connection(io, svc));
                    tasks.spawn(async move {
                        if let Err(err) = fut.await {
                            if let Some(e) = err.source() {
                                warn!("Error serving TLS connection: {}", e);
                            } else {
                                warn!("Error serving TLS connection: {}", err);
                            }
                        }
                    });
                } else {
                    // non-tls
                    let io = TokioIo::new(stream);
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
                };

                ControlFlow::Continue(())
            },
        )
        .await?;

        match admin_handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warn!("Admin task failed: {}", e);
            }
            Err(e) => {
                warn!("Admin task join failed: {}", e);
            }
        }

        if let Some(mtls_ingress_handle) = ingress_handle {
            match mtls_ingress_handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    warn!("mTLS task failed: {}", e);
                }
                Err(e) => {
                    warn!("mTLS task join failed: {}", e);
                }
            }
        }

        if let Some(mtls_egress_handle) = egress_handle {
            match mtls_egress_handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    warn!("mTLS task failed: {}", e);
                }
                Err(e) => {
                    warn!("mTLS task join failed: {}", e);
                }
            }
        }

        Ok(())
    });
    let instance = Instance { shutdown, join };
    Ok(instance)
}
