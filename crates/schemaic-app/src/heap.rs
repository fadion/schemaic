//! Process-wide heap accounting, for diagnosing whether memory growth is a real
//! leak or benign allocator/OS retention.
//!
//! [`Tracking`] is installed as the global allocator: it delegates every call to
//! the system allocator and only adds two atomics tracking **live** bytes
//! (allocated − freed) and the running **peak**. This is the *logical* heap —
//! what the program is actually holding — which the OS's Private Working Set
//! (Task Manager) can't show directly, because a freed allocation stays resident
//! until the allocator returns those pages to the OS.
//!
//! Reading the two together decides the question:
//!   * live returns near its pre-load baseline after closing a table, but the
//!     working set stays high → the allocator/OS is holding freed pages for reuse
//!     (benign — it plateaus and gets reused, doesn't stack up); versus
//!   * live stays elevated after close → we're actually leaking (scopes/views/
//!     signals not disposed), and the grid's disposal is where to look.
//!
//! Accounting is always on (the atomics are negligible); logging is opt-in via
//! the `SCHEMAIC_HEAP_LOG` env var (see [`spawn_logger`]). Best observed on a
//! debug build, whose console keeps `tracing` output visible.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// System allocator wrapper that tallies live/peak bytes.
pub struct Tracking;

// SAFETY: every method forwards to the system allocator unchanged; the only
// additions are `Relaxed` atomic counters, which cannot affect allocation
// correctness.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grew(l.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let np = unsafe { System.realloc(p, l, new) };
        if !np.is_null() {
            let old = l.size();
            if new >= old {
                grew(new - old);
            } else {
                LIVE.fetch_sub(old - new, Ordering::Relaxed);
            }
        }
        np
    }
}

/// Record `n` newly-live bytes and lift the peak if we've passed it.
fn grew(n: usize) {
    let live = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

/// Bytes currently allocated and not yet freed.
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// Highest [`live_bytes`] seen so far this run.
pub fn peak_bytes() -> usize {
    PEAK.load(Ordering::Relaxed)
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// If `SCHEMAIC_HEAP_LOG` is set, spawn a background thread that logs live/peak
/// heap every second (or every `SCHEMAIC_HEAP_LOG_MS` ms if that parses to a
/// positive value). No-op otherwise, so it's free to call unconditionally.
pub fn spawn_logger() {
    if std::env::var_os("SCHEMAIC_HEAP_LOG").is_none() {
        return;
    }
    let period = std::env::var("SCHEMAIC_HEAP_LOG_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&ms| ms > 0)
        .unwrap_or(1000);
    let _ = std::thread::Builder::new()
        .name("heap-log".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(period));
                tracing::info!(
                    "heap: live={:.1} MB  peak={:.1} MB",
                    mib(live_bytes()),
                    mib(peak_bytes()),
                );
            }
        });
}
