use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::routing::any;
use axum::{Router, http::StatusCode, routing::get};
use axum_extra::extract::CookieJar;
use futures_util::stream;
use hyper::body::Bytes;
use hyper::header;
use tokio::sync::Mutex;
use tracing::{debug, info};

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

    let letthrough: Vec<String> = vec!["8314".into(), "8315".into(), "8317".into()];

    let state = Arc::new(Mutex::new(AppState {
        on: true,
        port: port.clone(),
    }));
    let app = Router::new()
        .route(
            "/",
            get({
                let port = port.clone();
                async move |headers: axum::http::HeaderMap, jar: CookieJar| {
                    info!("Headers: {:?}", headers);
                    if letthrough.contains(&port) {
                        if jar.get("x-client-id").is_some() {
                            debug!("x-client-id found!");
                            return (StatusCode::OK, jar, format!("Hello from port {port}\n"));
                        }
                        debug!("x-client-id not found, setting...");
                        let uuid = uuid::Uuid::new_v4().to_string();
                        let jar = jar.add(axum_extra::extract::cookie::Cookie::new(
                            "x-client-id",
                            uuid,
                        ));
                        (StatusCode::OK, jar, format!("Hello from port {port}\n"))
                    } else {
                        debug!("This port does not let through");
                        let jar = jar.add(axum_extra::extract::cookie::Cookie::new(
                            "x-client-id",
                            "hihi",
                        ));
                        (StatusCode::INTERNAL_SERVER_ERROR, jar, format!("Error!"))
                    }
                }
            }),
        )
        .route(
            "/internal/ok",
            any(|| async { (StatusCode::OK, "Internal Endpoint") }),
        )
        .route("/healthz", get(healthz))
        .route(
            "/sleep",
            get(|| async {
                let s = stream::once(async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok::<Bytes, Infallible>(Bytes::from_static(b"hello after 5s\n"))
                });

                let body = Body::from_stream(s);

                (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain")], body)
            }),
        )
        .route(
            "/bad",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Bad") }),
        )
        .route(
            "/flip",
            get(|State(state): State<Arc<Mutex<AppState>>>| async move {
                let mut state = state.lock().await;
                state.on = !state.on;
                if state.on {
                    (StatusCode::OK, "Flipped to ON")
                } else {
                    (StatusCode::OK, "Flipped to OFF")
                }
            }),
        )
        .route(
            "/call/{cluster}/{domain}/{path}",
            get(
                |Path((cluster, domain, path)): Path<(String, String, String)>| async move {
                    info!("Calling domain: {} from cluster: {}", domain, cluster);
                    let client = reqwest::ClientBuilder::new()
                        .danger_accept_invalid_certs(true)
                        .build()
                        .unwrap();
                    let res = client
                        .get(format!("http://{}/{}", cluster, path))
                        .body("")
                        .header("HOST", &domain)
                        .send()
                        .await;
                    println!("Response from cluster {}: {:?}", cluster, res);
                    match res {
                        Ok(r) => (
                            StatusCode::OK,
                            r.text()
                                .await
                                .unwrap_or_else(|_| "Failed to read response".to_string()),
                        ),
                        Err(e) => {
                            println!("Error calling cluster {}: {}", cluster, e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Error calling domain: {}", domain),
                            );
                        }
                    }
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    info!("Listening on {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn healthz(State(state): State<Arc<Mutex<AppState>>>) -> (StatusCode, &'static str) {
    let state = state.lock().await;
    if !state.on {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Unhealthy");
    }
    if state.port == "8314" || state.port == "8317" {
        (StatusCode::OK, "Healthy")
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Unhealthy")
    }
}
