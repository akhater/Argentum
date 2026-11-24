//! How far a wide-gamut JPEG or TIFF is from its colours, read as sRGB (as
//! before `icc_input.rs`) and with its profile: each RAW given is decoded,
//! written as sRGB, converted to Adobe RGB, Display P3 and ProPhoto with the
//! profile embedded (an 8-bit JPEG at quality 100 and a 16-bit TIFF), loaded
//! back by the app's own loader both ways, and compared to the sRGB original
//! by CIEDE2000.
//!
//! `AG_RAWS='a.CR3;b.CR2' cargo test --lib mods::icc_input_measure -- --ignored --nocapture`
//!
//! And, with no file: how far each way of bringing a colour outside sRGB
//! inside lands from that colour (`out_of_gamut`).

use std::io::Cursor;

use argentum_engine::colour::{delta_e2000, lab, srgb_decode};
use image::{ImageEncoder, Rgb32FImage};
use moxcms::{ColorProfile, Layout, TransformOptions};

use super::highlight_rule::Rule;

const LONG_SIDE: u32 = 1500;

fn spaces() -> [(&'static str, ColorProfile); 3] {
    [
        ("Adobe RGB", ColorProfile::new_adobe_rgb()),
        ("Display P3", ColorProfile::new_display_p3()),
        ("ProPhoto", ColorProfile::new_pro_photo_rgb()),
    ]
}

/// sRGB-encoded floats, 0 to 1, written in `space`'s encoding.
fn into_space(srgb: &[f32], space: &ColorProfile) -> Vec<f32> {
    let transform = ColorProfile::new_srgb()
        .create_transform_f32(Layout::Rgb, space, Layout::Rgb, TransformOptions::default())
        .unwrap();
    let mut out = vec![0.0; srgb.len()];
    transform.transform(srgb, &mut out).unwrap();
    out
}

fn jpeg(samples: &[f32], w: u32, h: u32, icc: Option<Vec<u8>>) -> Vec<u8> {
    let bytes: Vec<u8> = samples
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 100);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc).unwrap();
    }
    encoder
        .write_image(&bytes, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn tiff16(samples: &[f32], w: u32, h: u32, icc: Option<Vec<u8>>) -> Vec<u8> {
    let words: Vec<u8> = samples
        .iter()
        .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_ne_bytes())
        .collect();
    let mut out = Cursor::new(Vec::new());
    let mut encoder = image::codecs::tiff::TiffEncoder::new(&mut out);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc).unwrap();
    }
    encoder
        .write_image(&words, w, h, image::ExtendedColorType::Rgb16)
        .unwrap();
    out.into_inner()
}

fn png16(samples: &[f32], w: u32, h: u32, icc: Option<Vec<u8>>) -> Vec<u8> {
    let words: Vec<u8> = samples
        .iter()
        .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_ne_bytes())
        .collect();
    let mut out = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc).unwrap();
    }
    encoder
        .write_image(&words, w, h, image::ExtendedColorType::Rgb16)
        .unwrap();
    out
}

fn loaded(bytes: &[u8], rule: Rule) -> Rgb32FImage {
    crate::image_loader::load_image_with_orientation(
        bytes,
        None,
        rule,
        crate::mods::heic::Purpose::Open,
    )
    .unwrap()
    .into_rgb32f()
}

/// Mean and 99th percentile of CIEDE2000 between two sRGB-encoded images.
fn delta(reference: &[f32], got: &[f32]) -> (f64, f64) {
    let linear = |p: &[f32; 3]| p.map(srgb_decode);
    let mut d: Vec<f64> = reference
        .as_chunks::<3>()
        .0
        .iter()
        .zip(got.as_chunks::<3>().0)
        .map(|(a, b)| delta_e2000(lab(linear(a)), lab(linear(b))))
        .collect();
    d.sort_by(f64::total_cmp);
    let mean = d.iter().sum::<f64>() / d.len() as f64;
    (mean, d[(d.len() * 99 / 100).min(d.len() - 1)])
}

#[test]
#[ignore = "reads RAW files; run by hand"]
fn wide_gamut_files_today_and_with_their_profile() {
    let raws = std::env::var("AG_RAWS").expect("set AG_RAWS");
    println!("| photo | space | file | today mean | today p99 | profile mean | profile p99 |");
    println!("|---|---|---|---|---|---|---|");
    for path in raws.split(';').filter(|p| !p.trim().is_empty()) {
        let bytes = std::fs::read(path).expect("read raw");
        let linear = crate::raw_processing::develop_raw_image(
            &bytes,
            false,
            2.5,
            "default".to_string(),
            None,
            Some(path),
        )
        .expect("decodes");
        let linear = linear.thumbnail(LONG_SIDE, LONG_SIDE).into_rgb32f();
        let (w, h) = linear.dimensions();
        let srgb: Vec<f32> = linear
            .as_raw()
            .iter()
            .map(|v| crate::image_processing::linear_channel_to_srgb(*v).min(1.0))
            .collect();
        let name = std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // sRGB, tagged and untagged: the same pixels both ways.
        let srgb_icc = ColorProfile::new_srgb().encode().ok();
        for file in [
            jpeg(&srgb, w, h, None),
            jpeg(&srgb, w, h, srgb_icc.clone()),
            tiff16(&srgb, w, h, None),
            tiff16(&srgb, w, h, srgb_icc.clone()),
        ] {
            assert_eq!(
                loaded(&file, Rule::PullShadowsDemosaic).as_raw(),
                loaded(&file, Rule::LATEST).as_raw(),
                "{name}: an sRGB file changed"
            );
        }

        for (space_name, space) in spaces() {
            let wide = into_space(&srgb, &space);
            let icc = space.encode().expect("profile encodes");
            for (kind, file, reference) in [
                (
                    "JPEG 8-bit",
                    jpeg(&wide, w, h, Some(icc.clone())),
                    loaded(&jpeg(&srgb, w, h, None), Rule::PullShadowsDemosaic),
                ),
                (
                    "TIFF 16-bit",
                    tiff16(&wide, w, h, Some(icc.clone())),
                    loaded(&tiff16(&srgb, w, h, None), Rule::PullShadowsDemosaic),
                ),
            ] {
                let (today_mean, today_p99) = delta(
                    reference.as_raw(),
                    loaded(&file, Rule::PullShadowsDemosaic).as_raw(),
                );
                let (fixed_mean, fixed_p99) =
                    delta(reference.as_raw(), loaded(&file, Rule::LATEST).as_raw());
                println!(
                    "| {name} | {space_name} | {kind} | {today_mean:.2} | {today_p99:.2} | {fixed_mean:.2} | {fixed_p99:.2} |"
                );
            }
        }
    }
}

/// Every colour of a 17-step cube in each wide space that is outside sRGB,
/// as the display shows it (each channel clipped to 0..1 at the end),
/// against the colour itself: clipped channel by channel to 0..1 at the
/// start, as `icc_input` converts it (clipped below zero, kept above one),
/// or by RapidRAW's pull toward the peak and scale back to white.
// RapidRAW 47e6274b: src-tauri/src/image_loader.rs (`convert_profile_to_srgb`)
fn rapidraw(rgb: [f32; 3]) -> [f32; 3] {
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    if peak <= 0.0 {
        return [0.0; 3];
    }
    let floor = rgb[0].min(rgb[1]).min(rgb[2]);
    let saturation = if floor < 0.0 {
        peak / (peak - floor)
    } else {
        1.0
    };
    let scale = peak.max(1.0);
    rgb.map(|c| (peak - (peak - c) * saturation) / scale)
}

#[test]
#[ignore = "prints a table; run by hand"]
fn out_of_gamut() {
    const STEPS: usize = 16;
    println!("| space | colours outside sRGB | clipped at the start | icc_input | RapidRAW |");
    println!("|---|---|---|---|---|");
    for (space_name, space) in spaces() {
        let encoded: Vec<f32> = (0..(STEPS + 1).pow(3))
            .flat_map(|i| {
                [
                    i % (STEPS + 1),
                    i / (STEPS + 1) % (STEPS + 1),
                    i / (STEPS + 1).pow(2),
                ]
                .map(|c| c as f32 / STEPS as f32)
            })
            .collect();
        // The true colours, linear sRGB unbounded: what the matrix gives.
        let truth = {
            let srgb = ColorProfile::new_srgb();
            let m = |p: &ColorProfile| {
                glam::DMat3::from_cols_array_2d(&p.colorant_matrix().v).transpose()
            };
            let to_srgb = (m(&srgb).inverse() * m(&space)).as_mat3();
            let mut lin = encoded.clone();
            let transform = space
                .create_transform_f32(
                    Layout::Rgb,
                    &{
                        let mut t = space.clone();
                        let id = moxcms::ToneReprCurve::Lut(Vec::new());
                        t.red_trc = Some(id.clone());
                        t.green_trc = Some(id.clone());
                        t.blue_trc = Some(id);
                        // moxcms's own Display P3 names DCI's primaries in
                        // its CICP: the colorants are the profile.
                        t.cicp = None;
                        t
                    },
                    Layout::Rgb,
                    TransformOptions::default(),
                )
                .unwrap();
            transform.transform(&encoded, &mut lin).unwrap();
            lin.as_chunks::<3>()
                .0
                .iter()
                .map(|p| (to_srgb * glam::Vec3::from_array(*p)).to_array())
                .collect::<Vec<_>>()
        };
        let mut ours = encoded.clone();
        super::icc_input::convert_to_srgb(&mut ours, &space).unwrap();
        let shown = |p: [f32; 3]| p.map(|c| c.clamp(0.0, 1.0));
        let (mut clip, mut kept, mut theirs) = (vec![], vec![], vec![]);
        for (i, t) in truth.iter().enumerate() {
            if t.iter().all(|c| (-1e-4..=1.0 + 1e-4).contains(c)) {
                continue;
            }
            let t_lab = lab(*t);
            clip.push(delta_e2000(t_lab, lab(shown(*t))));
            let o = [ours[i * 3], ours[i * 3 + 1], ours[i * 3 + 2]].map(srgb_decode);
            kept.push(delta_e2000(t_lab, lab(shown(o))));
            theirs.push(delta_e2000(t_lab, lab(rapidraw(*t))));
        }
        let stat = |d: &mut Vec<f64>| {
            d.sort_by(f64::total_cmp);
            (
                d.iter().sum::<f64>() / d.len() as f64,
                d[(d.len() * 99 / 100).min(d.len() - 1)],
            )
        };
        let (c, k, r) = (stat(&mut clip), stat(&mut kept), stat(&mut theirs));
        println!(
            "| {space_name} | {} | {:.2} / {:.2} | {:.2} / {:.2} | {:.2} / {:.2} |",
            clip.len(),
            c.0,
            c.1,
            k.0,
            k.1,
            r.0,
            r.1
        );
    }
}

/// Through the app's loader, on a small made-up picture: a wide-gamut TIFF
/// or PNG comes back to its sRGB colours under the profile rule and keeps
/// today's numbers under the one before; an sRGB file, tagged or not, is the
/// same under both.
#[test]
fn the_loader_reads_profiles_only_under_the_profile_rule() {
    // At 24 x 16 the `image` crate's TIFF round trip loses the profile.
    let (w, h) = (240, 160);
    let srgb: Vec<f32> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            [0.1 + 0.8 * x, 0.2 + 0.6 * y, 0.9 - 0.7 * x * y]
        })
        .collect();
    let srgb_icc = ColorProfile::new_srgb().encode().ok();
    for file in [
        jpeg(&srgb, w, h, None),
        jpeg(&srgb, w, h, srgb_icc.clone()),
        tiff16(&srgb, w, h, srgb_icc.clone()),
        png16(&srgb, w, h, srgb_icc),
    ] {
        assert_eq!(
            loaded(&file, Rule::PullShadowsDemosaic).as_raw(),
            loaded(&file, Rule::LATEST).as_raw()
        );
    }
    let reference = loaded(&tiff16(&srgb, w, h, None), Rule::PullShadowsDemosaic);
    for (_, space) in spaces() {
        let wide = into_space(&srgb, &space);
        for file in [
            tiff16(&wide, w, h, space.encode().ok()),
            png16(&wide, w, h, space.encode().ok()),
        ] {
            let (today, _) = delta(
                reference.as_raw(),
                loaded(&file, Rule::PullShadowsDemosaic).as_raw(),
            );
            let (fixed, worst) = delta(reference.as_raw(), loaded(&file, Rule::LATEST).as_raw());
            assert!(
                today > 1.0 && fixed < 0.05 && worst < 0.1,
                "{today} {fixed} {worst}"
            );
        }
    }
}
