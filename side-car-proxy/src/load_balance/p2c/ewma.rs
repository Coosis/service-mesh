use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub struct Ewma {
    origin: std::time::Instant,
    value: AtomicU64,
    last: AtomicU64,

    tau: f64,
}

impl Ewma {
    pub fn new(tau: f64) -> Self {
        Self { 
            origin: std::time::Instant::now(),
            value: AtomicU64::new(f64::NAN.to_bits()), 
            last: AtomicU64::new(0),

            tau,
        }
    }

    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    pub fn update(&self, sample: f64) {
        use std::sync::atomic::Ordering;
        let now_ms = self.origin.elapsed().as_millis() as u64;
        let last   = self.last.load(Ordering::Relaxed);

        if last == 0 {
            self.last.store(now_ms, Ordering::Relaxed);
            self.value.store(sample.to_bits(), Ordering::Relaxed);
            return;
        }

        let mut dt = (now_ms - last) as f64;
        let dt_cap = 5.0 * self.tau;
        if dt > dt_cap { dt = dt_cap; }

        let alpha = 1.0 - (-dt / self.tau).exp();
        let prev  = f64::from_bits(self.value.load(Ordering::Relaxed));
        let new   = if prev.is_finite() { prev + alpha * (sample - prev) } else { sample };

        self.last.store(now_ms, Ordering::Relaxed);
        self.value.store(new.to_bits(), Ordering::Relaxed);
    }
}
