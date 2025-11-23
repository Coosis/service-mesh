use std::{pin::Pin, task::Poll};

use http::Request;
use mesh_core::acl::{AccessStrategy, Listener, ACL};
use pin_project_lite::pin_project;
use tower::{BoxError, Layer, Service};

use crate::layer::path::get_access_strategy;

pin_project! {
    pub struct AclFuture<F> { 
        #[pin]
        inner: F,
        access_strategy: AccessStrategy,
    }
}

impl<F, Res, E> Future for AclFuture<F>
where
    F: std::future::Future<Output = Result<Res, E>>,
    E: Into<BoxError>,
{
    type Output = Result<Res, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        if self.access_strategy == AccessStrategy::Deny {
            return Poll::Ready(Err(Box::new(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access Denied"))));
        }
        let mut this = self.project();
        match this.inner.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(res)) => Poll::Ready(Ok(res)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
        }
    }
}


pub struct AclService<S> {
    inner: S,
    acl: ACL,
    listener: Listener,
}

impl<S, B> Service<Request<B>> for AclService<S> 
where S: Service<Request<B>> + Send + 'static,
      S::Response: Send + 'static,
      S::Error: Into<BoxError> + Send + Sync + 'static,
      S::Future: Send + 'static,
      B: Send + 'static {
    type Response = S::Response;

    type Error = BoxError;

    type Future = AclFuture<S::Future>;
    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(r) => Poll::Ready(r.map_err(Into::into)),
        }
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let path = req.uri().path();
        let access_strategy = get_access_strategy(&self.acl, &self.listener, path);
        let fut = self.inner.call(req);
        AclFuture {
            inner: fut,
            access_strategy,
        }
    }
}


pub struct AclLayer {
    acl: ACL,
    listener: Listener,
}

impl AclLayer {
    pub fn new(
        acl: ACL,
        listener: Listener,
    ) -> Self {
        AclLayer { 
            acl,
            listener,
        }
    }
}

impl<S> Layer<S> for AclLayer {
    type Service = AclService<S>;

    fn layer(&self, service: S) -> Self::Service {
        AclService {
            inner: service,
            acl: self.acl.clone(),
            listener: self.listener.clone(),
        }
    }
}
