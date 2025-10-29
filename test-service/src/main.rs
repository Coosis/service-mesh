use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::{http::StatusCode, routing::get, Router};
use futures_util::stream;
use hyper::header;
use hyper::body::Bytes;
use axum_extra::extract::CookieJar;
use tracing::info;
use tokio::sync::Mutex;

struct AppState {
    on: bool,
    port: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = std::env::args().collect::<Vec<String>>();
    assert!(args.len() == 2, "Usage: {} <port>", args[0]);
    let port = &args[1];

    let state = Arc::new(Mutex::new(AppState { on: true, port: port.clone() }));
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
        .route("/healthz", get(healthz))
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
        .route("/bad", get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Bad") }))
        .route("/flip", get(|State(state): State<Arc<Mutex<AppState>>>| async move {
            let mut state = state.lock().await;
            state.on = !state.on;
            if state.on {
                (StatusCode::OK, "Flipped to ON")
            } else {
                (StatusCode::OK, "Flipped to OFF")
            }
        }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    info!("Listening on {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn healthz(State(state): State<Arc<Mutex<AppState>>>) -> (StatusCode, &'static str) {
    let state = state.lock().await;
    if !state.on {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Unhealthy");
    }
    if state.port == "8314" || state.port == "8315" {
        (StatusCode::OK, "Healthy")
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Unhealthy")
    }
}
