//! North star 5, app side: the most bytes a piece of work holds at once,
//! counted, so a memory budget is a test and not a hope.
//!
//! The counting allocator and its peak bookkeeping are the engine's, from
//! `engine/tests/memory.rs` and `engine/tests/highlights_memory.rs` (commit
//! 7c2bc9a6): a global allocator that adds each allocation to a held total and
//! keeps the highest total seen. Copied, not shared: those files are test
//! binaries of the engine crate, and nothing both crates build can hold it
//! without a new crate.
//!
//! What differs, and why. The engine measures in test binaries of their own,
//! one test each, because a global allocator belongs to its whole binary and
//! tests in one binary run in parallel, which would add each other's
//! allocations to the peak. The app cannot do that: its lib already installs
//! mimalloc as the global allocator (a second one in an integration test
//! binary is a link error), its modules are private to the lib, and
//! `tests/` here holds data. So the counting allocator lives in the lib's own
//! test build (`lib.rs` swaps it in under `cfg(test)`, wrapping mimalloc) and
//! counts only the threads of the measurement in progress: `peak_bytes` runs
//! the work on a private rayon pool whose threads are marked, so the work's own
//! allocations and its `par_iter` workers' count, and a test running beside it
//! does not. Measurements take turns, so two never share the counters.
//!
//! Limits: threads the work spawns itself with `std::thread` are not counted,
//! and neither is rayon work the closure hands to the global pool by name
//! (`rayon::spawn`); `par_iter` and `join` inside the closure use the private
//! pool and are counted. Bytes allocated inside and freed by an unmarked thread
//! stay counted, so a peak is never under-reported.
//!
//! Writing a budget test:
//!
//! ```ignore
//! // Anything made once (thread pools, statics) is made before counting.
//! let input = make_synthetic_input();
//! let (out, peak) = mem_probe::peak_bytes(|| the_function(&input));
//! // `peak` is bytes held at once beyond what was held before the call,
//! // the result included. Set the budget from this number, plus a margin.
//! assert!(peak <= BUDGET, "{peak} bytes held, allowed {BUDGET}");
//! ```

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
use std::alloc::System;
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static HELD: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

thread_local! {
    // `const` and without a destructor, so the allocator can read it at any
    // point of a thread's life.
    static MARKED: Cell<bool> = const { Cell::new(false) };
}

fn counting() -> bool {
    ACTIVE.load(Ordering::Relaxed) && MARKED.try_with(Cell::get).unwrap_or(false)
}

fn held(bytes: usize) {
    let now = HELD.fetch_add(bytes as isize, Ordering::SeqCst) + bytes as isize;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

fn released(bytes: usize) {
    // Not below zero: memory the pool's threads free that they did not
    // allocate while counting (their own bookkeeping, from before) is not
    // room for the work, and would make a peak read under what it held.
    let _ = HELD.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |h| {
        Some((h - bytes as isize).max(0))
    });
}

#[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
type Inner = mimalloc::MiMalloc;
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
type Inner = System;

/// The app's own allocator (mimalloc) with the counting added.
pub struct Counting(pub Inner);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.0.alloc(layout) };
        if !p.is_null() && counting() {
            held(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.0.alloc_zeroed(layout) };
        if !p.is_null() && counting() {
            held(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { self.0.dealloc(p, layout) };
        if counting() {
            released(layout.size());
        }
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = unsafe { self.0.realloc(p, layout, size) };
        if !q.is_null() && counting() {
            released(layout.size());
            held(size);
        }
        q
    }
}

/// Runs `work` and returns its result with the most bytes it held at once,
/// beyond what was held before it began (its result included while it is
/// alive). See the module comment for what is counted.
pub fn peak_bytes<R: Send>(work: impl FnOnce() -> R + Send) -> (R, usize) {
    // A test that failed while measuring must not stop the others.
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // Built before counting, with the threads' own bookkeeping.
    let pool = rayon::ThreadPoolBuilder::new()
        .start_handler(|_| MARKED.with(|m| m.set(true)))
        .build()
        .expect("the measuring thread pool");
    HELD.store(0, Ordering::SeqCst);
    PEAK.store(0, Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    let result = pool.install(work);
    ACTIVE.store(false, Ordering::SeqCst);
    let peak = PEAK.load(Ordering::SeqCst).max(0) as usize;
    drop(pool);
    (result, peak)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    const N: usize = 8 << 20;
    /// What a closure's own bookkeeping may add to the number it reports.
    const MARGIN: usize = 64 << 10;

    #[test]
    fn a_known_vec_is_counted_to_the_byte_or_a_little_over() {
        let (v, peak) = peak_bytes(|| black_box(vec![1u8; N]));
        assert!(peak >= N, "{peak} bytes reported for a {N} byte Vec");
        assert!(
            peak < N + MARGIN,
            "{peak} bytes reported for a {N} byte Vec"
        );
        drop(v);
    }

    #[test]
    fn memory_held_before_the_closure_is_not_counted() {
        let before = black_box(vec![1u8; 4 * N]);
        let (_, peak) = peak_bytes(|| black_box(vec![1u8; N]).len());
        assert!((N..N + MARGIN).contains(&peak), "{peak} bytes reported");
        drop(before);
    }

    #[test]
    fn rayon_workers_working_for_the_closure_are_counted() {
        use rayon::prelude::*;
        let (_, peak) = peak_bytes(|| {
            (0..4usize).into_par_iter().for_each(|_| {
                black_box(vec![1u8; N]);
            });
        });
        assert!(
            peak >= N,
            "{peak} bytes reported for workers holding {N} each"
        );
    }

    /// First budget from the memory sweep (2026-11-23): converting the decoded
    /// photo to the GPU's half-float RGBA texture input. Measured on a 4 MP
    /// synthetic photo: one output buffer of 8 bytes a pixel and nothing else.
    /// Allowed: that, plus the margin of the probe.
    #[test]
    fn the_gpu_input_conversion_holds_only_its_output() {
        use image::{DynamicImage, Rgb32FImage};
        let (w, h) = (2048u32, 2048u32);
        let img = DynamicImage::ImageRgb32F(Rgb32FImage::from_fn(w, h, |x, y| {
            image::Rgb([x as f32 / w as f32, y as f32 / h as f32, 0.5])
        }));
        let (out, peak) = peak_bytes(|| crate::gpu_processing::to_rgba_f16(&img));
        let output_bytes = w as usize * h as usize * 4 * std::mem::size_of::<half::f16>();
        eprintln!("  to_rgba_f16 on {w}x{h}: {peak} bytes at the peak, output {output_bytes}");
        assert_eq!(out.len() * 2, output_bytes);
        assert!(
            peak <= output_bytes + MARGIN,
            "{peak} bytes held to convert, allowed {}",
            output_bytes + MARGIN
        );
    }
}
