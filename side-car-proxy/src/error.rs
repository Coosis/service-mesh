use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("Failed to bind to TCP address: {0}")]
    TcpBindError(#[from] tokio::io::Error),

    #[error("Hyper http error: {0}")]
    HyperHttpError(#[from] hyper::http::Error),

    #[error("Hyper error: {0}")]
    HyperError(#[from] hyper::Error),

    #[error("Hyper client error: {0}")]
    LegacyClientError(#[from] hyper_util::client::legacy::Error),
}
