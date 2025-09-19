use std::sync::atomic::{AtomicBool, AtomicUsize};
use http::uri::Authority;
use crate::circuit_breaker::CircuitBreaker;

use super::p2c::Ewma;

const EWMA_TAU: f64 = 10_000.0;

pub struct Endpoint {
    pub authority: Authority,
    pub healthy: AtomicBool,
    /// when last ejected (ms since cluster origin), 0 if not ejected
    pub last_eject_ms: std::sync::atomic::AtomicU64,
    pub latency: Ewma,

    pub in_flight: AtomicUsize,
    pub consec_fail: AtomicUsize,

    pub breaker: CircuitBreaker,
}

impl Endpoint {
    pub fn new(authority: Authority) -> Self {
        Self { 
            authority,
            healthy: AtomicBool::new(true),
            last_eject_ms: std::sync::atomic::AtomicU64::new(0),
            latency: Ewma::new(EWMA_TAU),

            in_flight: AtomicUsize::new(0),
            consec_fail: AtomicUsize::new(0),

            breaker: CircuitBreaker::new(
                0,
                crate::circuit_breaker::CircuitConfig::default(),
            ),
        }
    }
}
