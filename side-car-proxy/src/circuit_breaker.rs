use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Mutex,
};

use tracing::warn;

#[derive(Clone)]
pub struct CircuitConfig {
    pub window_ms: u64,
    pub request_volume_threshold: u64,
    pub failure_ratio: f64,
    pub open_duration_ms: u64,
    pub half_open_max_probes: u64,
    pub half_open_successes_to_close: u64,
}
impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            window_ms: 10_000,
            request_volume_threshold: 5,
            failure_ratio: 0.5,
            open_duration_ms: 20_000,
            half_open_max_probes: 5,
            half_open_successes_to_close: 3,
        }
    }
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BreakerState { Closed = 0, Open = 1, HalfOpen = 2 }

struct Window {
    start_ms: u64,
    req: u64,
    fail: u64,
}
impl Window {
    fn new(now_ms: u64) -> Self { Self { start_ms: now_ms, req: 0, fail: 0 } }
    fn rotate_if_needed(&mut self, now_ms: u64, window_ms: u64) {
        if now_ms.saturating_sub(self.start_ms) >= window_ms {
            self.start_ms = now_ms;
            self.req = 0;
            self.fail = 0;
        }
    }
}

pub struct CircuitBreaker {
    cfg: CircuitConfig,
    state: AtomicUsize,        // State
    opened_at_ms: AtomicU64,   // valid when Open
    win: Mutex<Window>,        // Closed window (always present)
    half_open_probes: AtomicUsize,
    half_open_successes: AtomicUsize,
}

impl CircuitBreaker {
    pub fn new(now_ms: u64, cfg: CircuitConfig) -> Self {
        Self {
            cfg,
            state: AtomicUsize::new(BreakerState::Closed as usize),
            opened_at_ms: AtomicU64::new(0),
            win: Mutex::new(Window::new(now_ms)),
            half_open_probes: AtomicUsize::new(0),
            half_open_successes: AtomicUsize::new(0),
        }
    }

    #[inline]
    pub fn load_state(&self) -> BreakerState {
        match self.state.load(Ordering::Acquire) {
            0 => BreakerState::Closed,
            1 => BreakerState::Open,
            _ => BreakerState::HalfOpen,
        }
    }

    fn set_state(&self, s: BreakerState, now_ms: u64) {
        match s {
            BreakerState::Closed => {
                // reset Closed window fresh
                *self.win.lock().unwrap() = Window::new(now_ms);
            }
            BreakerState::Open => {
                self.opened_at_ms.store(now_ms, Ordering::Release);
                self.half_open_probes.store(0, Ordering::Relaxed);
                self.half_open_successes.store(0, Ordering::Relaxed);
            }
            BreakerState::HalfOpen => {
                self.half_open_probes.store(0, Ordering::Relaxed);
                self.half_open_successes.store(0, Ordering::Relaxed);
            }
        }
        self.state.store(s as usize, Ordering::Release);
    }

    /// should we allow this request
    pub fn allow(&self, now_ms: u64) -> bool {
        match self.load_state() {
            BreakerState::Closed => true,
            BreakerState::Open => {
                let opened = self.opened_at_ms.load(Ordering::Acquire);
                if now_ms.saturating_sub(opened) >= self.cfg.open_duration_ms {
                    self.set_state(BreakerState::HalfOpen, now_ms);
                    true // probe
                } else {
                    false // fail-fast
                }
            }
            BreakerState::HalfOpen => {
                let probes = self.half_open_probes.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                probes <= self.cfg.half_open_max_probes
            }
        }
    }

    /// Record outcome after a request completes
    pub fn record(&self, now_ms: u64, ok: bool) {
        match self.load_state() {
            BreakerState::Closed => {
                let mut w = self.win.lock().unwrap();
                w.rotate_if_needed(now_ms, self.cfg.window_ms);
                w.req += 1;
                if !ok { w.fail += 1; }

                if w.req >= self.cfg.request_volume_threshold {
                    let ratio = (w.fail as f64) / (w.req as f64);
                    if ratio >= self.cfg.failure_ratio {
                        drop(w);
                        self.set_state(BreakerState::Open, now_ms);
                        // warn!("circuit breaker opened");
                    }
                }
            }
            BreakerState::Open => { }
            BreakerState::HalfOpen => {
                if ok {
                    let succ = self.half_open_successes.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                    let probes = self.half_open_probes.load(Ordering::Relaxed) as u64;
                    if succ >= self.cfg.half_open_successes_to_close || probes >= self.cfg.half_open_max_probes {
                        self.set_state(BreakerState::Closed, now_ms);
                        // warn!("circuit breaker closed");
                    }
                } else {
                    self.set_state(BreakerState::Open, now_ms); // any failure -> Open
                }
            }
        }
    }
}
