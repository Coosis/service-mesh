use std::error::Error;
use std::ops::ControlFlow;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use admin::admin_handler;
use graceful::run_graceful;
use http::header::COOKIE;
use http::uri::{Authority, PathAndQuery};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Limited};
use hyper::body::Bytes;
use hyper::header::{HeaderValue, HOST};
use hyper::{body::Incoming, server::conn::http1, Request, Response};
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use hyper_util::{client::legacy::{connect::HttpConnector, Client}, rt::TokioIo};
use load_balance::{Cluster, Endpoint};
use tokio::task::JoinSet;
use tokio::try_join;
use tower::{buffer::BufferLayer, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer, ServiceBuilder};
use tracing::{debug, info_span, info, warn, Instrument, Span};
use tracing_subscriber::fmt::format::FmtSpan;
use w3c::ensure_w3c;

mod util;
mod error;
mod config;
mod router;
mod w3c;
mod load_balance;
mod admin;
mod graceful;
mod tls;
mod hash;

async fn forward(
    r: Request<Incoming>,
    client: Client<HttpConnector, BoxBody<Bytes, error::ProxyError>>,
    cluster: Arc<Cluster>,

    ring: Option<Arc<hash::ring::HashRing>>,
) -> Result<Response<hyper::body::Incoming>> {
    let span = info_span!(
        "Request",
        method = %r.method(),
        path = %r.uri().path(),
        elapsed_ms = 0_i64,
    );
    let span_clone = span.clone();
    let t0 = std::time::Instant::now();

    let res = async move {
        let (mut parts, body) = r.into_parts();

        // per RFC 9110, drop specific headers
        util::strip_hop_by_hop(&mut parts.headers);

        // re-write url for client connector
        // tls term at proxy

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
            // let ep = match cluster.pick_round_robin() {
            ep = match cluster.pick_p2c() {
                Some(ep) => ep,
                None => {
                    warn!("No healthy endpoints available");
                    return Err(error::ProxyError::NoHealthyEndpoints);
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

        ensure_w3c(&mut parts.headers);

        let limit = Limited::new(body, 2 * 1024 * 1024)
            .map_err(|e| error::ProxyError::SomeError(format!("Body size limit error: {}", e)))
            .boxed();
        let req = Request::from_parts(parts, limit);

        let call_span = info_span!(
            "Hop", 
            to = %req.uri(),
            elapsed_ms = 0_i64,
        );
        let call_span_clone = call_span.clone();

        ep.in_flight.fetch_add(1, Ordering::Relaxed);
        let t0 = std::time::Instant::now();
        let res = client.request(req).instrument(call_span_clone).await;
        let elapsed = t0.elapsed().as_millis() as i64; // tracing
        let rtt_ms = t0.elapsed().as_millis() as u64; // ewma
        ep.in_flight.fetch_sub(1, Ordering::Relaxed);

        let ok = match &res {
            Ok(resp) => {
                let s = resp.status();
                debug!("Response from {}: {}", ep.authority, s);
                s.is_success() || (400u16..500u16).contains(&s.as_u16())
            }
            Err(_) => {
                debug!("Request error to {}", ep.authority);
                false
            }
        };

        if !ok {
            warn!("Request to {} failed", ep.authority);
        }
        cluster.observe(&ep, ok, rtt_ms);

        call_span.record("elapsed_ms", &elapsed);
        Span::current().record("elapsed_ms", &elapsed);
        Ok::<_, error::ProxyError>(res?)
    }.instrument(span_clone).await?;

    let elapsed = t0.elapsed().as_millis() as i64;
    span.record("elapsed_ms", &elapsed);

    Ok(res)
}

// type Result<T> = std::result::Result<T, BoxError>;
type Result<T> = std::result::Result<T, error::ProxyError>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .init();

    // let p = init_tracing().expect("failed to init tracing");

    let config_path = std::env::var("PROXY_CONFIG")
        .unwrap_or_else(|_| "example/proxy_config.toml".to_string());
    let config = config::ProxyConfig::from_file(&config_path).await?;
    debug!("Loaded config: {:?}", config);

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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8333").await?;
    let client: Client<_, BoxBody<Bytes, error::ProxyError>> = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpConnector::new());

    let endpoints: Vec<Arc<Endpoint>> = config.cluster.iter()
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

    let admin_handle = tokio::spawn({
        let admin_listener = tokio::net::TcpListener::bind("0.0.0.0:15000").await?;

        let cluster = cluster.clone();
        run_graceful(admin_listener, async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
            let io = TokioIo::new(stream);
            let svc = tower::service_fn({
                let cluster = cluster.clone();
                move |r: Request<Incoming>| {
                    admin_handler(r, cluster.clone())
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
    });

    let cluster_clone = cluster.clone();
    let client_clone = client.clone();
    let route_timeout = config.route_timeout.clone();
    let ring_clone = ring.clone();
    run_graceful(listener, async move |(stream, _), tasks: &mut JoinSet<()>, graceful: &GracefulShutdown| {
        let stream = match tls_acceptor.accept(stream).await {
            Ok(s) => s,
            Err(e) => {
                warn!("TLS accept error: {}", e);
                return ControlFlow::Continue(());
            }
        };
        let io = TokioIo::new(stream);

        let svc = tower::service_fn({
            let client_clone = client_clone.clone();
            let cluster_clone = cluster_clone.clone();
            let ring_clone = ring_clone.clone();
            move |r: Request<Incoming>| {
                forward(r, client_clone.clone(), cluster_clone.clone(), Some(ring_clone.clone()))
            }
        });
        let svc = ServiceBuilder::new()
            .layer(ConcurrencyLimitLayer::new(512))
            .layer(BufferLayer::new(512))
            .layer(router::PerRouteTimeoutLayer::new(
                    route_timeout.clone().iter()
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

    match try_join!(admin_handle) {
        Ok((Ok(()),)) => { info!("Admin server exited"); },
        Ok((Err(e),)) => warn!("Admin server error: {}", e),
        Err(e) => warn!("Admin server join error: {}", e),
    }

    // shutdown_tracing(p);
    Ok(())
}
