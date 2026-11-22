//! Measurements behind the highlight rule (`highlight_pull.rs`), run by hand.
//! None of it runs in CI: every test here reads local photos.
//!
//! `AG_RAWS` is a `;`-separated list of RAWs, `AG_SCRATCH` a folder the tests
//! may write in (copies of the RAWs and their sidecars go there, never next to
//! the originals).

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use argentum_engine::colour::{delta_e2000, lab};
use image::DynamicImage;
use rawler::rawimage::{RawImage, RawImageData};

use super::highlight_rule::Rule;
use super::render_check::{Bench, Photo, device, film_lut};

fn raws() -> Vec<PathBuf> {
    std::env::var("AG_RAWS")
        .expect("set AG_RAWS")
        .split(';')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn scratch() -> PathBuf {
    let dir = PathBuf::from(std::env::var("AG_SCRATCH").expect("set AG_SCRATCH"));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// `raw` copied into `dir` once, so a sidecar can be put beside it.
fn copy_into(raw: &Path, dir: &Path) -> PathBuf {
    let copy = dir.join(raw.file_name().expect("file name"));
    if std::fs::metadata(&copy).map(|m| m.len()).ok()
        != std::fs::metadata(raw).map(|m| m.len()).ok()
    {
        std::fs::copy(raw, &copy).expect("copy");
    }
    copy
}

/// An edit saved before the highlight rule changed: a sidecar with no mark,
/// written as bytes so nothing of the app's adds one.
fn put_old_sidecar(photo: &Path) {
    let sidecar = crate::file_management::sidecar_for(photo, None);
    std::fs::write(
        sidecar,
        r#"{"version":1,"rating":0,"adjustments":{"exposure":0.5}}"#,
    )
    .expect("sidecar");
}

/// An edit saved with the pull, before the shadow rule.
fn put_marked_sidecar(photo: &Path, rule: &str) {
    let sidecar = crate::file_management::sidecar_for(photo, None);
    let edit = serde_json::json!({
        "version": 1,
        "rating": 0,
        "adjustments": { "exposure": 0.5 },
        "agHighlights": rule,
    });
    std::fs::write(sidecar, edit.to_string()).expect("sidecar");
}

fn remove_sidecar(photo: &Path) {
    let _ = std::fs::remove_file(crate::file_management::sidecar_for(photo, None));
}

fn hash_of(image: &image::DynamicImage) -> u64 {
    let rgb = image.to_rgb32f();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (rgb.width(), rgb.height()).hash(&mut hasher);
    for v in rgb.as_raw() {
        v.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// The decode the editor and the thumbnails get, as hashes: per RAW, recovery
/// on and off, full and fast. With `AG_SIDECAR=old` each copy carries an edit
/// saved before the pull, with `AG_SIDECAR=pull` (or another rule's word, such
/// as `pull-shadows`) one saved with that rule; with
/// `AG_EXPECT` the hashes must equal that file's. Run once on the code before
/// the change with `AG_OUT`, then after it.
#[test]
#[ignore = "reads local photos; run by hand"]
fn decode_hashes() {
    let dir = scratch();
    let settings = crate::app_settings::AppSettings::default();
    let sidecar = std::env::var("AG_SIDECAR").unwrap_or_default();
    let mut lines = Vec::new();
    for raw in raws() {
        let photo = copy_into(&raw, &dir);
        match sidecar.as_str() {
            "old" => put_old_sidecar(&photo),
            "" => remove_sidecar(&photo),
            word => put_marked_sidecar(&photo, word),
        }
        let bytes = std::fs::read(&photo).expect("read");
        let path = photo.to_string_lossy().to_string();
        for recovery in [true, false] {
            super::highlights::set_enabled(recovery);
            for fast in [false, true] {
                let image = crate::image_loader::load_base_image_from_bytes(
                    &bytes, &path, fast, &settings, None,
                )
                .expect("decode");
                lines.push(format!(
                    "{} recovery={recovery} fast={fast} {:016x}",
                    raw.file_name().unwrap().to_string_lossy(),
                    hash_of(&image)
                ));
            }
        }
        remove_sidecar(&photo);
    }
    super::highlights::set_enabled(true);
    let text = lines.join("\n") + "\n";
    println!("{text}");
    if let Ok(out) = std::env::var("AG_OUT") {
        std::fs::write(out, &text).expect("write");
    }
    if let Ok(expect) = std::env::var("AG_EXPECT") {
        let want = std::fs::read_to_string(expect).expect("expected");
        assert_eq!(text, want, "the decode moved");
    }
}

// ---------------------------------------------------------------------------
// Synthetic clipping: the truth is known.

/// A RAW as rawler decodes it, with its sRAW levels put right (the first of
/// the decode's steps, which says what a sample means).
pub(super) fn decoded(bytes: &[u8]) -> RawImage {
    let source = rawler::rawsource::RawSource::new_from_slice(bytes);
    let decoder = rawler::get_decoder(&source).expect("decoder");
    let mut raw = crate::mods::raw_decoder::raw_image(&source, decoder.as_ref(), bytes, None)
        .expect("raw image");
    super::sraw_levels::fix(&mut raw, bytes);
    raw
}

/// Which colour each sample of `raw` holds: 0 red, 1 green, 2 blue.
fn colour_of(raw: &RawImage, i: usize) -> usize {
    if raw.cpp == 3 {
        i % 3
    } else {
        let (row, col) = (i / raw.width, i % raw.width);
        raw.camera.cfa.color_at(row, col).min(2)
    }
}

fn integers(raw: &RawImage) -> &[u16] {
    match &raw.data {
        RawImageData::Integer(v) => v,
        RawImageData::Float(_) => panic!("expected integer samples"),
    }
}

/// `raw` exposed `stops` brighter and cut at its white level, as a sensor
/// would have recorded it; and which samples were cut.
fn overexposed(raw: &RawImage, stops: i32) -> (RawImage, Vec<bool>) {
    let black = raw
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.);
    let white = *raw.whitelevel.0.first().expect("white") as f32;
    let gain = 2f32.powi(stops);
    let mut cut = Vec::new();
    let samples = integers(raw)
        .iter()
        .map(|&v| {
            let lit = black + (v as f32 - black).max(0.) * gain;
            cut.push(lit > white);
            lit.min(white).round() as u16
        })
        .collect();
    let mut out = raw.clone();
    out.data = RawImageData::Integer(samples);
    (out, cut)
}

/// The samples `ceilings` already calls clipped in `raw` as shot.
fn clipped_as_shot(raw: &RawImage) -> Vec<bool> {
    let ceilings = super::highlights::ceilings(raw).expect("ceilings");
    integers(raw)
        .iter()
        .enumerate()
        .map(|(i, &v)| v as f32 >= ceilings[colour_of(raw, i)])
        .collect()
}

/// Which developed pixels any sample in `flags` reaches through the demosaic
/// and crops of a develop: the flags developed as a picture of their own.
fn reach(raw: &RawImage, flags: &[bool]) -> Vec<bool> {
    use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
    let mut picture = raw.clone();
    picture.data = RawImageData::Float(flags.iter().map(|&f| f as u8 as f32).collect());
    let develop = RawDevelop {
        steps: vec![
            ProcessingStep::Demosaic,
            ProcessingStep::CropActiveArea,
            ProcessingStep::CropDefault,
        ],
        ..Default::default()
    };
    match develop.develop_intermediate(&picture).expect("develop") {
        Intermediate::ThreeColor(p) => p.data.iter().map(|p| p.iter().any(|v| *v > 1e-4)).collect(),
        Intermediate::Monochrome(p) => p.data.iter().map(|v| *v > 1e-4).collect(),
        Intermediate::FourColor(_) => panic!("four colours"),
    }
}

fn develop(raw: RawImage, bytes: &[u8], rule: Rule) -> image::Rgb32FImage {
    crate::raw_processing::develop_decoded(
        raw,
        bytes,
        false,
        &crate::app_settings::AppSettings::default().linear_raw_mode,
        None,
        None,
        rule,
    )
    .expect("develop")
    .to_rgb32f()
}

fn de(a: [f32; 3], b: [f32; 3]) -> f64 {
    delta_e2000(lab(a), lab(b))
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// Each sheet RAW exposed one and two stops brighter and cut at its white:
/// mean CIEDE2000 against the photo as shot (scene-linear, brought back down
/// by the same stops), on the pixels that lost a channel to the cut and had
/// none clipped as shot, today's rule against the pull. And on the bright
/// pixels nothing cut (some channel above half its white once brightened),
/// what each rule moves.
#[test]
#[ignore = "reads local photos; run by hand"]
fn synthetic_clipping() {
    for raw_path in raws() {
        let bytes = std::fs::read(&raw_path).expect("read");
        let shot = decoded(&bytes);
        let as_shot = reach(&shot, &clipped_as_shot(&shot));
        println!(
            "{}: black {:?}, white {:?}, ceilings {:?}, white balance {:?}",
            raw_path.file_name().unwrap().to_string_lossy(),
            shot.blacklevel.levels.first().map(|r| r.as_f32()),
            shot.whitelevel.0,
            super::highlights::ceilings(&shot),
            shot.wb_coeffs
        );
        super::highlights::set_enabled(false);
        let truth = develop(shot.clone(), &bytes, Rule::Pull);
        super::highlights::set_enabled(true);
        for stops in [1, 2] {
            let (lit, cut) = overexposed(&shot, stops);
            let lost = reach(&lit, &cut);
            let down = 2f32.powi(-stops);
            let today = develop(lit.clone(), &bytes, Rule::Rebuild);
            let pull = develop(lit, &bytes, Rule::Pull);
            let (mut on_lost, mut on_bright) = ((vec![], vec![]), (vec![], vec![]));
            for (i, ((t, a), p)) in truth
                .pixels()
                .zip(today.pixels())
                .zip(pull.pixels())
                .enumerate()
            {
                if as_shot[i] {
                    continue;
                }
                let t = t.0;
                let a = a.0.map(|v| v * down);
                let p = p.0.map(|v| v * down);
                let into = if lost[i] {
                    &mut on_lost
                } else if t.iter().any(|v| v / down > 0.5) {
                    &mut on_bright
                } else {
                    continue;
                };
                into.0.push(de(t, a));
                into.1.push(de(t, p));
            }
            println!(
                "{} +{stops}: lost a channel {} px, today {:.2} pull {:.2} | bright, nothing cut {} px, today {:.2} pull {:.2}",
                Path::new(&raw_path).file_name().unwrap().to_string_lossy(),
                on_lost.0.len(),
                mean(&on_lost.0),
                mean(&on_lost.1),
                on_bright.0.len(),
                mean(&on_bright.0),
                mean(&on_bright.1),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Through the editor's render: the tone stages after the decode.

/// The app's starting edit, `stops` of exposure on it.
fn starting_edit(stops: f32) -> serde_json::Value {
    let mut js = crate::image_processing::default_adjustments();
    js["exposure"] = serde_json::json!(stops);
    js
}

/// The editor's render of `photo` at preview size, `stops` of exposure on the
/// starting edit, as linear RGB per pixel (from the 8-bit sRGB it shows).
fn rendered(ctx: &crate::image_processing::GpuContext, photo: &Photo, stops: f32) -> Vec<[f32; 3]> {
    // The input stage keeps its results under the key: each render its own.
    static KEY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let key = KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let bench = Bench::new(ctx, photo, 1920, key);
    let lut = film_lut("K-Portra 400.cube");
    let js = starting_edit(stops);
    let pixels = bench.render(bench.request(&js, &[], &lut, None), false);
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|c| argentum_engine::colour::srgb_decode(p[c] as f32 / 255.)))
        .collect()
}

fn summary(des: &mut [f64]) -> String {
    des.sort_by(|a, b| a.total_cmp(b));
    let p99 = des.get(des.len() * 99 / 100).copied().unwrap_or(0.);
    let above = des.iter().filter(|d| **d > 2.).count() as f64 / des.len().max(1) as f64;
    format!(
        "mean {:.3} p99 {:.2} above 2 {:.3}%",
        mean(des),
        p99,
        above * 100.
    )
}

/// Each RAW as the app opens it, an edit saved before the change (today's
/// rule) against a new one (the pull), through the editor's render at 0, -1
/// and -2 EV: CIEDE2000 over the frame.
#[test]
#[ignore = "reads local photos and needs a GPU; run by hand"]
fn real_files_through_the_render() {
    let ctx = device();
    let dir = scratch();
    let settings = crate::app_settings::AppSettings::default();
    for raw in raws() {
        let photo_path = copy_into(&raw, &dir);
        let path = photo_path.to_string_lossy().to_string();
        let bytes = std::fs::read(&photo_path).expect("read");
        let open = |old: bool| {
            if old {
                put_old_sidecar(&photo_path);
            } else {
                remove_sidecar(&photo_path);
            }
            let full = crate::image_loader::load_base_image_from_bytes(
                &bytes, &path, false, &settings, None,
            )
            .expect("decode");
            remove_sidecar(&photo_path);
            Photo {
                path: path.clone(),
                shot: crate::raw_processing::read_as_shot(&bytes)
                    .unwrap_or_else(crate::as_shot::AsShot::reference),
                full,
            }
        };
        let (today, pull) = (open(true), open(false));
        for stops in [0., -1., -2.] {
            let (a, b) = (rendered(&ctx, &today, stops), rendered(&ctx, &pull, stops));
            let mut des: Vec<f64> = a.iter().zip(&b).map(|(a, b)| de(*a, *b)).collect();
            let moved = des.iter().filter(|d| **d > 0.).count();
            println!(
                "{} {stops:+} EV: {}, {moved} of {} px moved",
                raw.file_name().unwrap().to_string_lossy(),
                summary(&mut des),
                a.len()
            );
        }
    }
}

/// Each RAW as the app opens it, an edit saved with the pull (each channel
/// clipped at black) against one saved with the shadow rule (its pixels below
/// black keep their colour, RAWmakase 23d6c1c4), through the editor's render at the starting
/// edit, each against the camera's embedded JPEG: the median CIEDE2000 of
/// 32-pixel block means over the frame and over the JPEG's dark blocks
/// (L* < 25), as RAWmakase measured it; and how far the two renders are apart.
#[test]
#[ignore = "reads local photos and needs a GPU; run by hand"]
fn shadow_rule_against_the_camera_jpeg() {
    const BLOCK: usize = 32;
    let ctx = device();
    let dir = scratch();
    let settings = crate::app_settings::AppSettings::default();
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
    };
    for raw in raws() {
        let name = raw.file_name().unwrap().to_string_lossy().to_string();
        let photo_path = copy_into(&raw, &dir);
        let path = photo_path.to_string_lossy().to_string();
        let bytes = std::fs::read(&photo_path).expect("read");
        let open = |pull: bool| {
            put_marked_sidecar(&photo_path, if pull { "pull" } else { "pull-shadows" });
            let full = crate::image_loader::load_base_image_from_bytes(
                &bytes, &path, false, &settings, None,
            )
            .expect("decode");
            remove_sidecar(&photo_path);
            Photo {
                path: path.clone(),
                shot: crate::raw_processing::read_as_shot(&bytes)
                    .unwrap_or_else(crate::as_shot::AsShot::reference),
                full,
            }
        };
        let (before, after) = (open(true), open(false));
        let decode_same = hash_of(&before.full) == hash_of(&after.full);
        // The size `rendered` renders at (`Bench::new`).
        let (width, height) = {
            use image::GenericImageView;
            crate::image_processing::downscale_f32_image(&before.full, 1920, 1920).dimensions()
        };
        let (a, b) = (rendered(&ctx, &before, 0.), rendered(&ctx, &after, 0.));
        let mut apart: Vec<f64> = a.iter().zip(&b).map(|(a, b)| de(*a, *b)).collect();
        let moved = apart.iter().filter(|d| **d > 0.).count();
        println!(
            "{name}: decode {}, renders apart {}, {moved} of {} px moved",
            if decode_same { "identical" } else { "differs" },
            summary(&mut apart),
            a.len()
        );
        let Some(jpeg) = crate::image_loader::embedded_preview_fallback(&bytes) else {
            println!("{name}: no embedded JPEG");
            continue;
        };
        let (w, h) = (width as usize, height as usize);
        if (jpeg.width() > jpeg.height()) != (w > h) {
            println!("{name}: the JPEG is turned against the render; not compared");
            continue;
        }
        let jpeg = jpeg
            .resize_exact(width, height, image::imageops::FilterType::Triangle)
            .to_rgb8();
        let camera: Vec<[f32; 3]> = jpeg
            .pixels()
            .map(|p| {
                std::array::from_fn(|c| argentum_engine::colour::srgb_decode(p[c] as f32 / 255.))
            })
            .collect();
        let blocks = |px: &[[f32; 3]]| -> Vec<[f32; 3]> {
            let mut out = Vec::new();
            for by in 0..h / BLOCK {
                for bx in 0..w / BLOCK {
                    let mut sum = [0f32; 3];
                    for y in by * BLOCK..(by + 1) * BLOCK {
                        for x in bx * BLOCK..(bx + 1) * BLOCK {
                            let p = px[y * w + x];
                            (0..3).for_each(|c| sum[c] += p[c]);
                        }
                    }
                    out.push(sum.map(|v| v / (BLOCK * BLOCK) as f32));
                }
            }
            out
        };
        let (cam, ba, bb) = (blocks(&camera), blocks(&a), blocks(&b));
        let dark: Vec<bool> = cam.iter().map(|p| lab(*p)[0] < 25.).collect();
        let against = |r: &[[f32; 3]], only_dark: bool| {
            median(
                r.iter()
                    .zip(&cam)
                    .zip(&dark)
                    .filter(|(_, d)| !only_dark || **d)
                    .map(|((r, c), _)| de(*r, *c))
                    .collect(),
            )
        };
        println!(
            "{name}: against the camera JPEG, median block dE00 all {:.2} -> {:.2}, dark ({} of {} blocks) {:.2} -> {:.2}",
            against(&ba, false),
            against(&bb, false),
            dark.iter().filter(|d| **d).count(),
            dark.len(),
            against(&ba, true),
            against(&bb, true),
        );
    }
}

/// A full-size mask brought to the render's size: a pixel is in it when any
/// full-size pixel it averages is.
fn mask_at(mask: &[bool], width: u32, height: u32, long: u32) -> Vec<bool> {
    let image = image::Rgb32FImage::from_fn(width, height, |x, y| {
        let v = mask[(y * width + x) as usize] as u8 as f32;
        image::Rgb([v, v, v])
    });
    let small =
        crate::image_processing::downscale_f32_image(&DynamicImage::ImageRgb32F(image), long, long)
            .to_rgb32f();
    small.pixels().map(|p| p.0[0] > 0.).collect()
}

/// The synthetic clipping above through the editor's render: the brightened
/// photo brought that many stops down, against the photo as shot, both rendered
/// with the starting edit, on the pixels that lost a channel to the cut; and, as a check
/// on the method, on the pixels no cut reaches.
#[test]
#[ignore = "reads local photos and needs a GPU; run by hand"]
fn synthetic_clipping_through_the_render() {
    let ctx = device();
    for raw_path in raws() {
        let bytes = std::fs::read(&raw_path).expect("read");
        let path = raw_path.to_string_lossy().to_string();
        let shot = decoded(&bytes);
        let as_shot = reach(&shot, &clipped_as_shot(&shot));
        let photo = |image: image::Rgb32FImage| Photo {
            path: path.clone(),
            shot: crate::raw_processing::read_as_shot(&bytes)
                .unwrap_or_else(crate::as_shot::AsShot::reference),
            full: DynamicImage::ImageRgb32F(image),
        };
        super::highlights::set_enabled(false);
        let truth_linear = develop(shot.clone(), &bytes, Rule::Pull);
        super::highlights::set_enabled(true);
        let (w, h) = truth_linear.dimensions();
        let truth = rendered(&ctx, &photo(truth_linear), 0.);
        let as_shot = mask_at(&as_shot, w, h, 1920);
        for stops in [1, 2] {
            let (lit, cut) = overexposed(&shot, stops);
            let lost = mask_at(&reach(&lit, &cut), w, h, 1920);
            // Brought back down in the scene-linear decode, not with the
            // Exposure slider, whose stage is not a plain gain on everything
            // before it: the tone stages then see the same scene as the truth.
            let down = |image: image::Rgb32FImage| {
                let gain = 2f32.powi(-stops);
                let mut image = image;
                image
                    .pixels_mut()
                    .for_each(|p| p.0.iter_mut().for_each(|v| *v *= gain));
                image
            };
            let today = rendered(
                &ctx,
                &photo(down(develop(lit.clone(), &bytes, Rule::Rebuild))),
                0.,
            );
            let pull = rendered(&ctx, &photo(down(develop(lit, &bytes, Rule::Pull))), 0.);
            let (mut on_lost, mut elsewhere) = ((vec![], vec![]), (vec![], vec![]));
            for i in 0..truth.len() {
                if as_shot[i] {
                    continue;
                }
                let into = if lost[i] {
                    &mut on_lost
                } else {
                    &mut elsewhere
                };
                into.0.push(de(truth[i], today[i]));
                into.1.push(de(truth[i], pull[i]));
            }
            println!(
                "{} +{stops}: lost a channel {} px: today {} | pull {} || nothing cut {} px: today {} | pull {}",
                raw_path.file_name().unwrap().to_string_lossy(),
                on_lost.0.len(),
                summary(&mut on_lost.0),
                summary(&mut on_lost.1),
                elsewhere.0.len(),
                summary(&mut elsewhere.0),
                summary(&mut elsewhere.1),
            );
        }
    }
}

/// How long the RAW develop takes (the decode before Noise Reduction and
/// sharpening, which are the same for both), today's rule against the pull,
/// full quality and the thumbnails' fast one. The two take turns, seven times
/// each after one to warm up, and the quickest of each is kept: on a machine
/// doing other work the slowest runs measure that work, not the decode.
#[test]
#[ignore = "reads local photos; timing, run by hand"]
fn decode_time() {
    let dir = scratch();
    let settings = crate::app_settings::AppSettings::default();
    for raw in raws() {
        let photo = copy_into(&raw, &dir);
        let path = photo.to_string_lossy().to_string();
        let bytes = std::fs::read(&photo).expect("read");
        for fast in [false, true] {
            let mut quickest = [f64::MAX; 2];
            for round in 0..8 {
                for (slot, old) in [true, false].into_iter().enumerate() {
                    if old {
                        put_old_sidecar(&photo);
                    } else {
                        remove_sidecar(&photo);
                    }
                    let start = std::time::Instant::now();
                    crate::raw_processing::develop_raw_image(
                        &bytes,
                        fast,
                        2.5,
                        settings.linear_raw_mode.clone(),
                        None,
                        Some(&path),
                    )
                    .expect("decode");
                    let ms = start.elapsed().as_secs_f64() * 1000.;
                    if round > 0 {
                        quickest[slot] = quickest[slot].min(ms);
                    }
                }
            }
            remove_sidecar(&photo);
            println!(
                "{} fast={fast}: today {:.0} ms, pull {:.0} ms",
                raw.file_name().unwrap().to_string_lossy(),
                quickest[0],
                quickest[1]
            );
        }
    }
}

/// The pull alone on 24 million pixels every one of which it works on (each
/// channel between a quarter and all of its white), the quickest of five.
#[test]
#[ignore = "timing, run by hand"]
fn pull_time() {
    let mut state = 12345u32;
    let mut next = move || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        0.25 + (state >> 8) as f32 / (1 << 24) as f32
    };
    let frame: Vec<[f32; 3]> = (0..24_000_000).map(|_| [next(), next(), next()]).collect();
    let scale = super::highlight_pull::SensorScale::new([1.; 4], 0., 1.).unwrap();
    let quickest = (0..5)
        .map(|_| {
            let mut pixels = frame.clone();
            let start = std::time::Instant::now();
            super::highlight_pull::apply(&mut pixels, &[2.1, 1., 1.6, 0.], scale);
            start.elapsed().as_secs_f64() * 1000.
        })
        .fold(f64::MAX, f64::min);
    println!("pull on 24 MP: {quickest:.0} ms");
}
