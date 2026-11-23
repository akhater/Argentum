//! How long an editor render may take: the budgets
//! `sharpen_gpu::tests::where_the_time_goes` holds every row to. Ours.
//!
//! WHERE THE NUMBERS COME FROM
//!
//! One laptop: Intel Core Ultra 9 288V with its Arc 140V graphics, driver
//! 32.0.101.6042, on mains power. Measured on 2026-10-21 with main at
//! 4d559684, in the test profile (plain `cargo test`, no `--release`).
//!
//! The test runs every row three times, each round on a device of its own,
//! and keeps the fastest: noise only ever adds time, and a slowdown slows all
//! three. Ten runs of the test gave each row's median and slowest, and its
//! budget is the largest of 1.5 x the median, the median + 5 ms and 1.2 x the
//! slowest, rounded up to a whole ms. The last term is for a row that takes
//! one of two speeds: DirectX 12's first mask view at 6960 px is 119 ms in
//! most runs and 175 in some.
//!
//! "look grain" (their render with a provider look's grain on, look_grain.rs)
//! was measured the same way on 2026-11-04, ten runs on mains power.
//!
//! "noise reduction moved" (its pass before sharpening, sharpening again and
//! their render: a tick of its slider at fit, its release at 100%) and "noise
//! reduction drag" (a tick of its drag at 100%, a region, which leaves it to
//! their shader; input_stage.rs) were measured the same way on 2026-11-07, ten
//! runs on mains power, in a pass of their own after the other rows.
//!
//! A slower GPU fails them, so the test stays `#[ignore]`d and is run by hand
//! on that laptop before every release (docs/RELEASING.md). Anywhere else,
//! `AG_SPEED_BUDGETS=report` prints the rows over budget without failing.
//!
//! NEVER WORSE
//!
//! A row over its budget is a slowdown to fix, not a number to raise. When a
//! change makes a row faster, measure again and lower its budget: the test
//! prints what it measured as lines of this table.

#[path = "../../engine/tests/common/full_speed.rs"]
pub mod full_speed;

/// What `where_the_time_goes` times, in this order everywhere below.
pub const ROWS: [&str; 13] = [
    "theirs alone",
    "measure first",
    "measure again",
    "sharpening first",
    "sharpening kept",
    "mask view on",
    "mask view off",
    "pan step",
    "two eye masks",
    "whole-frame mask",
    "look grain",
    "noise reduction moved",
    "noise reduction drag",
];

/// Milliseconds per row of `ROWS`, by backend and photo size.
#[rustfmt::skip]
const BUDGETS_MS: [(&str, [u32; 2], [u32; ROWS.len()]); 8] = [
    //                            their  measure    sharpening   mask view    pan   masks:              noise red.
    //                            alone  1st again  1st   kept   on    off    step  eyes  frame  grain  moved  drag
    ("Auto",       [3328, 2219], [  41,   36,  5,   197,   23,   21,   21,   21,   213,  224,   54,   197,     9]),
    ("Auto",       [6960, 4640], [ 186,  150,  5,   929,  107,  159,  186,   93,   909,  962,  219,   857,    21]),
    ("DirectX 12", [3328, 2219], [  32,   36,  5,   462,   26,   27,   24,   24,   218,  234,   52,   233,     9]),
    ("DirectX 12", [6960, 4640], [ 162,  152,  5,   962,  119,  210,  197,  105,   963, 1023,  198,  1003,    23]),
    ("OpenGL",     [3328, 2219], [  75,   36,  5,   290,   44,   54,   39,   41,   254,  284,   54,   243,    10]),
    ("OpenGL",     [6960, 4640], [ 342,  152,  5,  1047,  165,  239,  167,  165,  1089, 1193,  230,  1013,    26]),
    ("Vulkan",     [3328, 2219], [  38,   36,  5,   207,   23,   21,   21,   21,   218,  225,   56,   196,     9]),
    ("Vulkan",     [6960, 4640], [ 174,  152,  5,   906,  105,  158,  189,   93,   906,  965,  254,   864,    21]),
];

/// RapidRAW's demosaic on a 32 MP mosaic, all threads, the quickest of five
/// (`demosaic_measure::demosaic_within_budget`), by pattern: what a new
/// edit's decode spends demosaicing, on the CPU. Measured on 2026-11-22 on
/// the same laptop, three runs: Bayer 570-575 ms, X-Trans 1595-1698 ms
/// (rawler's own: 83-88 and 422-442 ms); each budget by the rule above.
pub const DEMOSAIC_MS: [(&str, u32); 2] = [("Bayer", 861), ("X-Trans", 2454)];

/// The line for a demosaic over its budget, or none.
pub fn demosaic_breach(pattern: &str, measured_ms: f32) -> Option<String> {
    let budget = DEMOSAIC_MS
        .iter()
        .find(|(name, _)| *name == pattern)
        .map_or(0, |(_, ms)| *ms);
    (measured_ms > budget as f32)
        .then(|| format!("demosaic {pattern}: {measured_ms:.0} ms, budget {budget} ms"))
}

/// Each row's fastest round.
pub fn fastest_ms(rounds: &[[f32; ROWS.len()]]) -> [f32; ROWS.len()] {
    std::array::from_fn(|row| {
        rounds
            .iter()
            .map(|round| round[row])
            .fold(f32::INFINITY, f32::min)
    })
}

/// One line for each row over its budget, or with no budget at all.
pub fn breaches(backend: &str, size: [u32; 2], measured_ms: &[f32; ROWS.len()]) -> Vec<String> {
    let [w, h] = size;
    let Some((.., budget_ms)) = BUDGETS_MS
        .iter()
        .find(|(b, s, _)| *b == backend && *s == size)
    else {
        return vec![format!("{backend} {w}x{h}: no budgets")];
    };
    ROWS.iter()
        .zip(measured_ms.iter().zip(budget_ms))
        .filter(|(_, (ms, budget))| **ms > **budget as f32)
        .map(|(row, (ms, budget))| {
            format!("{backend} {w}x{h} {row}: {ms:.0} ms, budget {budget} ms")
        })
        .collect()
}

/// A table line of what was measured, to lower budgets from.
pub fn table_line(backend: &str, size: [u32; 2], measured_ms: &[f32; ROWS.len()]) -> String {
    let ms: Vec<String> = measured_ms
        .iter()
        .map(|ms| format!("{:5.0}", ms.ceil()))
        .collect();
    format!(
        "{:<15}{size:?}, [{}]),",
        format!("(\"{backend}\","),
        ms.join(",")
    )
}

/// Fail on any breach, unless `AG_SPEED_BUDGETS=report` says these budgets
/// are not this machine's.
pub fn assert_none(breaches: &[String]) {
    if breaches.is_empty() {
        eprintln!("speed: every row within its budget");
        return;
    }
    let lines = breaches.join("\n");
    if std::env::var("AG_SPEED_BUDGETS").as_deref() == Ok("report") {
        eprintln!("speed, report only:\n{lines}");
    } else {
        panic!("over budget:\n{lines}");
    }
}

/// Windows takes the request and keeps it, so a timing test that makes it
/// runs on every core with any window in front.
#[cfg(windows)]
#[test]
fn windows_keeps_a_timing_test_at_full_speed() {
    full_speed::hold();
    assert!(full_speed::held());
}

/// The Lensfun database parse (`lens_correction::read_lensfun_dir` on the
/// bundled `lensfun_db`, 59 files, 5.2 MB of XML): the heaviest step of
/// start-up that needs no app handle, the quickest of five. Measured on
/// 2026-11-23 on the same laptop, seven runs on mains power: 40-59 ms, median
/// 45; the budget by the rule above.
pub const LENS_PARSE_MS: u32 = 71;

/// The line for a lens database parse over its budget, or none.
pub fn lens_parse_breach(measured_ms: f32) -> Option<String> {
    (measured_ms > LENS_PARSE_MS as f32)
        .then(|| format!("lens database parse: {measured_ms:.0} ms, budget {LENS_PARSE_MS} ms"))
}

/// The lens database parse against its budget, the quickest of five. Run by
/// hand on the laptop the budgets come from (docs/RELEASING.md). Start-up
/// reads this database before the window is built.
#[test]
#[ignore = "timing against the reference laptop's budgets; run by hand"]
fn lens_parse_within_budget() {
    full_speed::hold();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lensfun_db");
    let quickest = (0..5)
        .map(|_| {
            let start = std::time::Instant::now();
            let db = crate::lens_correction::read_lensfun_dir(&dir);
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            assert!(!db.lenses.is_empty(), "no lenses read from {dir:?}");
            ms
        })
        .fold(f32::MAX, f32::min);
    println!("lens database parse: {quickest:.0} ms");
    assert_none(&lens_parse_breach(quickest).into_iter().collect::<Vec<_>>());
}
