use std::sync::Arc;
use tokio::sync::RwLock;
use side_car_proxy::config::ProxyConfig;
use side_car_proxy::config;

use axum::{
    body::Bytes, extract::State, routing::{get, post}, Router
};

mod error;
type Result<T> = std::result::Result<T, error::Error>;

type SharedState = Arc<RwLock<AppState>>;
struct AppState {
    config: Option<Bytes>,
    config_tx: tokio::sync::broadcast::Sender<Bytes>,
}

#[tokio::main]
async fn main() {
    // read config
    let config_path = std::env::var("CONFIG_PATH").unwrap_or("example/proxy_config.toml".to_string());
    // let config_file = config::ProxyConfig::from_file(&config_path).await;
    let config_file = tokio::fs::read_to_string(&config_path).await;

    let state = Arc::new(RwLock::new(AppState {
        config: Some(Bytes::from(config_file.unwrap_or("".to_string()))),
        config_tx: tokio::sync::broadcast::channel(16).0,
    }));
    let app = Router::new()
        .route("/poll_config", get(poll_config))
        .route("/upload_config", post(upload_config))
        .route("/config", get(config))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:13000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn poll_config(
    State(s): State<SharedState>,
) -> Result<Bytes> {
    let mut rx = {
        let s = s.read().await;
        s.config_tx.subscribe()
    };
    if let Ok(Ok(config)) = tokio::time::timeout(
        std::time::Duration::from_millis(3000),
        rx.recv(),
    ).await
    {
        return Ok(config);
    }
    return Err(error::Error::Timeout);
}

async fn config(
    State(s): State<SharedState>,
) -> Result<Bytes> {
    let s = s.read().await;
    s.config
        .clone()
        .map(|co| Bytes::from(co))
        .ok_or(error::Error::NoConfig)
}

async fn upload_config(
    State(s): State<SharedState>,
    body: Bytes,
) -> Result<()> {
    let mut s = s.write().await;
    // todo: parse and validate
    s.config = Some(body);
    match s.config_tx.send(s.config.clone().unwrap()) {
        Ok(_) => (),
        Err(e) => {
            println!("error broadcasting config: {}", e);
        }
    }
    return Ok(());
}
