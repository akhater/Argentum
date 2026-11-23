//! North star 5: memory, measured. The RapidRAW demosaic writes each tile
//! straight into the picture it returns: the result is never held twice.
//!
//! Counted with a counting allocator, as `tests/highlights_memory.rs` counts:
//! every byte this test binary's Rust code allocates, and the most held at
//! once. A test binary of its own, with one test in it.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use argentum_engine::rr_demosaic::Mosaic;

/// What a worker thread may hold beyond the picture itself: its tile's
/// working buffers, measured at 11 MB for a Bayer tile, and 2 MB of slack.
/// The result held twice, as each band of tiles was collected before it was
/// copied in, peaked at 249 to 262 MB here (the picture 100 MB, 8 threads, 8
/// bands in flight); written straight in, 189 MB.
const WORKER_BYTES: usize = 13 << 20;

#[test]
fn the_demosaic_does_not_hold_its_result_twice() {
    let (w, h) = (4096usize, 2048usize);
    let raw = common::bayer(w, h, |r, c| 600 + ((r * 13 + c * 7) % 9000) as u16);
    let mosaic = Mosaic::new(&raw, [2.0, 1.0, 1.5], 15000.0).expect("a Bayer mosaic");

    // The thread pool and anything else made once, made before counting.
    let small = common::bayer(64, 64, |r, c| 600 + ((r + c) % 900) as u16);
    drop(
        Mosaic::new(&small, [2.0, 1.0, 1.5], 15000.0)
            .unwrap()
            .develop(),
    );

    let picture = w * h * 12;
    let before = HEAP_BYTES.load(Ordering::SeqCst);
    PEAK_BYTES.store(before, Ordering::SeqCst);
    let started = std::time::Instant::now();
    let out = mosaic.develop();
    let took = started.elapsed();
    let peak = PEAK_BYTES.load(Ordering::SeqCst) - before;
    assert_eq!(out.len(), w * h);
    let budget = picture + WORKER_BYTES * rayon::current_num_threads();
    eprintln!(
        "  {w} x {h}: {took:?}, peak {peak} bytes, picture {picture}, {} threads, allowed {budget}",
        rayon::current_num_threads()
    );
    assert!(peak <= budget, "{peak} bytes held, allowed {budget}");
}

static HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn held(bytes: usize) {
    let now = HEAP_BYTES.fetch_add(bytes, Ordering::SeqCst) + bytes;
    PEAK_BYTES.fetch_max(now, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            held(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            held(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
        HEAP_BYTES.fetch_sub(layout.size(), Ordering::SeqCst);
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, layout, size) };
        if !q.is_null() {
            HEAP_BYTES.fetch_sub(layout.size(), Ordering::SeqCst);
            held(size);
        }
        q
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;
