use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
use super::Endpoint;

pub struct Cluster {
    pub endpoints: Vec<Arc<Endpoint>>,
    pub cursor: AtomicUsize,
    /// clock for generating monotonic time, derived from kernel, 
    /// in the case of kernal compromise, this may jump backwards 
    /// thus service is vulnerable to these attacks
    origin: std::time::Instant,
    cooldown_ms: u64,

    consec_fail_threshold: usize,
}

impl Cluster {
    pub fn new(endpoints: Vec<Arc<Endpoint>>) -> Self {
        Self { 
            endpoints,
            cursor: AtomicUsize::new(0),
            origin: std::time::Instant::now(),
            cooldown_ms: 10_000,

            consec_fail_threshold: 3,
        }
    }

    #[inline]
    pub fn healthy_endpoints(&self) -> impl Iterator<Item = &Arc<Endpoint>> {
        self.endpoints.iter()
            .filter(move |e| {
                self.end_cooldown_if_elapsed(e);
                e.healthy.load(std::sync::atomic::Ordering::Relaxed)
            })
    }

    #[inline]
    fn now_ms(&self) -> u64 {
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

    // If cooldown elapsed, allow routing again.
    #[inline]
    fn end_cooldown_if_elapsed(&self, ep: &Endpoint) {
        let now_ms = self.now_ms();
        if !ep.healthy.load(Ordering::Relaxed) && !self.is_in_cooldown(ep, now_ms) {
            ep.healthy.store(true, Ordering::Relaxed);
        }
    }

    /// marks an endpoint as healthy/unhealthy based on the result of a request
    pub fn observe(&self, ep: &Endpoint, ok: bool, rtt_ms: u64) {
        ep.latency.update(rtt_ms as f64);

        if ok {
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
