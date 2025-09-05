use axum::{http::StatusCode, routing::get, Router};
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
            move |jar: CookieJar| async move { 
                if port == "8314" {
                    if jar.get("x-client-id").is_some() {
                        return (StatusCode::OK, jar, format!("Hello from port {port}\n"));
                    }
                    let uuid = uuid::Uuid::new_v4().to_string();
                    let jar = jar.add(axum_extra::extract::cookie::Cookie::new("x-client-id", uuid));
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
        }));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    info!("Listening on {}", port);
    axum::serve(listener, app).await.unwrap();
}
