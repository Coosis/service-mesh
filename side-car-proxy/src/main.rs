use std::sync::atomic::Ordering;
use std::sync::Arc;
use http::header::COOKIE;
use http::uri::PathAndQuery;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Bytes;
use hyper::header::{HeaderValue, HOST};
use hyper::{body::Incoming, Request, Response};
use hyper_util::client::legacy::{connect::HttpConnector, Client};
use load_balance::{Cluster, Endpoint};
use opentelemetry::{global, Context, KeyValue};
use opentelemetry_http::{HeaderExtractor, HeaderInjector};
use tracing::{debug, info, warn};
use tracing_subscriber::fmt::format::FmtSpan;
use opentelemetry::trace::{SpanKind, Status, TraceContextExt, Tracer};

use crate::instance::start_instance;
use crate::tel::otel::{self, init_tracer_exporter, init_tracer_provider, init_tracing_and_propagation};
use mesh_core::config;
use mesh_core::strategy::LoadBalanceStrategy;
type ProxyConfig = config::ProxyConfig<crate::error::ProxyError>;
// use side_car_proxy::config;

mod util;
mod tel;
mod router;
mod load_balance;
mod admin;
mod graceful;
mod tls;
mod hash;
mod circuit_breaker;
mod instance;
mod error;

/// main forward utility. first time, we pick an endpoint by either:
/// 1. p2c
/// 2. round robin
/// after we pick an endpoint, we route to that endpoint
/// that specific endpoint is able to set "x-client-id" for client to opt-in sticky session
/// 
/// next time, if "x-client-id" is present in cookie header, we use consistent hashing to pick the
/// endpoint
/// otherwise, repeat the above process
async fn forward(
    r: Request<Incoming>,
    client: Client<HttpConnector, BoxBody<Bytes, error::ProxyError>>,
    cluster: Arc<Cluster>,

    strat: LoadBalanceStrategy,
    ring: Option<Arc<hash::ring::HashRing>>,
) -> Result<Response<BoxBody<Bytes, error::ProxyError>>> {
    let t0 = std::time::Instant::now();

    let (mut parts, body) = r.into_parts();
    let parent_ctx = global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderExtractor(&parts.headers))
    });

    let tracer = otel::get_tracer();
    let otel_span = tracer.span_builder(format!("{} {}", parts.method, parts.uri.path()))
        .with_kind(SpanKind::Server)
        .start_with_context(tracer, &parent_ctx);

    let (res, cx): (Result<Response<BoxBody<Bytes, error::ProxyError>>>, Context) = {
        // per RFC 9110, drop specific headers
        util::strip_hop_by_hop(&mut parts.headers);

        // re-write url for client connector
        // tls term at proxy

        // endpoint selection
        let sid = parts.headers.get_all(COOKIE).iter()
            .find_map(|val| {
                let s = val.to_str().ok()?;
                // Cookie header format: "a=1; b=2; c=3"
                for pair in s.split(';') {
                    let pair = pair.trim();
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "x-client-id" {
                            return Some(v.to_string());
                        }
                    }
                }
                None
            });
        let ep: Arc<Endpoint>;
        if let Some(ring) = ring && let Some(sid) = sid {
            // Prefer the sticky endpoint, but skip ejected/unhealthy ones.
            if let Some(idx) = ring.get_index_filtered(&sid, |i| {
                cluster.endpoints[i].healthy.load(Ordering::Relaxed)
            }) {
                info!("Routing sid={} to endpoint idx={} (sticky, healthy)", sid, idx);
                ep = cluster.endpoints[idx].clone();
            } else if let Some(p2c_ep) = cluster.pick_p2c() {
                // No healthy node along the ring path; fall back to p2c.
                warn!("No healthy endpoints in ring walk for sid={}; falling back to p2c", sid);
                ep = p2c_ep;
            } else {
                warn!("No healthy endpoints available (ring & p2c)");
                return Err(error::ProxyError::NoHealthyEndpoints);
            }
        } else {
            // ! uncommnet to use round robin instead!
            ep = match strat {
                LoadBalanceStrategy::P2C => {
                    match cluster.pick_p2c() {
                        Some(ep) => ep,
                        None => {
                            warn!("No healthy endpoints available");
                            return Err(error::ProxyError::NoHealthyEndpoints);
                        }
                    }
                }
                LoadBalanceStrategy::RoundRobin => {
                    match cluster.pick_round_robin() {
                        Some(ep) => ep,
                        None => {
                            warn!("No healthy endpoints available");
                            return Err(error::ProxyError::NoHealthyEndpoints);
                        }
                    }
                }
            };
        }

        let uri = http::Uri::builder()
            .scheme("http")
            .authority(ep.authority.clone())
            .path_and_query(parts.uri.path_and_query().map_or(PathAndQuery::from_static("/"), |pq| pq.to_owned()))
            .build()?;
        parts.uri = uri;

        parts.headers.insert(
            HOST,
            HeaderValue::from_str(&ep.authority.as_str()).unwrap_or_else(|_| HeaderValue::from_static("unknown"))
        );

        // ensure_w3c(&mut parts.headers);

        let limit = Limited::new(body, 2 * 1024 * 1024)
            .map_err(|e| error::ProxyError::SomeError(format!("Body size limit error: {}", e)))
            .boxed();

        let cx = Context::current_with_span(otel_span);
        global::get_text_map_propagator(|prop| {
            prop.inject_context(&cx, &mut HeaderInjector(&mut parts.headers))
        });
        let req = Request::from_parts(parts, limit);

        let now_ms = cluster.now_ms();
        if !ep.breaker.allow(now_ms) {
            use hyper::StatusCode;
            let body = Full::new(Bytes::from("Service Unavailable"))
                .map_err(|e| error::ProxyError::SomeError(format!("Full body error: {}", e)))
                .boxed();
            let resp = Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(body)
                .map_err(|e| error::ProxyError::SomeError(format!("resp build: {e}")))?;
            warn!("Circuit breaker open for {}, rejecting request", ep.authority);
            return Ok(resp);
        }

        ep.in_flight.fetch_add(1, Ordering::Relaxed);
        let t0 = std::time::Instant::now();
        let res = client.request(req).await;
        let elapsed = t0.elapsed().as_millis() as i64; // tracing
        cx.span().set_attribute(KeyValue::new("upstream_elapsed_ms", elapsed));
        let rtt_ms = t0.elapsed().as_millis() as u64; // ewma
        ep.in_flight.fetch_sub(1, Ordering::Relaxed);

        let ok = match &res {
            Ok(resp) => {
                let s = resp.status();
                debug!("Response from {}: {}", ep.authority, s);
                s.is_success() || !(400u16..500u16).contains(&s.as_u16())
            }
            Err(_) => {
                debug!("Request error to {}", ep.authority);
                false
            }
        };

        if !ok {
            warn!("Request to {} failed", ep.authority);
            cx.span().set_status(Status::error("Upstream request failed"));
        }
        // comment out to test out circuit breaker
        cluster.observe(&ep, ok, rtt_ms);
        cx.span().set_status(if ok {
            Status::Ok
        } else {
            Status::error("Upstream request failed")
        });

        ep.breaker.record(cluster.now_ms(), ok);

        cx.span().set_attribute(KeyValue::new("upstream", ep.authority.as_str().to_string()));
        let res = res?;
        let (parts, body) = res.into_parts();
        let body = body
            .map_err(|e| error::ProxyError::SomeError(format!("Response body error: {}", e)))
            .boxed();
        let proxied = Response::from_parts(parts, body);
        (
            Ok::<Response<BoxBody<Bytes, error::ProxyError>>, error::ProxyError>(proxied),
            cx
        )
    };

    let elapsed = t0.elapsed().as_millis() as i64;
    debug!("Total elapsed time: {} ms", elapsed);
    cx.span().set_attribute(KeyValue::new("elapsed_ms", elapsed));

    res
}

// type Result<T> = std::result::Result<T, BoxError>;
type Result<T> = std::result::Result<T, error::ProxyError>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        // .with_max_level(tracing::Level::DEBUG)
        .with_max_level(tracing::Level::INFO)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .init();

    init_tracer_provider();
    init_tracing_and_propagation();
    init_tracer_exporter().expect("failed to init tracer exporter");

    // let config_path = std::env::var("PROXY_CONFIG")
    //     .unwrap_or_else(|_| "example/proxy_config.toml".to_string());
    // let config = config::ProxyConfig::from_file(&config_path).await?;
    // debug!("Using load balancing strategy: {:?}", config.strategy);
    // debug!("Loaded config: {:?}", config);
    // 
    let fullchain = std::env::var("TLS_FULLCHAIN")
        .unwrap_or_else(|_| {
            warn!("TLS_FULLCHAIN not set, using fullchain.pem as default");
            "fullchain.pem".to_string()
        });
    let key = std::env::var("TLS_KEY")
        .unwrap_or_else(|_| {
            warn!("TLS_KEY not set, using server.key as default");
            "server.key".to_string()
        });
    let tls_config = tls::get_server_config(fullchain, key).unwrap();
    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config);

    let client: Client<_, BoxBody<Bytes, error::ProxyError>> = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpConnector::new());

    let config_txt = reqwest::get("http://127.0.0.1:13000/config").await?;
    let cfg = config::ProxyConfig::from_content(&config_txt.text().await.unwrap()).await?;
    let mut instance = start_instance(tls_acceptor.clone(), client.clone(), cfg.clone())?;

    let (tx, mut rx) = tokio::sync::watch::channel(cfg);
    tokio::spawn(async move {
        loop {
            let response = reqwest::get("http://127.0.0.1:13000/poll_config").await;
            match response {
                Ok(resp) => { 
                    info!("received new config from admin server: {:?}", resp);
                    match resp.status().is_success() {
                        true => {},
                        false => {
                            warn!("Config poll returned non-success status: {}", resp.status());
                            continue;
                        }
                    }
                    let txt = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            warn!("Failed to read config text: {}", e);
                            continue;
                        }
                    };
                    let config = match config::ProxyConfig::from_content(&txt).await {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to parse config: {}", e);
                            continue;
                        }
                    };
                    tx.send(config).unwrap();
                }
                Err(e) => {
                    warn!("Config poll error: {}", e);
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Main Received Ctrl-C, shutting down");
                instance.request_shutdown();
                instance.join_with_deadline(std::time::Duration::from_secs(15)).await;
                break;
            }
            Ok(_) = rx.changed() => {
                let new_config = rx.borrow().clone();
                info!("Applying new config: {:?}", new_config);

                instance.request_shutdown();
                instance.join_with_deadline(std::time::Duration::from_secs(15)).await;
                instance = start_instance(tls_acceptor.clone(), client.clone(), new_config.clone())?;
            }
        }
    }

    // shutdown_tracing(p);
    Ok(())
}
