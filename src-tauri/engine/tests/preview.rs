//! A preview that half the photo still covers develops at half size, and has
//! to look like the full-size develop made small - same size, crop, colour
//! and detail - and take less time than it.

mod common;

use argentum_engine::rawler::imgop::{Dim2, Point, Rect};
use argentum_engine::rawler::rawimage::RawPhotometricInterpretation;
use argentum_engine::tools::rt_capture_sharpen;
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
    // Each with the colour limits it is held to: mean and 95th percentile.
    let synthetic = (SYNTHETIC_MEAN, SYNTHETIC_P95);
    let mut sources = vec![
        (
            "synthetic".to_string(),
            common::source(1280, 960),
            (300, 300),
            synthetic,
        ),
        (
            "synthetic, cropped".to_string(),
            Source::new(7, cropped),
            (300, 300),
            synthetic,
        ),
        (
            "synthetic, odd active area".to_string(),
            Source::new(8, odd),
            (300, 300),
            synthetic,
        ),
    ];
    for path in common::raws("a_half_size_preview_is_the_full_develop_made_small") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        sources.push((
            name,
            common::open(&path),
            (1600, 1600),
            (PHOTO_MEAN, PHOTO_P95),
        ));
    }

    for (name, source, (max_w, max_h), (max_mean, max_p95)) in sources {
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
            let mut p = Pipeline::standard().unwrap();
            p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
                capture: false,
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
        let small = made_small(full, preview.width, preview.height);
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
            de_mean <= max_mean && de_p95 <= max_p95,
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

// RAWmakase's gates (`half_check.rs` @ 183e09fd): blocks of 16 half-size
// pixels, here 8 preview pixels, about as many photosites. A file is held to
// its own reference plus 0.1, or without one to 0.3 (mean) and 1.0 (95th).
const BLOCK: usize = 8;
const SYNTHETIC_MEAN: f64 = 0.3;
const SYNTHETIC_P95: f64 = 1.0;
/// A real photo's reference: a 32 MP CR3 measured 0.456 and 1.047 on
/// 2026-10-19, all of it fine-scale - blocks of 16 halve it and of 32 take it
/// to a third, the demosaics and the noise differing within a block - and
/// none of it a colour shift (`MAX_CAST`).
const PHOTO_MEAN: f64 = 0.456 + 0.1;
const PHOTO_P95: f64 = 1.047 + 0.1;
/// The most the photo's mean L*, a* or b* may move: measured 0.08 at most.
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

// Copied from RAWmakase `crates/rawmakase-decode/src/half_check.rs`
// @ 183e09fd (pch/rawmakase#526, MIT): `lab` and `delta_e2000`, on f32
// pixels here.
/// CIELAB (D65) of linear sRGB.
fn lab([r, g, b]: [f32; 3]) -> [f64; 3] {
    let [r, g, b] = [r as f64, g as f64, b as f64];
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (24389. / 27. * t + 16.) / 116.
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116. * fy - 16., 500. * (fx - fy), 200. * (fy - fz)]
}

/// CIEDE2000 colour difference (Sharma, Wu and Dalal 2005).
fn delta_e2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.;
    let g = 0.5 * (1. - (c_bar.powi(7) / (c_bar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1. + g) * a1, (1. + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if a == 0. && b == 0. {
            0.
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.)
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0. {
        0.
    } else if (h2p - h1p).abs() <= 180. {
        h2p - h1p
    } else if h2p - h1p > 180. {
        h2p - h1p - 360.
    } else {
        h2p - h1p + 360.
    };
    let dhh = 2. * (c1p * c2p).sqrt() * (dh / 2.).to_radians().sin();
    let l_bar = (l1 + l2) / 2.;
    let cp_bar = (c1p + c2p) / 2.;
    let hp_bar = if c1p * c2p == 0. {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180. {
        (h1p + h2p) / 2.
    } else if h1p + h2p < 360. {
        (h1p + h2p + 360.) / 2.
    } else {
        (h1p + h2p - 360.) / 2.
    };
    let t = 1. - 0.17 * (hp_bar - 30.).to_radians().cos()
        + 0.24 * (2. * hp_bar).to_radians().cos()
        + 0.32 * (3. * hp_bar + 6.).to_radians().cos()
        - 0.20 * (4. * hp_bar - 63.).to_radians().cos();
    let d_theta = 30. * (-((hp_bar - 275.) / 25.).powi(2)).exp();
    let rc = 2. * (cp_bar.powi(7) / (cp_bar.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1. + 0.015 * (l_bar - 50.).powi(2) / (20. + (l_bar - 50.).powi(2)).sqrt();
    let sc = 1. + 0.045 * cp_bar;
    let sh = 1. + 0.015 * cp_bar * t;
    let rt = -(2. * d_theta).to_radians().sin() * rc;
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dhh / sh).powi(2) + rt * (dc / sc) * (dhh / sh))
        .sqrt()
}

#[test]
fn ciede2000_matches_published_pairs() {
    // Sharma, Wu and Dalal's test data, pairs 1, 7 and 17 (RAWmakase's test).
    for (a, b, want) in [
        ([50., 2.6772, -79.7751], [50., 0., -82.7485], 2.0425),
        ([50., 0., 0.], [50., -1., 2.], 2.3669),
        ([50., 2.5, 0.], [73., 25., -18.], 27.1492),
    ] {
        let got = delta_e2000(a, b);
        assert!((got - want).abs() < 1e-4, "{got} for {want}");
    }
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
