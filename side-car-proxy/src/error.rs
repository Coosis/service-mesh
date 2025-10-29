use opentelemetry_otlp::ExporterBuildError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("IO error: {0}")]
    IOError(#[from] tokio::io::Error),

    // tls
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Certificate open error")]
    CertOpenError,
    #[error("Certificate malformed error")]
    CertMalformedError,

    // reqwest
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),

    // otel
    #[error("OpenTelemetry error: {0}")]
    ExporterBuildError(#[from] ExporterBuildError),

    #[error("Hyper http error: {0}")]
    HyperHttpError(#[from] hyper::http::Error),

    #[error("Hyper error: {0}")]
    HyperError(#[from] hyper::Error),

    #[error("Hyper client error: {0}")]
    LegacyClientError(#[from] hyper_util::client::legacy::Error),

    #[error("Failed to parse config: {0}")]
    ConfigParseError(#[from] toml::de::Error),

    #[error("Some other error: {0}")]
    SomeError(String),

    #[error("No healthy endpoints available")]
    NoHealthyEndpoints,

    #[error("JSON error: {0}")]
    JSONError(#[from] serde_json::Error),
}
