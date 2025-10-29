use http::Request;
use http_body_util::{combinators::BoxBody, BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::client::legacy::connect::HttpConnector;
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
use tracing::{info, warn};

use crate::error::ProxyError;

use super::Endpoint;

pub struct Cluster {
    pub endpoints: Vec<Arc<Endpoint>>,
    pub cursor: AtomicUsize,
    /// clock for generating monotonic time, derived from kernel, 
    /// in the case of kernal compromise, this may jump backwards 
    /// thus service is vulnerable to these attacks
    origin: std::time::Instant,
    cooldown_ms: u64,

    client: hyper_util::client::legacy::Client<HttpConnector, BoxBody<Bytes, ProxyError>>,
    consec_fail_threshold: usize,
}

impl Cluster {
    pub fn new(
        client: hyper_util::client::legacy::Client<HttpConnector, BoxBody<Bytes, ProxyError>>,
        endpoints: Vec<Arc<Endpoint>>,
    ) -> Self {
        let c = Self {
            endpoints,
            cursor: AtomicUsize::new(0),
            origin: std::time::Instant::now(),
            cooldown_ms: 20_000,

            consec_fail_threshold: 3,
            client,
        };
        let now_ms = c.now_ms();
        for ep in &c.endpoints {
            ep.breaker.record(now_ms, true);
        }
        c
    }

    #[inline]
    pub fn healthy_endpoints(&self) -> impl Iterator<Item = &Arc<Endpoint>> {
        self.endpoints.iter()
            .filter(move |e| {
                info!("endpoint {} healthy={}", e.authority, e.healthy.load(std::sync::atomic::Ordering::Relaxed));
                e.healthy.load(std::sync::atomic::Ordering::Relaxed)
            })
    }

    pub fn spawn_active_health(self: Arc<Self>) {
        let endpoints = self.endpoints.clone();
        let cluster = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                for ep in endpoints.iter() {
                    let ep = ep.clone();
                    let cluster = cluster.clone();
                    tokio::spawn(async move {
                        let healthy = cluster.check_health(&ep).await;
                        if healthy {
                            info!("Health check success for {}", ep.authority);
                        } else {
                            if !cluster.is_in_cooldown(&ep, cluster.now_ms()) {
                                warn!("Health check failed for {}", ep.authority);
                                cluster.begin_cooldown(&ep);
                            }
                        }
                    });
                }
            }
        });
    }

    async fn check_health(&self, ep: &Arc<Endpoint>) -> bool {
        if self.is_in_cooldown(ep, self.now_ms()) {
            info!("Skipping health check for {} due to cooldown", ep.authority);
            return false;
        }

        let uri = http::Uri::builder()
            .scheme("http")
            .authority(ep.authority.clone())
            .path_and_query("/healthz")
            .build()
            .unwrap();
        let empty = Empty::<Bytes>::new()
            .map_err(|e| ProxyError::SomeError(format!("Empty body error: {}", e)))
            .boxed();
        let req = Request::get(uri)
            .body(empty)
            .unwrap();

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(3), 
            self.client.request(req)
        ).await;

        match res {
            Ok(Ok(resp)) => {
                if !resp.status().is_success() {
                    warn!("Health check non-200 status for {}: {}", ep.authority, resp.status());
                    return false;
                }
                ep.healthy.store(true, Ordering::Relaxed);
                ep.last_eject_ms.store(0, Ordering::Relaxed);
                ep.consec_fail.store(0, Ordering::Relaxed);
                ep.latency.reset();
                true
            }
            _ => {
                warn!("Health check request timeout for {}", ep.authority);
                false
            }
        }
    }

    #[inline]
    pub fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    #[inline]
    pub fn begin_cooldown(&self, ep: &Endpoint) {
        let now = self.now_ms();
        ep.healthy.store(false, Ordering::Relaxed);
        ep.last_eject_ms.store(now, Ordering::Relaxed);
    }

    #[inline]
    fn is_in_cooldown(&self, ep: &Endpoint, now_ms: u64) -> bool {
        let t = ep.last_eject_ms.load(Ordering::Relaxed);
        t != 0 && now_ms.saturating_sub(t) < self.cooldown_ms
    }

    /// marks an endpoint as healthy/unhealthy based on the result of a request
    pub fn observe(&self, ep: &Endpoint, ok: bool, rtt_ms: u64) {
        if ok {
            ep.latency.update(rtt_ms as f64);
            ep.consec_fail.store(0, Ordering::Relaxed);
        } else {
            let fails = ep.consec_fail.fetch_add(1, Ordering::Relaxed) + 1;
            if fails >= self.consec_fail_threshold {
                self.begin_cooldown(ep);
                ep.consec_fail.store(0, Ordering::Relaxed);
            }
        }
    }
}
