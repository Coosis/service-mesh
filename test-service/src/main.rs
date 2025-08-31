use axum::{
    handler::Handler, http::{self, StatusCode}, routing::{get, post}, Json, Router
};
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
            move || async move { 
                if port == "8314" {
                    (StatusCode::OK, format!("Hello from port {port}\n"))
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Error!"))
                }
            }
        }));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    info!("Listening on {}", port);
    axum::serve(listener, app).await.unwrap();
}
