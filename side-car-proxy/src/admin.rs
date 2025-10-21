use hyper::StatusCode;
use std::{convert::Infallible, sync::{atomic::Ordering, Arc}};

use http::{Request, Response};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Bytes, Incoming};

use crate::{circuit_breaker::BreakerState, config::ProxyConfig, load_balance::Cluster};

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

pub async fn admin_handler(
    req: Request<Incoming>,
    cluster: Arc<Cluster>,
    config: ProxyConfig,
) -> Result<http::Response<BoxBody<Bytes, crate::error::ProxyError>>, Infallible> {
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
            let body: BoxBody<Bytes, crate::error::ProxyError> = Full::from(Bytes::from(body))
                .map_err(|e| crate::error::ProxyError::SomeError(e.to_string()))
                .boxed();
            let resp = http::Response::builder()
                .status(status)
                .body(body)
                .unwrap();
                // .map_err(|e| crate::error::ProxyError::SomeError(e.to_string()));
            Ok(resp)
        }
        "/config" => {
            let json = config.to_json().unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
            let body = Full::new(Bytes::from(json))
                .map_err(|e| crate::error::ProxyError::SomeError(e.to_string()))
                .boxed();
            return Ok(Response::builder()
                .header("content-type", "application/json")
                .status(StatusCode::OK)
                .body(body)
                .unwrap());
        }
        "/metrics" => {
            let mut buf = String::new();
            for (i, ep) in cluster.endpoints.iter().enumerate() {
                let authority_escaped = escape(&ep.authority.to_string());
                let healthy = ep.healthy.load(Ordering::Relaxed);
                let inflight = ep.in_flight.load(Ordering::Relaxed);
                let br = match ep.breaker.load_state() {
                    BreakerState::Closed => "closed",
                    BreakerState::Open => "open",
                    BreakerState::HalfOpen => "half_open",
                };
                buf.push_str(&format!(
                    "proxy_endpoint_healthy{{idx=\"{}\",authority=\"{}\"}} {}\n",
                    i, authority_escaped, if healthy {1} else {0}
                ));
                buf.push_str(&format!(
                    "proxy_endpoint_in_flight{{idx=\"{}\",authority=\"{}\"}} {}\n",
                    i, authority_escaped, inflight
                ));
                buf.push_str(&format!(
                    "proxy_endpoint_breaker_state{{idx=\"{}\",authority=\"{}\"}} {}\n",
                    i, authority_escaped, br
                ));
                let rtt_secs = if ep.latency.initialized() {
                    ep.latency.get() / 1000.0
                } else { continue; };
                buf.push_str(&format!(
                    "proxy_endpoint_rtt_ewma_secs{{idx=\"{}\",authority=\"{}\"}} {}\n",
                    i, authority_escaped, rtt_secs
                ));
            }
            let body = Full::new(Bytes::from(buf))
                .map_err(|e| crate::error::ProxyError::SomeError(e.to_string()))
                .boxed();
            return Ok(Response::builder()
                .header("content-type", "text/plain; version=0.0.4")
                .status(StatusCode::OK)
                .body(body)
                .unwrap());
        }
        _ => Ok(Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(Full::from(Bytes::from("Not Found")).map_err(|e| crate::error::ProxyError::SomeError(e.to_string())).boxed())
            .unwrap()),
    }
}
