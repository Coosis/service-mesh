use std::{convert::Infallible, sync::Arc};

use http::{Request, Response};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};

use crate::load_balance::{Cluster, Endpoint};

pub async fn admin_handler(
    req: Request<Incoming>,
    cluster: Arc<Cluster>,
) -> Result<http::Response<Full<Bytes>>, Infallible> {
    let p = req.uri().path();
    match p {
        "/ready" => {
            let endpoints = cluster.endpoints.clone();
            let ok = cluster.healthy_endpoints().count() > 0;
            let status = if ok { http::StatusCode::OK } else { http::StatusCode::SERVICE_UNAVAILABLE };
            let res = endpoints.iter()
                .map(|e| {
                    let health = if e.healthy.load(std::sync::atomic::Ordering::Relaxed) {
                        "healthy"
                    } else {
                        "unhealthy"
                    };
                    format!("{} - {}", e.authority, health)
                })
                .collect::<Vec<_>>().join("\n");
            let body = if ok { format!("OK: \n{}", res) } else { "No healthy endpoints".to_owned() };
            let resp = http::Response::builder()
                .status(status)
                .body(Full::from(Bytes::from(body))).unwrap();
            Ok(resp)
        }
        _ => Ok(Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(Full::from(Bytes::from("Not Found")))
            .unwrap()),
    }
}
