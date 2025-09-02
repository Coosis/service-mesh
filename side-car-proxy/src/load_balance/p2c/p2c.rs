use std::sync::{atomic::Ordering, Arc};
use rand::{rng, seq::index::sample};
use tracing::debug;
use crate::load_balance::{Cluster, Endpoint};

impl Cluster {
    /// peak-of-two choices load balancing
    pub fn pick_p2c(&self) -> Option<Arc<Endpoint>> {
        let healthy = self.healthy_endpoints().collect::<Vec<_>>();
        let len = healthy.len();
        if len == 0 {
            return None;
        } else if len == 1 {
            return Some(healthy[0].clone());
        }

        let mut rng = rng();
        let idx = sample(&mut rng, len, 2);
        let e1 = healthy[idx.index(0)];
        let e2 = healthy[idx.index(1)];
        debug!("p2c candidates: {} (in_flight={} latency={}), {} (in_flight={} latency={})", 
            e1.authority, e1.in_flight.load(Ordering::Relaxed), e1.latency.get(),
            e2.authority, e2.in_flight.load(Ordering::Relaxed), e2.latency.get(),
        );
        
        let fa = e1.in_flight.load(Ordering::Relaxed);
        let fb = e2.in_flight.load(Ordering::Relaxed);
        if fa != fb {
            if fa < fb {
                return Some(e1.clone());
            } else {
                return Some(e2.clone());
            }
        }

        debug!("p2c tie break on latency: {} (latency={}), {} (latency={})", 
            e1.authority, e1.latency.get(),
            e2.authority, e2.latency.get(),
        );

        // tie break
        let latency_a = e1.latency.get();
        let latency_b = e2.latency.get();
        let a_ok = latency_a.is_finite();
        let b_ok = latency_b.is_finite();
        
        match (a_ok, b_ok) {
            (true, true) => {
                if e1.latency.get() <= e2.latency.get() {
                    Some(e1.clone())
                } else {
                    Some(e2.clone())
                }
            },
            (true, false) => Some(e2.clone()),
            (false, true) => Some(e1.clone()),
            (false, false) => if rand::random() { Some(e1.clone()) } else { Some(e2.clone()) },
        }
    }
}
