//! North star 5: memory, measured. What the engine holds at its peak while
//! a photo is edited as the app does it, against the cache's budget; and a
//! closed photo's memory given back.
//!
//! What the engine holds is counted, not read off the process: every byte
//! its Rust code allocates (a counting allocator for this test binary) and
//! every byte of GPU memory wgpu holds for its device (wgpu's allocator
//! report, its own staging copies included). Counted, it is the same on any
//! machine and any GPU, a software one included, so the budget is checked on
//! every pull request. The whole process's peak - the GPU driver's and the
//! operating system's share with it - is printed beside it, not checked: it
//! depends on the machine, not on the engine.
//!
//! A test binary of its own, with one test in it: the counting allocator is
//! the whole binary's.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use argentum_engine::Engine;
use argentum_engine::contract::{EditPatch, Event, RenderTicket, View};
use argentum_engine::pipeline::DEFAULT_CACHE_BYTES;

// The limits are the design's own arithmetic, not a measurement plus a
// margin: every result the engine keeps is in the cache, which counts it
// against its budget, and the test asserts that separately. Outside the
// cache the design keeps only what is named here. Anything else at the
// peak is memory nobody accounted for, as wgpu's staging copies of the
// pixel pass's whole input were (0.45 GB at 61 MP, caught by this test).

/// The photo's decoded mosaic, the photo's own and never copied: 16-bit
/// sensor values.
const MOSAIC_BYTES_A_PIXEL: usize = 2;

/// The picture the pixel pass reads, kept on the GPU as half-float RGBA
/// while it is shown: the full-size picture at 100%.
const GPU_INPUT_BYTES_A_PIXEL: usize = 8;

/// What a render works in, a band at a time, whatever the photo's area:
/// the 64 MB bands a picture crosses to and from the GPU in (the half
/// floats being sent, wgpu's staging copy of them, the frame read back),
/// capture sharpening's row of tiles, and the guided filter's small
/// textures.
const BANDS_BYTES: usize = 256 << 20;

#[test]
fn memory_stays_within_the_budget_and_is_given_back() {
    // Frames are made on the GPU.
    if common::gpu("memory_stays_within_the_budget_and_is_given_back").is_none() {
        return;
    }
    let (tx, rx) = channel();
    let engine = Engine::new(move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let budget = DEFAULT_CACHE_BYTES;
    let held = Held::new();
    let process = process_memory().map(|m| m.now);

    // A real photo when there is one; otherwise a synthetic one as large as
    // the largest the app is for.
    let (opened, name) = match common::raw("memory_stays_within_the_budget_and_is_given_back") {
        Some(path) => (engine.open(&path).unwrap(), path.display().to_string()),
        None => (
            engine.insert(common::source(9504, 6336)).unwrap(),
            "synthetic".to_string(),
        ),
    };
    let id = opened.photo_id;
    let pixels = opened.width as usize * opened.height as usize;
    let picture = pixels * 12;
    let gb = |bytes: usize| bytes as f64 / (1u64 << 30) as f64;
    eprintln!(
        "\n  {name}: {} x {}, {:.2} GB a picture, a cache of {:.2} GB",
        opened.width,
        opened.height,
        gb(picture),
        gb(budget)
    );

    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| held.sample(&engine, &stop));

        // Fitted, then at 100%, in two window sizes, with a slider dragged
        // at each: what the app does with a photo.
        for (zoom, (w, h)) in [
            ("fit", (1600, 1067)),
            ("fit", (2560, 1440)),
            ("100%", (1600, 1067)),
            ("100%", (2560, 1440)),
        ] {
            let fit = (w as f32 / opened.width as f32).min(h as f32 / opened.height as f32);
            let view = View {
                zoom: if zoom == "fit" { fit } else { 1.0 },
                center_x: 0.5,
                center_y: 0.5,
                width: w,
                height: h,
            };
            let started = Instant::now();
            until_framed(&rx, engine.view(id, view).unwrap());
            let first = started.elapsed();
            let mut ticks = Vec::new();
            for i in 1..=6 {
                let tick =
                    patch(serde_json::json!({ "ag_exposure": { "exposure_ev": i as f32 * 0.1 } }));
                let started = Instant::now();
                until_framed(&rx, engine.apply(id, &tick).unwrap().ticket);
                ticks.push(started.elapsed());
            }
            ticks.sort();
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            let peak = held.peak();
            eprintln!(
                "  {zoom:>4} in {w} x {h}: first frame {:.0} ms, a tick {:.0} ms; cache {:.2} GB; the engine holds at most {:.2} GB ({:.2} its heap, {:.2} the GPU's){}",
                ms(first),
                ms(ticks[ticks.len() / 2]),
                gb(engine.cache_bytes()),
                gb(peak.total()),
                gb(peak.heap),
                gb(peak.gpu),
                process_peak(process)
            );
            assert!(
                engine.cache_bytes() <= budget,
                "the cache is past its budget"
            );
            if zoom == "fit" {
                // The photo's first, fitted view develops at half size: four
                // quarter-size pictures, one full-size picture's bytes, and
                // the mosaic. A full-size develop would hold three.
                let allowed = picture + MOSAIC_BYTES_A_PIXEL * pixels + BANDS_BYTES;
                assert!(
                    peak.total() <= allowed,
                    "fitted, the engine holds {:.2} GB; allowed {:.2}",
                    gb(peak.total()),
                    gb(allowed)
                );
            }
        }
        stop.store(true, Ordering::Relaxed);
    });

    let peak = held.peak();
    let allowed = budget + (MOSAIC_BYTES_A_PIXEL + GPU_INPUT_BYTES_A_PIXEL) * pixels + BANDS_BYTES;
    assert!(
        peak.total() <= allowed,
        "the engine holds {:.2} GB at its peak ({:.2} its heap, {:.2} the GPU's); allowed {:.2}: the cache's budget, the mosaic, the GPU's input and the bands",
        gb(peak.total()),
        gb(peak.heap),
        gb(peak.gpu),
        gb(allowed)
    );

    // Closed: the results, the decoded photo and the GPU's copy go.
    let cached = engine.cache_bytes();
    let before = held.now(&engine).total();
    engine.close(id).unwrap();
    let waited = Instant::now();
    while engine.cache_bytes() > 0 {
        assert!(waited.elapsed() < Duration::from_secs(30), "never let go");
        std::thread::sleep(Duration::from_millis(1));
    }
    let freed = before.saturating_sub(held.now(&engine).total());
    eprintln!(
        "  closed: {:.2} GB given back, of {:.2} GB of results",
        gb(freed),
        gb(cached)
    );
    assert!(freed >= cached, "closing kept memory");
}

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

fn until_framed(rx: &Receiver<Event>, ticket: RenderTicket) {
    loop {
        match rx.recv_timeout(Duration::from_secs(120)).expect("an event") {
            Event::Frame { ticket: t, .. } if t == ticket => return,
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

/// Every byte this binary's Rust code holds on the heap.
struct Counting;

static HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: the system allocator's own, with a count beside it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            HEAP_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            HEAP_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
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
            HEAP_BYTES.fetch_add(size, Ordering::Relaxed);
            HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What the engine holds past the start of the test.
#[derive(Clone, Copy, Default)]
struct Bytes {
    heap: usize,
    gpu: usize,
}

impl Bytes {
    fn total(self) -> usize {
        self.heap + self.gpu
    }
}

/// The engine's memory, counted, and the most of it seen together.
struct Held {
    heap_at_start: usize,
    peak_heap: AtomicUsize,
    peak_gpu: AtomicUsize,
}

impl Held {
    fn new() -> Self {
        Self {
            heap_at_start: HEAP_BYTES.load(Ordering::SeqCst),
            peak_heap: AtomicUsize::new(0),
            peak_gpu: AtomicUsize::new(0),
        }
    }

    fn now(&self, engine: &Engine) -> Bytes {
        Bytes {
            heap: HEAP_BYTES
                .load(Ordering::SeqCst)
                .saturating_sub(self.heap_at_start),
            gpu: engine.gpu_bytes().unwrap_or(0),
        }
    }

    /// Every millisecond until `stop`: the heap and the GPU's memory move
    /// at different moments, so the peak is of the two taken together.
    fn sample(&self, engine: &Engine, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            let now = self.now(engine);
            let peak = self.peak();
            if now.total() > peak.total() {
                self.peak_heap.store(now.heap, Ordering::SeqCst);
                self.peak_gpu.store(now.gpu, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn peak(&self) -> Bytes {
        Bytes {
            heap: self.peak_heap.load(Ordering::SeqCst),
            gpu: self.peak_gpu.load(Ordering::SeqCst),
        }
    }
}

/// ", the process's peak N GB past the start", where there are counters.
fn process_peak(start: Option<usize>) -> String {
    match (start, process_memory()) {
        (Some(start), Some(m)) => format!(
            "; the whole process's peak, driver and system included, {:.2} GB",
            m.peak.saturating_sub(start) as f64 / (1u64 << 30) as f64
        ),
        _ => String::new(),
    }
}

/// The process's memory in bytes: what it holds now, and the most it has.
struct ProcessMemory {
    now: usize,
    peak: usize,
}

/// Private bytes committed, and their peak.
#[cfg(windows)]
fn process_memory() -> Option<ProcessMemory> {
    use std::ffi::c_void;

    // PROCESS_MEMORY_COUNTERS, psapi.h.
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32GetProcessMemoryInfo(process: *mut c_void, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: the current process's pseudo-handle, and a counters struct of
    // the size it says.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some(ProcessMemory {
        now: c.pagefile_usage,
        peak: c.peak_pagefile_usage,
    })
}

/// Resident bytes, and their peak.
#[cfg(target_os = "linux")]
fn process_memory() -> Option<ProcessMemory> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |field: &str| -> Option<usize> {
        let line = status.lines().find(|l| l.starts_with(field))?;
        line.split_whitespace().nth(1)?.parse::<usize>().ok()
    };
    Some(ProcessMemory {
        now: kb("VmRSS:")? * 1024,
        peak: kb("VmHWM:")? * 1024,
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_memory() -> Option<ProcessMemory> {
    None
}
