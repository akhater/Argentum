//! A preview that half the photo still covers develops at half size, and has
//! to look like the full-size develop made small - same size, crop, colour
//! and detail - and take less time than it.

mod common;

use argentum_engine::colour::{delta_e2000, lab};
use argentum_engine::rawler::imgop::{Dim2, Point, Rect};
use argentum_engine::rawler::rawimage::RawPhotometricInterpretation;
use argentum_engine::tools::{ag_dng_standard, rt_capture_sharpen};
use argentum_engine::{Pipeline, Rgb, Size, Source};

fn ran(p: &Pipeline) -> Vec<&'static str> {
    p.last_render()
        .iter()
        .filter(|s| s.ran)
        .map(|s| s.name)
        .collect()
}

/// `rgb` box-averaged to `w` x `h`.
fn made_small(rgb: &Rgb, w: usize, h: usize) -> Vec<[f32; 3]> {
    let span = |o: usize, out: usize, input: usize| {
        let a = o * input / out;
        a..((o + 1) * input / out).max(a + 1).min(input)
    };
    let mut out = Vec::with_capacity(w * h);
    for oy in 0..h {
        for ox in 0..w {
            let (ys, xs) = (span(oy, h, rgb.height), span(ox, w, rgb.width));
            let mut sum = [0.0f64; 3];
            for y in ys.clone() {
                for p in &rgb.pixels[y * rgb.width + xs.start..y * rgb.width + xs.end] {
                    (0..3).for_each(|c| sum[c] += p[c] as f64);
                }
            }
            let n = (ys.len() * xs.len()) as f64;
            out.push(sum.map(|s| (s / n) as f32));
        }
    }
    out
}

#[test]
fn a_half_size_preview_is_the_full_develop_made_small() {
    let mut cropped = common::scene(1280, 960);
    cropped.active_area = Some(Rect::new(Point::new(24, 16), Dim2::new(1232, 928)));
    cropped.crop_area = Some(Rect::new(Point::new(40, 28), Dim2::new(1180, 880)));
    // An active area starting on an odd photosite: the mosaic's pattern
    // shifts under it.
    let mut odd = common::scene(1280, 960);
    odd.active_area = Some(Rect::new(Point::new(13, 9), Dim2::new(1250, 940)));
    let mut sources = vec![
        (
            "synthetic".to_string(),
            common::source(1280, 960),
            (300, 300),
        ),
        (
            "synthetic, cropped".to_string(),
            Source::new(7, cropped),
            (300, 300),
        ),
        (
            "synthetic, odd active area".to_string(),
            Source::new(8, odd),
            (300, 300),
        ),
    ];
    for path in common::raws("a_half_size_preview_is_the_full_develop_made_small") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        sources.push((name, common::open(&path), (1600, 1600)));
    }

    for (name, source, (max_w, max_h)) in sources {
        let bayer = source.raw.cpp == 1
            && matches!(&source.raw.photometric, RawPhotometricInterpretation::Cfa(c) if c.cfa.width == 2);
        if !bayer {
            eprintln!("  {name:<40} not a Bayer mosaic: it develops at full size");
            continue;
        }
        // Each develop timed from a cold cache, the fastest of three.
        let (mut full_s, mut half_s) = (f64::MAX, f64::MAX);
        let mut pictures = None;
        for _ in 0..RUNS {
            // The develop alone: white balance at the photo's own, which
            // changes nothing, and no sharpening or DNG Standard after it.
            let mut p = Pipeline::standard().unwrap();
            let defaults = p.registry().defaults_for(&source).unwrap();
            p.set_edit(&defaults).unwrap();
            p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
                capture: false,
                ..Default::default()
            })
            .unwrap();
            p.set::<ag_dng_standard::AgDngStandard>(ag_dng_standard::Settings {
                enabled: false,
                ..Default::default()
            })
            .unwrap();
            let full = p.render(&source, Size::Full).unwrap();
            full_s = full_s.min(step_seconds(&p, "develop"));
            let preview = p.render(&source, Size::Fit(max_w, max_h)).unwrap();
            assert!(
                ran(&p).contains(&"develop_half") && !ran(&p).contains(&"develop"),
                "{name}: {:?}",
                ran(&p)
            );
            half_s = half_s.min(step_seconds(&p, "develop_half"));
            pictures.get_or_insert((full, preview));
        }
        let (full, preview) = pictures.unwrap();
        let (full, preview) = (full.as_rgb().unwrap(), preview.as_rgb().unwrap());
        let fits = |v: usize| {
            v as f64 * (max_w as f64 / full.width as f64).min(max_h as f64 / full.height as f64)
        };
        assert_eq!(
            (preview.width, preview.height),
            (
                fits(full.width).round() as usize,
                fits(full.height).round() as usize
            ),
            "{name}: the size the full develop would fit at"
        );
        assert_eq!(preview.full_width, full.width, "{name}");

        // Against the full develop box-averaged to the same size, by
        // RAWmakase's gates for its own half-size develop
        // (`crates/rawmakase-decode/src/half_check.rs` @ 183e09fd,
        // pch/rawmakase#526, MIT): colour as CIEDE2000 between blocks' mean
        // colours, detail as the energy of the luminance Laplacian, and time
        // against the full develop. Here also: the crop, as the shift that
        // matches best, and the colour's shift over the whole photo.
        // The full develop made small the way the preview is: first to the
        // half-size develop's size, each pixel the 2 x 2 block under it (as
        // RAWmakase's `half_check.rs` reduces it), then by the same box
        // filter. Straight from full size, the filter's whole-pixel spans
        // (4 or 5 full-size pixels, against 2 or 3 half-size ones) fall elsewhere,
        // and on fine backlit branches that alone moved the colour by 0.20
        // (mean) and 0.65 (95th) dE2000 with the develops not compared at all.
        let (half_w, half_h) = (full.width / 2, full.height / 2);
        let halved = Rgb::new(half_w, half_h, made_small(full, half_w, half_h));
        let small = made_small(&halved, preview.width, preview.height);
        let shift = best_shift(&preview.pixels, &small, preview.width, preview.height);
        let blocks = |v: &[[f32; 3]]| {
            let rgb = Rgb::new(preview.width, preview.height, v.to_vec());
            made_small(&rgb, preview.width / BLOCK, preview.height / BLOCK)
        };
        let (ours, theirs) = (blocks(&preview.pixels), blocks(&small));
        let mut de: Vec<f64> = ours
            .iter()
            .zip(&theirs)
            .map(|(a, b)| delta_e2000(lab(*a), lab(*b)))
            .collect();
        de.sort_by(f64::total_cmp);
        let de_mean = de.iter().sum::<f64>() / de.len() as f64;
        let de_p95 = de[((de.len() as f64 * 0.95) as usize).min(de.len() - 1)];
        let mut cast = [0.0f64; 3];
        for (a, b) in ours.iter().zip(&theirs) {
            let (a, b) = (lab(*a), lab(*b));
            (0..3).for_each(|c| cast[c] += (a[c] - b[c]) / ours.len() as f64);
        }
        let cast = cast.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let detail = laplacian_energy(&preview.pixels, preview.width, preview.height)
            / laplacian_energy(&small, preview.width, preview.height);
        eprintln!(
            "  {name:<40} {} x {}: shift {shift:?}, colour {de_mean:.3} (mean) {de_p95:.3} (95th) dE2000, cast {cast:.3}, detail {detail:.3}, develop {:.0} ms half, {:.0} ms full",
            preview.width,
            preview.height,
            half_s * 1000.0,
            full_s * 1000.0
        );
        assert_eq!(shift, (0, 0), "{name}: the crop moved");
        assert!(
            de_mean <= MAX_MEAN && de_p95 <= MAX_P95,
            "{name}: the colour moved"
        );
        assert!(cast <= MAX_CAST, "{name}: the colour shifted");
        assert!(DETAIL.contains(&detail), "{name}: the detail changed");
        assert!(
            half_s <= MAX_TIME * full_s + TIME_SLACK,
            "{name}: the half-size develop is no faster"
        );
    }
}

// RAWmakase's gates (`half_check.rs` @ 183e09fd): a file is held to its own
// reference plus 0.1, or without one to 0.3 (mean) and 1.0 (95th). Here every
// source is held to the second pair, over blocks of 32 preview pixels.
//
// Not RAWmakase's 16 half-size pixels (8 preview pixels here): at that size
// the colour gate measured the two develops' fine texture, not their colour.
// The half-size develop is resized to the preview by the engine and the full
// one box-averaged here, so a block of 8 holds two different textures of the
// same detail and noise. Fitted to one photo (a 32 MP CR3: 0.456 and 1.047 on
// 2026-10-19, plus 0.1), the gate failed a noisier R6 Mark III frame at 0.406
// and 1.216 with no colour shift at all (cast 0.045). Blocks of 32 average the
// texture out and keep the colour: on 2026-10-25, eight photos of five bodies
// (R6 Mark III, R5 Mark II, Z 9, ILCE-7M4, K-5 II s) measured at most 0.247 and
// 0.665; a colour shift still moves every block, and `MAX_CAST` holds the
// photo's mean.
const BLOCK: usize = 32;
const MAX_MEAN: f64 = 0.3;
const MAX_P95: f64 = 1.0;
/// The most the photo's mean L*, a* or b* may move: measured 0.08 at most on
/// 2026-10-19, 0.116 on an ILCE-7M4 frame on 2026-10-25.
const MAX_CAST: f64 = 0.2;
const DETAIL: std::ops::RangeInclusive<f64> = 0.8..=1.25;
const MAX_TIME: f64 = 1.1;
const TIME_SLACK: f64 = 0.02;
const RUNS: usize = 3;

fn step_seconds(p: &Pipeline, name: &str) -> f64 {
    p.last_render()
        .iter()
        .find(|s| s.name == name && s.ran)
        .map_or(f64::MAX, |s| s.time.as_secs_f64())
}

/// The mean squared luminance Laplacian: how much fine detail there is.
fn laplacian_energy(v: &[[f32; 3]], w: usize, h: usize) -> f64 {
    let luma = |p: [f32; 3]| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
    let (mut e, mut n) = (0.0, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let k = y * w + x;
            let l = 4.0 * luma(v[k])
                - luma(v[k - 1])
                - luma(v[k + 1])
                - luma(v[k - w])
                - luma(v[k + w]);
            e += l * l;
            n += 1.0;
        }
    }
    e / n
}

/// The shift of `a` against `b`, within three pixels, that matches best.
fn best_shift(a: &[[f32; 3]], b: &[[f32; 3]], w: usize, h: usize) -> (i64, i64) {
    let (w, h) = (w as i64, h as i64);
    let mut best = (f64::MAX, (0, 0));
    for dy in -3i64..=3 {
        for dx in -3i64..=3 {
            let mut d = 0.0f64;
            for y in 4..h - 4 {
                for x in 4..w - 4 {
                    let (p, q) = (a[(y * w + x) as usize], b[((y + dy) * w + x + dx) as usize]);
                    d += (0..3).map(|c| (p[c] - q[c]).abs() as f64).sum::<f64>();
                }
            }
            if d < best.0 {
                best = (d, (dx, dy));
            }
        }
    }
    best.1
}
