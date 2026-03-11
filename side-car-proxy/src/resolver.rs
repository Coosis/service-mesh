use crate::registry::Registry;
use crate::{error::ProxyError, registry::EtcdRegistry};
use hyper_util::client::legacy::connect::dns::{GaiAddrs, GaiFuture, GaiResolver, Name};
use pin_project_lite::pin_project;
use std::sync::Arc;
use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};
use tower::Service;

pub struct ResolverAddr(Vec<SocketAddr>);
impl Iterator for ResolverAddr {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop()
    }
}
impl From<Vec<SocketAddr>> for ResolverAddr {
    fn from(addrs: Vec<SocketAddr>) -> Self {
        ResolverAddr(addrs)
    }
}

pin_project! {
    pub struct ResolverFuture<T, R>
    where R: Registry {
        #[pin]
        inner: T,
        name: String,
        dev_map: HashMap<String, SocketAddr>,
        registry: Option<Arc<R>>,
    }
}

impl<R> Future for ResolverFuture<GaiFuture, R>
where
    R: Registry,
{
    type Output = std::result::Result<ResolverAddr, ProxyError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(addr) = self.dev_map.get(&self.name) {
            Poll::Ready(Ok(vec![*addr].into()))
        } else {
            if let Some(r) = &self.registry {
                if let Some(v) = r.discover(&self.name) {
                    let addrs = v
                        .into_iter()
                        .filter_map(|s| s.parse().ok())
                        .collect::<Vec<SocketAddr>>();
                    if !addrs.is_empty() {
                        return Poll::Ready(Ok(addrs.into()));
                    }
                }
            }
            let projected = self.project();
            projected
                .inner
                .poll(cx)
                .map(|res| res.map_err(|e| e.into()))
                .map(|addrs| {
                    addrs.map(|gai_addrs: GaiAddrs| gai_addrs.collect::<Vec<SocketAddr>>().into())
                })
        }
    }
}

#[derive(Clone)]
pub struct Resolver {
    gai: GaiResolver,
    dev_map: HashMap<String, SocketAddr>,
    r: Option<Arc<EtcdRegistry>>,
}

impl Resolver {
    pub fn new(r: Option<Arc<EtcdRegistry>>) -> Self {
        Resolver {
            gai: GaiResolver::new(),
            dev_map: HashMap::new(),
            r,
        }
    }
}

impl Service<Name> for Resolver {
    type Response = ResolverAddr;

    type Error = crate::error::ProxyError;

    type Future = ResolverFuture<GaiFuture, EtcdRegistry>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        ResolverFuture {
            inner: self.gai.call(name.clone()),
            name: name.as_str().to_string(),
            dev_map: self.dev_map.clone(),
            registry: self.r.clone(),
        }
    }
}
