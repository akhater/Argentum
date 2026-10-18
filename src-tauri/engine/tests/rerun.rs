//! Pass test 2: changing one tool's settings re-runs that tool and the ones
//! after it, and nothing before it - counted on a synthetic RAW, and counted
//! and timed on a real one.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use argentum_engine::tools::{ag_exposure_wb, dt_highlights, rt_capture_sharpen};
use argentum_engine::{Gpu, GpuSlot, Pipeline, Size, standard};

fn ran(p: &Pipeline) -> Vec<&'static str> {
    p.last_render()
        .iter()
        .filter(|s| s.ran)
        .map(|s| s.name)
        .collect()
}

/// With a GPU, capture sharpening runs for real; without one it is switched
/// off, and still counted as a step.
fn pipeline(gpu: Option<Arc<Gpu>>) -> Pipeline {
    match gpu {
        Some(gpu) => Pipeline::with_gpu(Arc::new(standard()), GpuSlot::with(gpu)).unwrap(),
        None => {
            let mut p = Pipeline::standard().unwrap();
            p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
                capture: false,
                ..Default::default()
            })
            .unwrap();
            p
        }
    }
}

fn exposure(ev: f32) -> ag_exposure_wb::Settings {
    ag_exposure_wb::Settings {
        exposure: ev,
        ..Default::default()
    }
}

fn amount(pct: f32) -> rt_capture_sharpen::Settings {
    rt_capture_sharpen::Settings {
        capture_amount: pct,
        ..Default::default()
    }
}

#[test]
fn changing_one_tool_reruns_it_and_what_follows() {
    let mut p = pipeline(common::gpu("changing_one_tool_reruns_it_and_what_follows"));
    let source = common::source(512, 384);
    let all = [
        "dt_highlights",
        "develop",
        "rt_capture_sharpen",
        "ag_exposure_wb",
    ];

    let first = p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), all);
    let again = p.render(&source, Size::Full).unwrap();
    assert!(ran(&p).is_empty(), "nothing changed: {:?}", ran(&p));
    assert!(Arc::ptr_eq(&first, &again), "the same picture, not a copy");

    // The last tool: only it.
    p.set::<ag_exposure_wb::AgExposureWb>(exposure(1.0))
        .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), ["ag_exposure_wb"]);

    // The middle one: it and the one after it.
    if p.get::<rt_capture_sharpen::RtCaptureSharpen>()
        .unwrap()
        .capture
    {
        p.set::<rt_capture_sharpen::RtCaptureSharpen>(amount(50.0))
            .unwrap();
    } else {
        p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
            capture: false,
            iterations: 10,
            ..Default::default()
        })
        .unwrap();
    }
    p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), ["rt_capture_sharpen", "ag_exposure_wb"]);

    // The first: everything, the develop included.
    p.set::<dt_highlights::DtHighlights>(dt_highlights::Settings { recover: false })
        .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), all);

    // Back as it was: those results are still kept, so nothing runs. The
    // cache is keyed by what the settings are, not by what changed.
    p.set::<dt_highlights::DtHighlights>(dt_highlights::Settings { recover: true })
        .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert!(ran(&p).is_empty(), "{:?}", ran(&p));

    // A preview shares everything up to the develop.
    p.render(&source, Size::Fit(128, 128)).unwrap();
    assert_eq!(ran(&p), ["resize", "rt_capture_sharpen", "ag_exposure_wb"]);
    p.set::<ag_exposure_wb::AgExposureWb>(exposure(-1.0))
        .unwrap();
    p.render(&source, Size::Fit(128, 128)).unwrap();
    assert_eq!(ran(&p), ["ag_exposure_wb"]);

    assert_eq!(p.runs("dt_highlights"), 2);
    assert_eq!(p.runs("develop"), 2);
}

#[test]
fn a_preview_is_the_size_asked_for_and_says_its_scale() {
    let mut p = pipeline(None);
    let source = common::source(512, 384);
    let preview = p.render(&source, Size::Fit(128, 128)).unwrap();
    let rgb = preview.as_rgb().unwrap();
    assert_eq!((rgb.width, rgb.height), (128, 96));
    assert_eq!(rgb.full_width, 512);
    assert_eq!(rgb.scale(), 0.25);
    let full = p.render(&source, Size::Full).unwrap();
    assert_eq!(full.as_rgb().unwrap().width, 512);
    assert_eq!(ran(&p), ["rt_capture_sharpen", "ag_exposure_wb"]);
}

/// North star 5: the cache is bounded. Least recently used results go, and
/// what is kept stays within the budget.
#[test]
fn the_cache_stays_within_its_budget() {
    let mut p = pipeline(None);
    let source = common::source(512, 384);
    // A developed 512 x 384 picture is 2.4 MB; keep room for one and a bit.
    let budget = 3 << 20;
    p.set_cache_budget_bytes(budget);
    for ev in [0.5, 1.0, 1.5, 2.0] {
        p.set::<ag_exposure_wb::AgExposureWb>(exposure(ev)).unwrap();
        p.render(&source, Size::Full).unwrap();
        assert!(p.cache_bytes() <= budget, "{} bytes", p.cache_bytes());
    }
    // And with the budget, what was evicted is made again when asked for.
    p.set::<ag_exposure_wb::AgExposureWb>(exposure(0.5))
        .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert!(!ran(&p).is_empty());
}

/// The same, on a real photo, with the time each step takes.
/// `cargo test --test rerun -- --nocapture` to see the table.
#[test]
fn reruns_timed_on_a_real_raw() {
    let Some(path) = common::raw("reruns_timed_on_a_real_raw") else {
        return;
    };
    let mut p = pipeline(common::gpu("reruns_timed_on_a_real_raw"));
    // A full-size render of a large photo and a preview of it, kept whole.
    p.set_cache_budget_bytes(6 << 30);

    let started = Instant::now();
    let source = common::open(&path);
    let decode = started.elapsed();
    let raw = &source.raw;
    eprintln!(
        "\n{}\n  {}x{}, {}, decoded in {:.0} ms",
        path.display(),
        raw.width,
        raw.height,
        if raw.cpp == 1 {
            "a mosaic"
        } else {
            "3 colours per pixel"
        },
        ms(decode)
    );

    let mut table = Vec::new();
    let mut step = |p: &mut Pipeline, label: &str, size: Size| {
        let started = Instant::now();
        p.render(&source, size).unwrap();
        let total = started.elapsed();
        let steps: Vec<String> = p
            .last_render()
            .iter()
            .filter(|s| s.ran)
            .map(|s| format!("{} {:.0}", s.name, ms(s.time)))
            .collect();
        eprintln!(
            "  {label:<34} {:>7.0} ms   ran: {}",
            ms(total),
            if steps.is_empty() {
                "nothing".to_string()
            } else {
                steps.join(", ")
            }
        );
        table.push((label.to_string(), ran(p), total));
    };

    step(&mut p, "full size, cold", Size::Full);
    step(&mut p, "full size, again", Size::Full);
    p.set::<ag_exposure_wb::AgExposureWb>(exposure(0.5))
        .unwrap();
    step(&mut p, "full size, exposure changed", Size::Full);
    let sharpening = p
        .get::<rt_capture_sharpen::RtCaptureSharpen>()
        .unwrap()
        .capture;
    p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
        capture: sharpening,
        capture_amount: 60.0,
        ..Default::default()
    })
    .unwrap();
    step(&mut p, "full size, sharpening changed", Size::Full);
    p.set::<dt_highlights::DtHighlights>(dt_highlights::Settings { recover: false })
        .unwrap();
    step(&mut p, "full size, highlights changed", Size::Full);
    step(&mut p, "preview 1600, after full", Size::Fit(1600, 1600));
    p.set::<ag_exposure_wb::AgExposureWb>(exposure(-0.5))
        .unwrap();
    step(
        &mut p,
        "preview 1600, exposure changed",
        Size::Fit(1600, 1600),
    );

    let expect: [&[&str]; 7] = [
        &[
            "dt_highlights",
            "develop",
            "rt_capture_sharpen",
            "ag_exposure_wb",
        ],
        &[],
        &["ag_exposure_wb"],
        &["rt_capture_sharpen", "ag_exposure_wb"],
        &[
            "dt_highlights",
            "develop",
            "rt_capture_sharpen",
            "ag_exposure_wb",
        ],
        &["resize", "rt_capture_sharpen", "ag_exposure_wb"],
        &["ag_exposure_wb"],
    ];
    for ((label, ran, _), expect) in table.iter().zip(expect) {
        assert_eq!(ran.as_slice(), expect, "{label}");
    }
    eprintln!(
        "  cache: {:.0} MB of a {:.0} MB budget",
        p.cache_bytes() as f64 / 1e6,
        (6u64 << 30) as f64 / 1e6
    );
    assert!(p.cache_bytes() <= 6 << 30);
    // Re-running only the last tool has to be much cheaper than the whole
    // pipeline - the point of keeping every result.
    let (cold, exposure_only) = (table[0].2, table[2].2);
    assert!(
        exposure_only * 4 < cold,
        "exposure alone took {:.0} ms of a {:.0} ms render",
        ms(exposure_only),
        ms(cold)
    );
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
