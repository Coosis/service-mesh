use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body;
use axum::{http::StatusCode, routing::get, Router};
use futures_util::stream;
use hyper::header;
use hyper::body::Bytes;
use axum_extra::extract::CookieJar;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = std::env::args().collect::<Vec<String>>();
    assert!(args.len() == 2, "Usage: {} <port>", args[0]);
    let port = &args[1];

    let app = Router::new()
        .route("/", get({
            let port = port.clone();
            async move |headers: axum::http::HeaderMap, jar: CookieJar| {
                info!("Headers: {:?}", headers);
                if port == "8314" {
                    if jar.get("x-client-id").is_some() {
                        return (StatusCode::OK, jar, format!("Hello from port {port}\n"));
                    }
                    let uuid = uuid::Uuid::new_v4().to_string();
                    let jar = jar.add(axum_extra::extract::cookie::Cookie::new("x-client-id", uuid));
                    // debug!("{:?}", )
                    (StatusCode::OK, jar, format!("Hello from port {port}\n"))
                } else if port == "8315" {
                    if jar.get("x-client-id").is_some() {
                        return (StatusCode::OK, jar, format!("Hello from port {port}\n"));
                    }
                    let uuid = uuid::Uuid::new_v4().to_string();
                    let jar = jar.add(axum_extra::extract::cookie::Cookie::new("x-client-id", uuid));
                    (StatusCode::OK, jar, format!("Hello from port {port}\n"))
                } else {
                    let jar = jar.add(axum_extra::extract::cookie::Cookie::new("x-client-id", "hihi"));
                    (StatusCode::INTERNAL_SERVER_ERROR, jar, format!("Error!"))
                }
            }
        }))
        .route("/healthz", get({
            let port = port.clone();
            || async move {
                if port == "8314" || port == "8315" {
                    (StatusCode::OK, "Healthy")
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Unhealthy")
                }
            }
        }))
        .route("/sleep", get(|| async {
            let s = stream::once(async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok::<Bytes, Infallible>(Bytes::from_static(b"hello after 5s\n"))
            });

            let body = Body::from_stream(s);

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain")],
                body,
            )
        }))
        .route("/bad", get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Bad") }));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    info!("Listening on {}", port);
    axum::serve(listener, app).await.unwrap();
}
