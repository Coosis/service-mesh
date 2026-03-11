use super::{Cluster, Endpoint};
use std::sync::Arc;

impl Cluster {
    pub fn pick_round_robin(&self) -> Option<Arc<Endpoint>> {
        let healthy = self.healthy_endpoints().collect::<Vec<_>>();
        if healthy.is_empty() {
            return None;
        }

        let idx = self
            .cursor
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(healthy[idx % healthy.len()].clone())
    }
}
