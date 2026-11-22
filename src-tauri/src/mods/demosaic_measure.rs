//! Measurements behind the demosaic rule (`demosaic_rule.rs`), run by hand.
//! None of it runs in CI: every test here reads local photos.
//!
//! `AG_RAWS` is a `;`-separated list of Bayer RAWs. The decode's own hashes
//! and times are `highlight_pull_measure::decode_hashes` and `decode_time`.

use std::path::PathBuf;

use argentum_engine::colour::{delta_e2000, lab, luma};
use argentum_engine::rr_demosaic::Mosaic;
use rawler::cfa::CFA;
use rawler::imgop::sensor::bayer::{Demosaic, ppg::PPGDemosaic};
use rawler::imgop::sensor::xtrans::demosaic::XTransDemosaic;
use rawler::imgop::{Dim2, Point, Rect};
use rawler::pixarray::PixF32;
use rawler::rawimage::{
    BlackLevel, CFAConfig, RawImage, RawImageData, RawPhotometricInterpretation,
};

use super::highlight_pull_measure::decoded;

fn raws() -> Vec<PathBuf> {
    std::env::var("AG_RAWS")
        .expect("set AG_RAWS")
        .split(';')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Fujifilm's X-Trans layout (X-T1 and after), as dcraw lists it.
const XTRANS: &str = "GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG";

/// What a mosaic's samples mean: the photo's levels, its white balance
/// with green at 1, and its colours.
struct Photo {
    raw: RawImage,
    black: f32,
    white: f32,
    wb: [f32; 3],
}

fn photo(path: &PathBuf) -> Photo {
    let bytes = std::fs::read(path).expect("read");
    let raw = decoded(&bytes);
    let black = raw.blacklevel.as_bayer_array()[0];
    let white = *raw.whitelevel.0.first().expect("white") as f32;
    let wb = [
        raw.wb_coeffs[0] / raw.wb_coeffs[1],
        1.0,
        raw.wb_coeffs[2] / raw.wb_coeffs[1],
    ];
    Photo {
        raw,
        black,
        white,
        wb,
    }
}

fn samples(raw: &RawImage) -> Vec<f32> {
    match &raw.data {
        RawImageData::Integer(v) => v.iter().map(|&s| s as f32).collect(),
        RawImageData::Float(v) => v.clone(),
    }
}

/// The photo's mosaic binned quad by quad into one true-colour pixel (red,
/// the two greens' mean, blue), black at 0 and white at 1, not balanced: a
/// picture with no interpolation in it, the truth a re-mosaic of it is
/// judged against.
fn truth(p: &Photo) -> (Vec<[f32; 3]>, usize, usize) {
    let raw = &p.raw;
    let area = raw
        .active_area
        .unwrap_or(Rect::new(Point::zero(), raw.dim()));
    let data = samples(raw);
    let (w, h) = (area.d.w / 2, area.d.h / 2);
    let scale = 1.0 / (p.white - p.black);
    let mut out = vec![[0.0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 3];
            let mut n = [0.0f32; 3];
            for dy in 0..2 {
                for dx in 0..2 {
                    let (row, col) = (area.p.y + 2 * y + dy, area.p.x + 2 * x + dx);
                    let c = raw.camera.cfa.color_at(row, col).min(2);
                    sum[c] += (data[row * raw.width + col] - p.black) * scale;
                    n[c] += 1.0;
                }
            }
            out[y * w + x] = std::array::from_fn(|c| sum[c] / n[c].max(1.0));
        }
    }
    (out, w, h)
}

/// `truth` sampled through `cfa`.
fn mosaic(truth: &[[f32; 3]], w: usize, h: usize, cfa: &CFA) -> Vec<f32> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| truth[y * w + x][cfa.color_at(y, x).min(2)])
        .collect()
}

/// rawler's demosaic of a 0..1 mosaic: PPG on Bayer, its own on X-Trans.
fn rawlers(m: &[f32], w: usize, h: usize, cfa: &CFA, p: &Photo) -> Vec<[f32; 3]> {
    let RawPhotometricInterpretation::Cfa(config) = &p.raw.photometric else {
        panic!("not a mosaic")
    };
    let pixels = PixF32::new_with(m.to_vec(), w, h);
    let roi = Rect::new(Point::zero(), Dim2::new(w, h));
    let out = if cfa.width == 6 {
        XTransDemosaic::new().demosaic(&pixels, cfa, &config.colors, roi)
    } else {
        PPGDemosaic::new().demosaic(&pixels, cfa, &config.colors, roi)
    };
    out.data.iter().map(|px| px.map(|v| v.max(0.0))).collect()
}

/// RapidRAW's demosaic of a 0..1 mosaic, balanced by the photo's white
/// balance as it runs, then unbalanced again.
fn rapidraws(m: &[f32], w: usize, h: usize, cfa: &CFA, p: &Photo) -> Vec<[f32; 3]> {
    let RawPhotometricInterpretation::Cfa(config) = &p.raw.photometric else {
        panic!("not a mosaic")
    };
    let mut raw = p.raw.clone();
    raw.data = RawImageData::Float(m.to_vec());
    raw.width = w;
    raw.height = h;
    raw.cpp = 1;
    raw.active_area = None;
    raw.crop_area = None;
    raw.blacklevel = BlackLevel::new(&[0u32], 1, 1, 1);
    raw.photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new(cfa, &config.colors));
    let mosaic = Mosaic::new(&raw, p.wb, 1.0).expect("RapidRAW's demosaic takes it");
    mosaic
        .develop()
        .into_iter()
        .map(|px| std::array::from_fn(|c| px[c] / p.wb[c]))
        .collect()
}

/// The study's measures of `test` against `truth`: both white balanced,
/// read as linear sRGB and brought to a mean luminance of 0.18; 16 px of
/// border left out. CIEDE2000 mean, 99th percentile and share above 5, and
/// CPSNR in gamma 2.2.
fn score(test: &[[f32; 3]], truth: &[[f32; 3]], w: usize, h: usize, wb: [f32; 3]) -> [f64; 4] {
    let bal = |px: [f32; 3]| -> [f32; 3] { std::array::from_fn(|c| px[c] * wb[c]) };
    let inner = || (16..h - 16).flat_map(move |y| (16..w - 16).map(move |x| y * w + x));
    let n = inner().count() as f64;
    let mean_luma: f64 = inner().map(|i| luma(bal(truth[i])) as f64).sum::<f64>() / n;
    let k = (0.18 / mean_luma) as f32;
    let lin = |px: [f32; 3]| -> [f32; 3] { bal(px).map(|v| (v * k).max(0.0)) };
    let mut des: Vec<f64> = inner()
        .map(|i| delta_e2000(lab(lin(test[i])), lab(lin(truth[i]))))
        .collect();
    let mse: f64 = inner()
        .map(|i| {
            let (a, b) = (lin(test[i]), lin(truth[i]));
            (0..3)
                .map(|c| {
                    let g = |v: f32| (v.min(1.0) as f64).powf(1.0 / 2.2);
                    (g(a[c]) - g(b[c])).powi(2)
                })
                .sum::<f64>()
        })
        .sum::<f64>()
        / (3.0 * n);
    des.sort_by(f64::total_cmp);
    let mean = des.iter().sum::<f64>() / n;
    let p99 = des[((des.len() - 1) as f64 * 0.99) as usize];
    let over5 = des.iter().filter(|&&d| d > 5.0).count() as f64 / n;
    [mean, p99, over5 * 100.0, 10.0 * (1.0 / mse).log10()]
}

fn line(name: &str, s: [f64; 4]) -> String {
    format!(
        "{name}: mean {:.2}, p99 {:.2}, > 5 {:.2}%, CPSNR {:.2} dB",
        s[0], s[1], s[2], s[3]
    )
}

/// Each photo's binned truth, re-mosaiced as Bayer and as X-Trans and
/// demosaiced by rawler's and by RapidRAW's: the study's measure, so the
/// port's numbers can be held against its run of RapidRAW's own shader.
#[test]
#[ignore = "reads local photos; run by hand"]
fn against_the_truth() {
    for path in raws() {
        let p = photo(&path);
        let (truth, w, h) = truth(&p);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (label, cfa) in [("Bayer", CFA::new("RGGB")), ("X-Trans", CFA::new(XTRANS))] {
            let m = mosaic(&truth, w, h, &cfa);
            let ours = score(&rawlers(&m, w, h, &cfa, &p), &truth, w, h, p.wb);
            let theirs = score(&rapidraws(&m, w, h, &cfa, &p), &truth, w, h, p.wb);
            println!("{name} {label} {w}x{h}");
            println!("  {}", line("rawler   ", ours));
            println!("  {}", line("RapidRAW ", theirs));
        }
    }
}

/// The green checkerboard (what dbb84900 takes out): in flat, bright, unclipped
/// 16 px tiles of each photo's own mosaic, the green plane's amplitude at
/// (π, π) over its mean, rawler's PPG against RapidRAW's. A flat tile is one
/// whose 2 x 2-binned green (which the checkerboard cancels out of) varies by
/// under 2% of its mean.
#[test]
#[ignore = "reads local photos; run by hand"]
fn green_checkerboard() {
    for path in raws() {
        let p = photo(&path);
        let raw = &p.raw;
        let area = raw
            .active_area
            .unwrap_or(Rect::new(Point::zero(), raw.dim()));
        let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
            panic!("not a mosaic")
        };
        let data = samples(raw);
        let scale = 1.0 / (p.white - p.black);
        let (w, h) = (area.d.w, area.d.h);
        let m: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| (data[(area.p.y + y) * raw.width + area.p.x + x] - p.black) * scale)
            .collect();
        let cfa = config.cfa.shift(area.p.x, area.p.y);
        let ppg: Vec<f32> = rawlers(&m, w, h, &cfa, &p).iter().map(|px| px[1]).collect();
        let rr = Mosaic::new(raw, p.wb, p.white).expect("RapidRAW's demosaic takes it");
        let rr: Vec<f32> = rr.develop().iter().map(|px| px[1]).collect();

        const T: usize = 16;
        let mut amps = [Vec::new(), Vec::new()];
        for ty in (32..h - 32 - T).step_by(T) {
            for tx in (32..w - 32 - T).step_by(T) {
                let at = |g: &[f32], x: usize, y: usize| g[(ty + y) * w + tx + x];
                // Flat, bright and unclipped by the PPG green (the
                // checkerboard cancels out of a 2 x 2 bin either way).
                let bins: Vec<f32> = (0..T / 2)
                    .flat_map(|y| (0..T / 2).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        (at(&ppg, 2 * x, 2 * y)
                            + at(&ppg, 2 * x + 1, 2 * y)
                            + at(&ppg, 2 * x, 2 * y + 1)
                            + at(&ppg, 2 * x + 1, 2 * y + 1))
                            / 4.0
                    })
                    .collect();
                let mean = bins.iter().sum::<f32>() / bins.len() as f32;
                let sd = (bins.iter().map(|b| (b - mean).powi(2)).sum::<f32>() / bins.len() as f32)
                    .sqrt();
                let peak = (0..T * T)
                    .map(|i| at(&ppg, i % T, i / T))
                    .fold(0.0f32, f32::max);
                if !(0.15..=0.8).contains(&mean) || peak > 0.95 || sd > 0.02 * mean {
                    continue;
                }
                for (slot, g) in [&ppg, &rr].into_iter().enumerate() {
                    let mut nyquist = 0.0f32;
                    let mut sum = 0.0f32;
                    for y in 0..T {
                        for x in 0..T {
                            let v = at(g, x, y);
                            nyquist += if (x + y) % 2 == 0 { v } else { -v };
                            sum += v;
                        }
                    }
                    amps[slot].push((nyquist / sum).abs() as f64);
                }
            }
        }
        let pick = |v: &mut Vec<f64>, q: f64| {
            v.sort_by(f64::total_cmp);
            v[((v.len() - 1) as f64 * q) as usize]
        };
        let tiles = amps[0].len();
        let [mut a, mut b] = amps;
        println!(
            "{}: {tiles} flat bright tiles; (π,π) amplitude / mean, median and p95: rawler {:.2e} {:.2e}, RapidRAW {:.2e} {:.2e}",
            path.file_name().unwrap().to_string_lossy(),
            pick(&mut a, 0.5),
            pick(&mut a, 0.95),
            pick(&mut b, 0.5),
            pick(&mut b, 0.95),
        );
    }
}

/// The demosaic alone on each photo's own mosaic, rawler's PPG against
/// RapidRAW's, the quickest of five after one warm-up.
#[test]
#[ignore = "reads local photos; timing, run by hand"]
fn demosaic_time() {
    for path in raws() {
        let p = photo(&path);
        let raw = &p.raw;
        let area = raw
            .active_area
            .unwrap_or(Rect::new(Point::zero(), raw.dim()));
        let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
            panic!("not a mosaic")
        };
        let pixels = PixF32::new_with(samples(raw), raw.width, raw.height);
        let mut quickest = [f64::MAX; 2];
        for round in 0..6 {
            let start = std::time::Instant::now();
            let out = PPGDemosaic::new().demosaic(&pixels, &config.cfa, &config.colors, area);
            drop(out);
            let ppg = start.elapsed().as_secs_f64() * 1000.0;
            let start = std::time::Instant::now();
            let out = Mosaic::new(raw, p.wb, p.white).expect("takes it").develop();
            drop(out);
            let rr = start.elapsed().as_secs_f64() * 1000.0;
            if round > 0 {
                quickest = [quickest[0].min(ppg), quickest[1].min(rr)];
            }
        }
        println!(
            "{} {}x{}: PPG {:.0} ms, RapidRAW {:.0} ms",
            path.file_name().unwrap().to_string_lossy(),
            area.d.w,
            area.d.h,
            quickest[0],
            quickest[1]
        );
    }
}

/// A 32 MP mosaic (6982 x 4654, an EOS R6 Mark III's) through `cfa`: a smooth
/// coloured scene with fine texture and noise, in 14-bit samples.
fn synthetic(cfa: &str) -> RawImage {
    let (w, h) = (6982usize, 4654usize);
    let cfa = CFA::new(cfa);
    let data: Vec<u16> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let (fx, fy) = (x as f32 / 97.0, y as f32 / 71.0);
            let fine = 0.05 * ((x as f32 * 0.9).sin() * (y as f32 * 1.3).cos());
            let noise = ((x * 7919 + y * 104729) % 997) as f32 / 997.0 * 0.01;
            let rgb = [
                0.30 + 0.20 * fx.sin(),
                0.40 + 0.15 * (fx + fy).cos(),
                0.25 + 0.10 * fy.sin(),
            ];
            let v = rgb[cfa.color_at(y, x).min(2)] + fine + noise;
            (512.0 + v * 13000.0) as u16
        })
        .collect();
    RawImage::new(
        rawler::decoders::Camera::new(),
        rawler::pixarray::PixU16::new_with(data, w, h),
        1,
        [2.0, 1.0, 1.5, f32::NAN],
        RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &Default::default())),
        Some(BlackLevel::new(&[512u32], 1, 1, 1)),
        Some(rawler::rawimage::WhiteLevel::new(vec![16383])),
        false,
    )
}

/// RapidRAW's demosaic against its budget (`speed_budgets::DEMOSAIC_MS`) on
/// a 32 MP mosaic of each pattern, all threads, the quickest of five, with
/// rawler's beside it. Run by hand on the laptop the budgets come from, as
/// `sharpen_gpu::tests::where_the_time_goes` is (docs/RELEASING.md).
#[test]
#[ignore = "timing against the reference laptop's budgets; run by hand"]
fn demosaic_within_budget() {
    use super::speed_budgets;
    speed_budgets::full_speed::hold();
    let mut breaches = Vec::new();
    for (name, cfa) in [("Bayer", "RGGB"), ("X-Trans", XTRANS)] {
        let raw = synthetic(cfa);
        let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
            unreachable!()
        };
        let pixels = PixF32::new_with(samples(&raw), raw.width, raw.height);
        let roi = Rect::new(Point::zero(), raw.dim());
        let rawlers = || {
            if config.cfa.width == 6 {
                drop(XTransDemosaic::new().demosaic(&pixels, &config.cfa, &config.colors, roi));
            } else {
                drop(PPGDemosaic::new().demosaic(&pixels, &config.cfa, &config.colors, roi));
            }
        };
        let theirs = || {
            let mosaic = Mosaic::new(&raw, [2.0, 1.0, 1.5], 16383.0).expect("takes it");
            drop(mosaic.develop());
        };
        let quickest = |run: &dyn Fn()| {
            (0..5)
                .map(|_| {
                    let start = std::time::Instant::now();
                    run();
                    start.elapsed().as_secs_f32() * 1000.0
                })
                .fold(f32::MAX, f32::min)
        };
        let (ours, rapidraw) = (quickest(&rawlers), quickest(&theirs));
        println!("{name} 6982x4654: rawler {ours:.0} ms, RapidRAW {rapidraw:.0} ms");
        breaches.extend(speed_budgets::demosaic_breach(name, rapidraw));
    }
    speed_budgets::assert_none(&breaches);
}
