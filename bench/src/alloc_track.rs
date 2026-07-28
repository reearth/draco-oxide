//! Counting global allocator for the memory benchmarks: tracks live heap
//! bytes on every allocation event and, while a measurement window is open,
//! integrates the live-bytes curve over time to give the window's peak,
//! time-weighted average, and RMS (2-norm), all relative to the live bytes at
//! window start.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub struct TrackingAlloc;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static WINDOW_OPEN: AtomicBool = AtomicBool::new(false);
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

struct Window {
    base: isize,
    last: Instant,
    rel_peak: isize,
    /// Integral of relative live bytes over seconds.
    sum_dt: f64,
    /// Integral of squared relative live bytes over seconds.
    sum_sq_dt: f64,
    total_dt: f64,
}

impl Window {
    /// Closes the interval since the last event at `live` bytes held.
    fn integrate_to(&mut self, now: Instant, live: isize) {
        let dt = now.duration_since(self.last).as_secs_f64();
        let rel = (live - self.base) as f64;
        self.sum_dt += rel * dt;
        self.sum_sq_dt += rel * rel * dt;
        self.total_dt += dt;
        self.last = now;
    }
}

/// Heap usage of one measurement window, in bytes relative to window start.
pub struct HeapStats {
    pub peak: usize,
    pub avg: f64,
    pub rms: f64,
}

fn record(delta: isize) {
    let live_after = LIVE.fetch_add(delta, Ordering::Relaxed) + delta;
    if !WINDOW_OPEN.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut guard) = WINDOW.lock() {
        if let Some(w) = guard.as_mut() {
            w.integrate_to(Instant::now(), live_after - delta);
            let rel_after = live_after - w.base;
            if rel_after > w.rel_peak {
                w.rel_peak = rel_after;
            }
        }
    }
}

/// Opens the measurement window at the current live-byte level.
pub fn start_window() {
    let mut guard = WINDOW.lock().unwrap();
    *guard = Some(Window {
        base: LIVE.load(Ordering::Relaxed),
        last: Instant::now(),
        rel_peak: 0,
        sum_dt: 0.0,
        sum_sq_dt: 0.0,
        total_dt: 0.0,
    });
    drop(guard);
    WINDOW_OPEN.store(true, Ordering::Relaxed);
}

/// Closes the window and returns its stats.
pub fn end_window() -> HeapStats {
    WINDOW_OPEN.store(false, Ordering::Relaxed);
    let mut guard = WINDOW.lock().unwrap();
    let mut w = guard.take().expect("end_window without start_window");
    w.integrate_to(Instant::now(), LIVE.load(Ordering::Relaxed));
    let total = w.total_dt.max(f64::MIN_POSITIVE);
    HeapStats {
        peak: w.rel_peak.max(0) as usize,
        avg: w.sum_dt / total,
        rms: (w.sum_sq_dt / total).sqrt(),
    }
}

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            record(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        record(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            record(new_size as isize - layout.size() as isize);
        }
        new_ptr
    }
}
