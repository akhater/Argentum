//! North star 5: memory, measured, for the pixel pass's read-back. The whole
//! picture crosses back from the GPU as linear light; the alpha channel of
//! the texture it is read from is not part of the result, so it is dropped
//! as each band arrives and never held for the whole picture.
//!
//! A test binary of its own: the counting allocator is the whole binary's.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use argentum_engine::contract::EditPatch;
use argentum_engine::{GpuSlot, Pipeline, Rgb, standard};

/// The picture: a result is 12 bytes a pixel (three f32).
const PIXEL_BYTES: usize = 12;

/// What the read-back's bands may hold at once beside the picture: the half
/// floats being sent are one 64 MB band, with room for the allocator's
/// rounding. The old read-back held the RGBA result of the whole picture
/// (16 bytes a pixel) on top of it.
const BANDS_BYTES: usize = 128 << 20;

#[test]
fn the_read_back_does_not_hold_the_alpha_channel_of_the_whole_picture() {
    let test = "the_read_back_does_not_hold_the_alpha_channel_of_the_whole_picture";
    let Some(gpu) = common::gpu(test) else {
        return;
    };
    let (w, h) = (6000usize, 4000usize);
    let source = common::source(64, 64);
    let rgb = Rgb {
        width: w,
        height: h,
        pixels: vec![[0.25, 0.5, 0.75]; w * h],
        full_width: w,
    };
    let mut p = Pipeline::with_gpu(Arc::new(standard()), GpuSlot::with(gpu)).unwrap();
    let patch: EditPatch =
        serde_json::from_value(serde_json::json!({ "ag_exposure": { "exposure_ev": 0.5 } }))
            .unwrap();
    p.apply(&patch).unwrap();

    let before = HEAP_BYTES.load(Ordering::SeqCst);
    PEAK_BYTES.store(before, Ordering::SeqCst);
    let out = p.run_pixel_tools(&source, &rgb).unwrap();
    let peak = PEAK_BYTES.load(Ordering::SeqCst).saturating_sub(before);
    assert_eq!((out.width, out.height), (w, h));

    // The pass's own copy of the input and its result: two pictures.
    let allowed = 2 * PIXEL_BYTES * w * h + BANDS_BYTES;
    let mb = |b: usize| b as f64 / (1 << 20) as f64;
    eprintln!(
        "  read-back peak heap {:.0} MB; allowed {:.0} MB",
        mb(peak),
        mb(allowed)
    );
    assert!(
        peak <= allowed,
        "the pixel pass held {:.0} MB; allowed {:.0}: two pictures and the bands",
        mb(peak),
        mb(allowed)
    );
}

/// Every byte this binary's Rust code holds on the heap, and the most it
/// held.
struct Counting;

static HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = HEAP_BYTES.fetch_add(by, Ordering::Relaxed) + by;
    PEAK_BYTES.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: the system allocator's own, with a count beside it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
        HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, layout, size) };
        if !q.is_null() {
            HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            grew(size);
        }
        q
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;
