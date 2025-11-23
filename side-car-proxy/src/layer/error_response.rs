use http::Request;
use http::{Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use opentelemetry_http::Bytes;
use pin_project_lite::pin_project;
use std::io;
use std::{convert::Infallible, future::Future, pin::Pin, task::Poll};
use tower::{BoxError, Layer, Service};

pin_project! {
    pub struct ErrorToHttpFuture<F, E> {
        #[pin]
        inner: F,
        _marker: std::marker::PhantomData<E>,
    }
}

impl<F, E, SE> Future for ErrorToHttpFuture<F, E>
where
    F: Future<Output = Result<Response<BoxBody<Bytes, E>>, SE>>,
    SE: Into<BoxError>,
{
    type Output = Result<Response<BoxBody<Bytes, E>>, Infallible>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        match this.inner.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(resp)) => Poll::Ready(Ok(resp)),
            Poll::Ready(Err(err)) => {
                let resp = map_error_to_response::<E>(err.into());
                Poll::Ready(Ok(resp))
            }
        }
    }
}

fn map_error_to_response<E>(err: BoxError) -> Response<BoxBody<Bytes, E>> {
    if let Some(io_err) = err.downcast_ref::<io::Error>() {
        if io_err.kind() == io::ErrorKind::PermissionDenied {
            return simple_body::<E>(StatusCode::FORBIDDEN, "Access Denied");
        }
    }
    simple_body::<E>(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
}

fn simple_body<E>(status: StatusCode, msg: &'static str) -> Response<BoxBody<Bytes, E>> {
    let body: BoxBody<Bytes, E> = Full::new(Bytes::from_static(msg.as_bytes()))
        .map_err(|never| -> E { match never {} })
        .boxed();

    Response::builder()
        .status(status)
        .body(body)
        .expect("build error response")
}

pub struct ErrorToHttp<S> {
    inner: S,
}

impl<S: Clone> Clone for ErrorToHttp<S> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<S> ErrorToHttp<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, B, E, SE> Service<Request<B>> for ErrorToHttp<S>
where
    S: Service<Request<B>, Response = Response<BoxBody<Bytes, E>>, Error = SE> + Clone + Send + 'static,
    S::Future: Send + 'static,
    SE: Into<BoxError> + Send + 'static,
    E: Send + 'static,
    B: Send + 'static,
{
    type Response = Response<BoxBody<Bytes, E>>;
    type Error = Infallible;
    type Future = ErrorToHttpFuture<S::Future, E>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Ok(())),
        }
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        ErrorToHttpFuture { inner: self.inner.call(req), _marker: std::marker::PhantomData }
    }
}

pub struct ErrorToHttpLayer;
impl ErrorToHttpLayer { pub fn new() -> Self { Self } }

impl<S> Layer<S> for ErrorToHttpLayer {
    type Service = ErrorToHttp<S>;
    fn layer(&self, service: S) -> Self::Service { ErrorToHttp::new(service) }
}
