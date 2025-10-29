use axum::response::IntoResponse;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Timeout error")]
    Timeout,

    #[error("No configuration available")]
    NoConfig,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        axum::response::Response::builder()
            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(format!("{}", self).into())
            .unwrap()
    }
}
