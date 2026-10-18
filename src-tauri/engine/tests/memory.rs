//! North star 5: memory, measured. The process's peak while a photo is
//! edited through the engine as the app does it, against the cache's
//! budget; and a closed photo's memory given back.
//!
//! A test binary of its own, with one test in it: the peak is the whole
//! process's.

mod common;

use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use argentum_engine::Engine;
use argentum_engine::contract::{EditPatch, Event, RenderTicket, View};
use argentum_engine::pipeline::DEFAULT_CACHE_BYTES;

/// What the process may hold at its peak past the cache's budget, in
/// full-size pictures (12 bytes a pixel): the result being made and the
/// develop's working memory are within the budget, but a tool's own working
/// memory, the picture a step is reading and the photo's decoded data are
/// not.
///
/// Both limits here are set from their first measurement, with the default
/// budget of 1.5 GB, on an integrated GPU, whose memory is the process's,
/// 2026-10-18, and never to be raised. This one: 0.69 to 0.89 for a 32 MP
/// Canon CR3 (a peak of 1.75 to 1.82 GB over three runs) and 1.03 for the
/// synthetic 61 MP photo (2.19 GB).
const PAST_THE_BUDGET_PICTURES: f64 = 1.5;

/// What the process may hold at its peak while the photo is only fitted, in
/// full-size pictures: a fitted view develops at half size, so nothing
/// full-size is made. Measured: 1.58 for the CR3 (0.57 GB) and 1.22 for the
/// synthetic photo (0.82 GB); developing at full size, it was 3.3 and 2.6.
const FITTED_PICTURES: f64 = 2.0;

#[test]
fn memory_stays_within_the_budget_and_is_given_back() {
    if memory().is_none() {
        eprintln!(
            "memory_stays_within_the_budget_and_is_given_back: SKIPPED - no memory counters here"
        );
        return;
    }
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
    let base = memory().unwrap().now;

    // A real photo when there is one; otherwise a synthetic one as large as
    // the largest the app is for.
    let (opened, name) = match common::raw("memory_stays_within_the_budget_and_is_given_back") {
        Some(path) => (engine.open(&path).unwrap(), path.display().to_string()),
        None => (
            engine.insert(common::source(9504, 6336)),
            "synthetic".to_string(),
        ),
    };
    let id = opened.photo_id;
    let picture = opened.width as usize * opened.height as usize * 12;
    let gb = |bytes: usize| bytes as f64 / (1u64 << 30) as f64;
    eprintln!(
        "\n  {name}: {} x {}, {:.2} GB a picture, a cache of {:.2} GB",
        opened.width,
        opened.height,
        gb(picture),
        gb(budget)
    );

    // Fitted, then at 100%, in two window sizes, with a slider dragged at
    // each: what the app does with a photo.
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
        let now = memory().unwrap();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        eprintln!(
            "  {zoom:>4} in {w} x {h}: first frame {:.0} ms, a tick {:.0} ms; cache {:.2} GB, process {:.2} GB past the start, peak {:.2} GB",
            ms(first),
            ms(ticks[ticks.len() / 2]),
            gb(engine.cache_bytes()),
            gb(now.now.saturating_sub(base)),
            gb(now.peak.saturating_sub(base))
        );
        assert!(
            engine.cache_bytes() <= budget,
            "the cache is past its budget"
        );
        if zoom == "fit" {
            let fitted = now.peak.saturating_sub(base);
            assert!(
                fitted as f64 <= FITTED_PICTURES * picture as f64,
                "fitted, the peak is {:.2} GB, {:.2} pictures; allowed {FITTED_PICTURES}",
                gb(fitted),
                fitted as f64 / picture as f64
            );
        }
    }

    let peak = memory().unwrap().peak.saturating_sub(base);
    let allowed = budget + (PAST_THE_BUDGET_PICTURES * picture as f64) as usize;
    assert!(
        peak <= allowed,
        "the peak is {:.2} GB, {:.1} pictures past the budget; allowed {PAST_THE_BUDGET_PICTURES}",
        gb(peak),
        (peak as f64 - budget as f64) / picture as f64
    );

    // Closed: the results and the decoded photo go.
    let held = engine.cache_bytes();
    let before = memory().unwrap().now;
    engine.close(id).unwrap();
    let waited = Instant::now();
    while engine.cache_bytes() > 0 {
        assert!(waited.elapsed() < Duration::from_secs(30), "never let go");
        std::thread::sleep(Duration::from_millis(1));
    }
    let freed = before.saturating_sub(memory().unwrap().now);
    eprintln!(
        "  closed: {:.2} GB given back, of {:.2} GB of results",
        gb(freed),
        gb(held)
    );
    assert!(freed >= held, "closing kept memory");
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

/// The process's memory in bytes: what it holds now, and the most it has.
struct Memory {
    now: usize,
    peak: usize,
}

/// Private bytes committed, and their peak.
#[cfg(windows)]
fn memory() -> Option<Memory> {
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
    (ok != 0).then_some(Memory {
        now: c.pagefile_usage,
        peak: c.peak_pagefile_usage,
    })
}

/// Resident bytes, and their peak.
#[cfg(target_os = "linux")]
fn memory() -> Option<Memory> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |field: &str| -> Option<usize> {
        let line = status.lines().find(|l| l.starts_with(field))?;
        line.split_whitespace().nth(1)?.parse::<usize>().ok()
    };
    Some(Memory {
        now: kb("VmRSS:")? * 1024,
        peak: kb("VmHWM:")? * 1024,
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn memory() -> Option<Memory> {
    None
}
