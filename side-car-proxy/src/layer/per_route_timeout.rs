use std::fmt::Display;
use std::{pin::Pin, task::Poll};
use std::time::Duration;

use http::Request;
use hyper::body::Body;
use pin_project_lite::pin_project;
use tower::{BoxError, Layer, Service};

use crate::layer::path::{PathFragments, fragments_from_str, get_timeout};

#[derive(Debug)]
pub enum TimeoutError<E> {
    Inner(E),
    Timeout,
}

impl<E> Display for TimeoutError<E>
where E: Display {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeoutError::Inner(e) => write!(f, "Inner error: {}", e),
            TimeoutError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl<E> From<E> for TimeoutError<E> {
    fn from(e: E) -> Self {
        TimeoutError::Inner(e)
    }
}

pin_project! {
    pub struct TimeoutBody<B> {
        #[pin]
        inner: B,
        #[pin]
        sleep: tokio::time::Sleep,
        duration: tokio::time::Duration,
    }
}

impl<B> TimeoutBody<B> {
    pub fn new(
        inner: B,
        duration: std::time::Duration,
    ) -> Self {
        TimeoutBody { 
            inner,
            sleep: tokio::time::sleep(duration),
            duration: tokio::time::Duration::from(duration),
        }
    }
}

impl<B> Body for TimeoutBody<B> 
where B: Body {
    type Data = B::Data;

    type Error = TimeoutError<B::Error>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let mut proj = self.project();
        if proj.sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(TimeoutError::Timeout)));
        }
        match proj.inner.poll_frame(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e.into()))),
            Poll::Ready(Some(Ok(frame))) => {
                let deadline = tokio::time::Instant::now() + proj.duration.clone();
                proj.sleep.as_mut().reset(deadline);
                return Poll::Ready(Some(Ok(frame)));
            }
        }
    }
}

pin_project! {
    pub struct PerRouteFuture<F> {
        #[pin]
        inner: F,
        #[pin]
        sleep: tokio::time::Sleep,
    }
}

impl<F, Res, E> Future for PerRouteFuture<F>
where
    F: std::future::Future<Output = Result<Res, E>>,
    E: Into<BoxError>,
{
    type Output = Result<Res, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if this.sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Box::new(std::io::Error::new(std::io::ErrorKind::TimedOut, "request timed out"))));
        }

        match this.inner.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(res)) => Poll::Ready(Ok(res)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerRouteTimeout<S> {
    rules: Vec<(PathFragments, Duration)>,
    inner: S,
    default: Duration,
}

impl<S, B> Service<Request<B>> for PerRouteTimeout<S> 
where S: Service<Request<B>> + Send + 'static,
      S::Response: Send + 'static,
      S::Error: Into<BoxError> + Send + Sync + 'static,
      S::Future: Send + 'static,
      B: Send + 'static {
    type Response = S::Response;

    type Error = BoxError;

    type Future = PerRouteFuture<S::Future>;
    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(r) => Poll::Ready(r.map_err(Into::into)),
        }

    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let path = req.uri().path();
        let timeout = get_timeout((&self.rules).iter(), path, self.default);
        let fut = self.inner.call(req);
        let sleep = tokio::time::sleep(timeout);
        PerRouteFuture {
            inner: fut,
            sleep,
        }
    }
}

pub struct PerRouteTimeoutLayer {
    rules: Vec<(PathFragments, Duration)>,
    default: Duration,
}

impl PerRouteTimeoutLayer {
    pub fn new(rules: Vec<(String, Duration)>, default: Duration) -> Self {
        let rules = rules.into_iter()
            .map(|(s, d)| (fragments_from_str(&s), d))
            .collect();
        PerRouteTimeoutLayer {
            rules,
            default,
        }
    }
}

impl<S> Layer<S> for PerRouteTimeoutLayer {
    type Service = PerRouteTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PerRouteTimeout {
            rules: self.rules.clone(),
            inner,
            default: self.default,
        }
    }
}
