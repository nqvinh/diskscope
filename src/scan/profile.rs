//! Opt-in phase timings for performance work: set DISKSCOPE_PROFILE=1 and every phase prints one
//! line to stderr. `knob` reads numeric A/B switches (e.g. DISKSCOPE_MFT_READERS) so a single run
//! script can compare variants. Off by default: no output, no measurable cost.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DISKSCOPE_PROFILE").is_some())
}

pub fn report(phase: &str, d: Duration) {
    if on() {
        eprintln!("[profile] {phase:<30} {:>9.1} ms", d.as_secs_f64() * 1e3);
    }
}

/// Runs `f` and reports its wall time as `phase`.
pub fn time<T>(phase: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    report(phase, t.elapsed());
    r
}

/// Numeric environment switch `name`, or `default` when unset or not a number.
pub fn knob(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// Nanoseconds summed across threads (CPU time spent in a phase, not wall time).
#[derive(Default)]
pub struct Sum(AtomicU64);

impl Sum {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    pub fn reset(&self) {
        self.0.store(0, Relaxed);
    }
    pub fn add(&self, since: Instant) {
        if on() {
            self.0.fetch_add(since.elapsed().as_nanos() as u64, Relaxed);
        }
    }
    pub fn report(&self, phase: &str) {
        report(phase, Duration::from_nanos(self.0.load(Relaxed)));
    }
}
