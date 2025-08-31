use std::error::Error;
use std::pin::pin;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use http::uri::{Authority, PathAndQuery};
use http_body_util::Limited;
use hyper::header::{HeaderValue, HOST};
use hyper::{body::Incoming, server::conn::http1, Request, Response};
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use hyper_util::{client::legacy::{connect::HttpConnector, Client}, rt::TokioIo};
use load_balance::{Cluster, Endpoint};
use tokio::task::JoinSet;
use tower::{buffer::BufferLayer, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer, ServiceBuilder};
use tower::BoxError;
use tracing::{debug, info_span, warn, Instrument, Span};
use tracing_subscriber::fmt::format::FmtSpan;
use w3c::ensure_w3c;

mod util;
mod error;
mod config;
mod router;
mod w3c;
mod load_balance;

async fn forward(
    r: Request<Incoming>,
    client: Client<HttpConnector, Limited<Incoming>>,
    cluster: Arc<Cluster>,
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

        // ! uncommnet to use round robin instead!
        // let ep = match cluster.pick_round_robin() {
        let ep = match cluster.pick_p2c() {
            Some(ep) => ep,
            None => {
                warn!("No healthy endpoints available");
                return Err(error::ProxyError::NoHealthyEndpoints);
            }
        };
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

        let limit = Limited::new(body, 2 * 1024 * 1024);
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
                s.is_success() || (400..500).contains(&s.as_u16())
            }
            Err(_) => false,
        };

        cluster.observe(&ep, ok, rtt_ms);

        call_span.record("elapsed_ms", &elapsed);
        Span::current().record("elapsed_ms", &elapsed);
        Ok::<_, error::ProxyError>(res?)
    }.instrument(span_clone).await?;

    let elapsed = t0.elapsed().as_millis() as i64;
    span.record("elapsed_ms", &elapsed);

    Ok(res)
}

type Result<T> = std::result::Result<T, BoxError>;
// type Result<T> = std::result::Result<T, error::ProxyError>;

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

    let endpoints: Vec<Arc<Endpoint>> = config.cluster.iter()
        .flat_map(|(_, addrs)| addrs.iter())
        .map(|authority| Arc::new(Endpoint::new(Authority::from_str(authority).expect("valid authority"))))
        .collect();
    let cluster = Arc::new(Cluster::new(endpoints));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8333").await?;
    let client: Client<_, _> = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpConnector::new());

    let graceful = GracefulShutdown::new();
    let mut sig_int = pin!(tokio::signal::ctrl_c());

    let mut tasks = JoinSet::new();

    loop {
        let cluster = cluster.clone();
        tokio::select! {
            res = listener.accept() => {
                let (stream, _) = res?;
                let io = TokioIo::new(stream);

                let client_clone = client.clone();
                let route_timeout = config.route_timeout.clone();
                let svc = tower::service_fn(move |r: Request<Incoming>| {
                    forward(r, client_clone.clone(), cluster.clone())
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
            }

            _ = &mut sig_int => {
                debug!("Received SIGINT, shutting down...");
                break;
            }
        }
    }
    drop(listener);

    let deadline = std::time::Duration::from_secs(10);
    let tasks_drained = tokio::time::timeout(deadline, async {
        tokio::join!(
            graceful.shutdown(),
            async {
                while let Some(res) = tasks.join_next().await {
                    if let Err(err) = res {
                        debug!("Task failed: {}", err);
                    } else {
                        // debug!("Task completed");
                    }
                }
            }
        );
    }).await.is_ok();

    if tasks_drained {
        debug!("Graceful shutdown complete");
    } else {
        debug!("Timed out");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }

    // shutdown_tracing(p);
    Ok(())
}
