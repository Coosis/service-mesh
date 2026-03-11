use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use tracing::{info, warn};
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{filter::Targets, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::instance::start_instance;
use crate::resolver::Resolver;
use crate::tel::otel::{
    self, init_tracer_exporter, init_tracer_provider, init_tracing_and_propagation,
};
type MtlsClient =
    Client<HttpsConnector<HttpConnector<Resolver>>, BoxBody<Bytes, crate::error::ProxyError>>;
type HttpClient = Client<HttpConnector, BoxBody<Bytes, crate::error::ProxyError>>;
// type Result<T> = std::result::Result<T, BoxError>;
type Result<T> = std::result::Result<T, error::ProxyError>;

mod admin;
mod circuit_breaker;
mod config;
mod error;
mod forwarder;
mod graceful;
mod hash;
mod instance;
mod layer;
mod load_balance;
mod registry;
mod resolver;
mod tel;
mod tls;
mod util;

#[tokio::main]
async fn main() -> Result<()> {
    let targets = Targets::new()
        .with_target("config_poll", tracing::Level::ERROR)
        .with_target("health_check", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_span_events(FmtSpan::CLOSE)
                .with_target(true)
                .with_filter(targets),
        )
        .init();

    init_tracer_provider();
    init_tracing_and_propagation();
    init_tracer_exporter().expect("failed to init tracer exporter");

    let svc_name = std::env::var("SERVICE_NAME").unwrap_or_else(|_| {
        warn!("SERVICE_NAME not set, using 'service-a' as default");
        "service-a".to_string()
    });
    let config_txt = reqwest::get(format!("http://127.0.0.1:13000/config/{}", svc_name)).await?;
    let cfg = config::from_content(&config_txt.text().await.unwrap())?;

    let client: HttpClient =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(HttpConnector::new());

    let mut instance = start_instance(cfg.clone(), client.clone()).or_else(|e| {
        warn!("Failed to start initial instance: {}", e);
        Err(e)
    })?;

    // config long-polling
    let (tx, mut rx) = tokio::sync::watch::channel(cfg.clone());
    tokio::spawn(async move {
        loop {
            let response = reqwest::get(format!(
                "http://127.0.0.1:13000/poll_config/{}",
                svc_name.clone()
            ))
            .await;
            match response {
                Ok(resp) => {
                    // info!("received new config from admin server: {:?}", resp);
                    let should_read = match resp.status().is_success() {
                        true => true,
                        false => {
                            if resp.status() != reqwest::StatusCode::REQUEST_TIMEOUT {
                                warn!(target: "config_poll", "Config poll returned non-success, non-timeout status: {}", resp.status());
                            }
                            false
                        }
                    };
                    if !should_read {
                        continue;
                    }
                    let txt = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            warn!(target: "config_poll", "Failed to read config text: {}", e);
                            continue;
                        }
                    };
                    let config = match config::from_content(&txt) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(target: "config_poll", "Failed to parse config: {}", e);
                            continue;
                        }
                    };
                    tx.send(config).unwrap();
                }
                Err(e) => {
                    tracing::error!(target: "config_poll", "Config poll error: {}", e);
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Main Received Ctrl-C, shutting down");
                instance.request_shutdown();
                instance.join_with_deadline(std::time::Duration::from_secs(15)).await;
                break;
            }
            Ok(_) = rx.changed() => {
                let new_config = rx.borrow().clone();
                info!("Applying new config: {:?}", new_config);

                instance.request_shutdown();
                instance.join_with_deadline(std::time::Duration::from_secs(15)).await;
                instance = start_instance(
                    cfg.clone(),
                    client.clone(),
                )?;

            }
        }
    }

    // shutdown_tracing(p);
    Ok(())
}
