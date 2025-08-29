use std::{pin::Pin, task::Poll};
use std::time::Duration;

use http::Request;
use pin_project_lite::pin_project;
use tower::{BoxError, Layer, Service};

#[derive(Debug, Clone)]
enum PathFragment {
    String(String),
    Wildcard,
}
type PathFragments = Vec<PathFragment>;
fn fragments_from_str(v: &str) -> PathFragments {
    v.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "*" {
                PathFragment::Wildcard
            } else {
                PathFragment::String(s.to_string())
            }
        })
        .collect()
}
fn get_timeout(rules: &[(PathFragments, Duration)], path: &str, default: Duration) -> Duration {
    let fragments = fragments_from_str(path);
    let d = rules.iter()
        .filter(|(rule, _)| rule.len() == fragments.len())
        .find(|(rule, _)| {
            rule.iter().zip(&fragments).all(|(rule_frag, path_frag)| {
                match (rule_frag, path_frag) {
                    (PathFragment::Wildcard, _) => true,
                    (PathFragment::String(r), PathFragment::String(p)) => r.eq(p),
                    _ => false,
                }
            })
        })
    .map_or(default, |(_, dur)| *dur);
    d
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
        let timeout = get_timeout(&self.rules, path, self.default);
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
