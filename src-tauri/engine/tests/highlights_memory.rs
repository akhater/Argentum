//! North star 5: memory, measured. `dt_highlights` on a photo with nothing
//! clipped finds that out by reading the photo, without copying it.
//!
//! Counted with a counting allocator, as `tests/memory.rs` counts: every byte
//! this test binary's Rust code allocates, and the most held at once. A test
//! binary of its own, with one test in it: the allocator is the whole
//! binary's.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use argentum_engine::rawler::rawimage::{RawImage, RawImageData};
use argentum_engine::tools::dt_highlights::{self, DtHighlights};
use argentum_engine::{Context, GpuSlot, Image, Source, Tool};

/// What the tool may hold at once, for each worker thread, to say a photo has
/// nothing clipped: the histograms `ceilings` builds to find the white a file
/// proves, 12 KB each for this 14-bit mosaic, one for each piece of work a
/// worker has split off and not yet merged, and nothing the size of the
/// photo. Measured at 23 to 112 KB a thread (185 to 892 KB on 8 threads, as
/// the work happens to split) at 6 MP, whatever the photo's size; 6.3 bytes a
/// photosite (37.6 MB) before the scan came first.
const UNCLIPPED_BUDGET_BYTES_A_THREAD: usize = 256 << 10;

#[test]
fn nothing_clipped_is_found_out_without_copying_the_photo() {
    let dim = |w, h| common::bayer(w, h, |r, c| 600 + ((r * 13 + c * 7) % 900) as u16);
    let float = |mut raw: RawImage| {
        if let RawImageData::Integer(v) = &raw.data {
            raw.data = RawImageData::Float(v.iter().map(|&x| x as f32).collect());
        }
        raw
    };

    // The thread pool and anything else made once, made before counting.
    for settings in [true, false] {
        peak_of(&dim(64, 64), settings);
        peak_of(&float(dim(64, 64)), settings);
    }

    let (w, h) = (3000, 2000);
    let mosaic_bytes = w * h * std::mem::size_of::<u16>();
    let budget = UNCLIPPED_BUDGET_BYTES_A_THREAD * rayon::current_num_threads();
    for (label, raw) in [("integer", dim(w, h)), ("float", float(dim(w, h)))] {
        for recover in [true, false] {
            let peak = peak_of(&raw, recover);
            eprintln!(
                "  {label} mosaic of {} MB, recovery {recover}: {peak} bytes at the peak, {} threads",
                mosaic_bytes >> 20,
                rayon::current_num_threads()
            );
            assert!(
                peak <= budget,
                "{label}, recovery {recover}: {peak} bytes held to find nothing clipped, \
                 allowed {budget}"
            );
        }
    }
}

/// The most the tool held at once, beyond what was held before, running on
/// `raw`, which has nothing clipped.
fn peak_of(raw: &RawImage, recover: bool) -> usize {
    let source = Source::new(1, raw.clone());
    let slot = GpuSlot::new();
    let cx = Context::new::<DtHighlights>(&source, 1, &slot);
    let input = Image::Raw(source.raw.clone());
    let settings = dt_highlights::Settings { recover };

    let before = HEAP_BYTES.load(Ordering::SeqCst);
    PEAK_BYTES.store(before, Ordering::SeqCst);
    let out = DtHighlights.process(&settings, &input, &cx).unwrap();
    let peak = PEAK_BYTES.load(Ordering::SeqCst) - before;
    assert!(out.is_none(), "nothing is clipped, so nothing is made");
    peak
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
