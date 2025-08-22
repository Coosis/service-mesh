use http::uri::PathAndQuery;
use http_body_util::Limited;
use hyper::header::{CONNECTION, TE, TRANSFER_ENCODING, UPGRADE, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};
use hyper::{header::{HeaderValue, HOST}, service::Service};
use tower::{buffer::BufferLayer, limit::{ConcurrencyLimitLayer, GlobalConcurrencyLimitLayer, RateLimitLayer}, retry::RetryLayer, timeout::TimeoutLayer, Layer, ServiceBuilder};
use hyper::{body::Incoming, server::conn::http1, Request, Response};
use hyper_util::{client::legacy::{connect::HttpConnector, Client}, rt::TokioIo};
use tracing::debug;
use hyper_util::service::TowerToHyperService;

mod error;

fn strip_hop_by_hop(headers: &mut hyper::HeaderMap) {
    let conns = headers.get_all(CONNECTION).iter()
        .filter_map(|hv| hv.to_str().ok())
        .flat_map(|s| s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()))
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();

    for h in [CONNECTION, TE, TRANSFER_ENCODING, UPGRADE, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION] {
        headers.remove(h);
    }

    for name in conns {
        if let Ok(hn) = hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            headers.remove(hn);
        }
    }
}

async fn forward(
    r: Request<Incoming>,
    client: Client<HttpConnector, Limited<Incoming>>
) -> Result<Response<hyper::body::Incoming>> {
    debug!("Received request: {}", r.uri());
    debug!("Re-routing...");

    let (mut parts, body) = r.into_parts();

    // per RFC 9110, drop specific headers
    strip_hop_by_hop(&mut parts.headers);

    // re-write url for client connector
    let uri = http::Uri::builder()
        .scheme("http")
        .authority("localhost:8314")
        .path_and_query(parts.uri.path_and_query().map_or(PathAndQuery::from_static("/"), |pq| pq.to_owned()))
        .build()?;
    parts.uri = uri;

    parts.headers.insert(
        HOST,
        HeaderValue::from_static("localhost:8314"),
    );

    let limit = Limited::new(body, 2 * 1024 * 1024);

    let req = Request::from_parts(parts, limit);

    let res = client.request(req).await?;

    Ok(res)
}

type Result<T> = std::result::Result<T, error::ProxyError>;
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8333").await?;
    let client: Client<_, _> = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpConnector::new());

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        let client_clone = client.clone();
        tokio::spawn(async move {
            let svc = tower::service_fn(move |r: Request<Incoming>| {
                forward(r, client_clone.clone())
            });
            let svc = ServiceBuilder::new()
                .layer(ConcurrencyLimitLayer::new(512))
                .layer(TimeoutLayer::new(std::time::Duration::from_secs(10)))
                .layer(BufferLayer::new(512))
                .service(svc);
            let svc = TowerToHyperService::new(svc);
            if let Err(err) = http1::Builder::new().serve_connection(io, svc).await {
                eprintln!("server error: {}", err);
            }
        });
    }
}
