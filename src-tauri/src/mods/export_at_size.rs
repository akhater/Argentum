//! An export developed at twice the size it is written, not at full size.
//!
//! The headless export makes Shoebox's previews: 2048 px from a 6960 px RAW.
//! Rendering all 32 million pixels on the GPU and then throwing nine in ten
//! away cost about 2 s a photo, and the Lanczos shrink of the result another
//! second. The editor renders at the size it shows (lib.rs
//! `compute_transformed_preview`, RapidRAW): the picture is warped, rotated
//! and cropped at full size, shrunk by area average, and only then
//! developed, with RapidRAW's radii relative to the image (`scale` in
//! `gpu_processing.rs`) and Argentum's sharpening told the scale
//! (`input_stage::at_export_scale`). An export does the same at twice its
//! size (`DEVELOP_OVERSIZE`), then takes the last step the full-size export
//! takes, Lanczos3 on the finished picture (`lanczos3_to`).
//!
//! WHY TWICE: the same look as full size then Lanczos
//!
//! Against 26.45.3's full-size export of the five sheet RAWs at 2048 px,
//! ΔE2000 per pixel (worst file, mean / p95), 2026-11-05:
//! developed at 1x 2.11 / 5.70 (fine texture softer and brighter: the shrink
//! came before the tone curve), 1.25x 1.79 / 4.73, 1.5x 1.21 / 2.95, 1.75x
//! 1.12 / 2.73, 2x 0.73 / 2.13. A photo under twice the written size is
//! developed at full size, exactly as before. Test:
//! `delta_e_against_the_full_size_export`.

use std::borrow::Cow;

use image::{DynamicImage, GenericImageView, RgbaImage};
use rayon::prelude::*;

use crate::image_processing::downscale_f32_image;

/// How many times the written size a photo is developed at, at most.
pub const DEVELOP_OVERSIZE: u32 = 2;

/// `size` scaled to fit inside `bounds`, rounded as `DynamicImage::resize`
/// rounds it (image 0.25.10, src/math/utils.rs `resize_dimensions`).
pub fn fit_within((w, h): (u32, u32), (bw, bh): (u32, u32)) -> (u32, u32) {
    let ratio = (bw as f64 / w as f64).min(bh as f64 / h as f64);
    (
        ((w as f64 * ratio).round() as u32).max(1),
        ((h as f64 * ratio).round() as u32).max(1),
    )
}

/// The size to develop a photo written at `size` at.
pub fn develop_size(size: (u32, u32)) -> (u32, u32) {
    (size.0 * DEVELOP_OVERSIZE, size.1 * DEVELOP_OVERSIZE)
}

/// Memory one photo exported this way needs, decode included, with room to
/// spare: two at once peaked at 2.7 GB for the whole process on 32 MP RAWs.
pub const GB_PER_PHOTO: f64 = 2.0;

/// Photos developed at once. The decode already uses every core: on nine R6
/// photos two at once took 8.4 s against 11.4 s for one, three 8.1 s and four
/// 8.7 s, and the first photo came later with each one added.
pub const MOST_AT_ONCE: usize = 2;

/// `image` shrunk to `size`, when that is smaller, and how many of its pixels
/// each full-size pixel became (1.0 when it was left alone).
pub fn shrink_to(image: Cow<'_, DynamicImage>, size: (u32, u32)) -> (Cow<'_, DynamicImage>, f32) {
    let (full_w, full_h) = image.dimensions();
    if size.0 >= full_w || size.1 >= full_h || size.0 == 0 || size.1 == 0 {
        return (image, 1.0);
    }
    let shrunk = downscale_f32_image(&image, size.0, size.1);
    let px_scale = shrunk.width() as f32 / full_w as f32;
    (Cow::Owned(shrunk), px_scale)
}

/// The developed picture at exactly `size`, by Lanczos3: what the full-size
/// export's `DynamicImage::resize` does, on every core for an 8-bit picture.
pub fn lanczos3_to(image: DynamicImage, (w, h): (u32, u32)) -> DynamicImage {
    if image.dimensions() == (w, h) {
        return image;
    }
    match image {
        DynamicImage::ImageRgba8(rgba) => DynamicImage::ImageRgba8(lanczos3_rgba8(&rgba, w, h)),
        other => other.resize_exact(w, h, image::imageops::FilterType::Lanczos3),
    }
}

// Ported from image 0.25.10, src/imageops/sample.rs (MIT OR Apache-2.0):
// `resize` with `FilterType::Lanczos3` - a vertical pass into f32, then a
// horizontal pass rounded back to 8 bits, the same weights and sums in the same
// order. Theirs runs on one core; here each output row is its own task.

fn lanczos3(x: f32) -> f32 {
    let sinc = |t: f32| {
        if t == 0.0 {
            1.0
        } else {
            (t * std::f32::consts::PI).sin() / (t * std::f32::consts::PI)
        }
    };
    if x.abs() < 3.0 {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// For each output position along an axis of `from` pixels shrunk to `to`:
/// the first input pixel and the normalised weights.
fn lanczos3_taps(from: u32, to: u32) -> Vec<(u32, Vec<f32>)> {
    let ratio = from as f32 / to as f32;
    let sratio = ratio.max(1.0);
    let support = 3.0 * sratio;
    (0..to)
        .map(|out| {
            let centre = (out as f32 + 0.5) * ratio;
            let left = ((centre - support).floor() as i64).clamp(0, from as i64 - 1) as u32;
            let right =
                ((centre + support).ceil() as i64).clamp(left as i64 + 1, from as i64) as u32;
            let centre = centre - 0.5;
            let mut ws: Vec<f32> = (left..right)
                .map(|i| lanczos3((i as f32 - centre) / sratio))
                .collect();
            let sum: f32 = ws.iter().sum();
            ws.iter_mut().for_each(|w| *w /= sum);
            (left, ws)
        })
        .collect()
}

fn lanczos3_rgba8(image: &RgbaImage, new_w: u32, new_h: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let src = image.as_raw();
    let row = w as usize * 4;

    let vertical = lanczos3_taps(h, new_h);
    let mut tmp = vec![0.0f32; row * new_h as usize];
    tmp.par_chunks_mut(row)
        .zip(vertical.par_iter())
        .for_each(|(out, (top, ws))| {
            for x in 0..w as usize {
                let mut t = [0.0f32; 4];
                for (i, wt) in ws.iter().enumerate() {
                    let p = &src[(*top as usize + i) * row + x * 4..][..4];
                    (0..4).for_each(|c| t[c] += p[c] as f32 * wt);
                }
                out[x * 4..x * 4 + 4].copy_from_slice(&t);
            }
        });

    let horizontal = lanczos3_taps(w, new_w);
    let mut out = vec![0u8; new_w as usize * 4 * new_h as usize];
    out.par_chunks_mut(new_w as usize * 4)
        .zip(tmp.par_chunks(row))
        .for_each(|(out, line)| {
            for (x, (left, ws)) in horizontal.iter().enumerate() {
                let mut t = [0.0f32; 4];
                for (i, wt) in ws.iter().enumerate() {
                    let p = &line[(*left as usize + i) * 4..][..4];
                    (0..4).for_each(|c| t[c] += p[c] * wt);
                }
                (0..4).for_each(|c| out[x * 4 + c] = t[c].clamp(0.0, 255.0).round() as u8);
            }
        });
    RgbaImage::from_raw(new_w, new_h, out).expect("sized for new_w x new_h")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn photo(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb32F(ImageBuffer::from_pixel(w, h, Rgb([0.5, 0.25, 0.125])))
    }

    #[test]
    fn a_smaller_size_is_developed_at_that_size() {
        let (shrunk, px_scale) = shrink_to(Cow::Owned(photo(6960, 4640)), (2048, 1365));
        assert_eq!(shrunk.dimensions(), (2048, 1365));
        assert!((px_scale - 2048.0 / 6960.0).abs() < 1e-6);
    }

    /// How far a reduced-size export is from the full-size-then-shrunk one:
    /// ΔE2000 per pixel, mean and p95, for each file in both folders.
    /// `AG_BEFORE` and `AG_AFTER` are two export folders of the same photos.
    #[test]
    #[ignore = "measurement; needs AG_BEFORE and AG_AFTER"]
    fn delta_e_against_the_full_size_export() {
        use argentum_engine::colour::{delta_e2000, lab, srgb_decode};
        let before = std::path::PathBuf::from(std::env::var("AG_BEFORE").expect("AG_BEFORE"));
        let after = std::path::PathBuf::from(std::env::var("AG_AFTER").expect("AG_AFTER"));
        let mut names: Vec<_> = std::fs::read_dir(&before)
            .expect("AG_BEFORE is a folder")
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        names.sort();
        for name in names {
            let (Ok(a), Ok(b)) = (
                image::open(before.join(&name)),
                image::open(after.join(&name)),
            ) else {
                continue;
            };
            let (a, b) = (a.to_rgb8(), b.to_rgb8());
            if a.dimensions() != b.dimensions() {
                println!(
                    "MEASURED: {name:?} sizes differ {:?} {:?}",
                    a.dimensions(),
                    b.dimensions()
                );
                continue;
            }
            let linear = |p: &Rgb<u8>| p.0.map(|c| srgb_decode(c as f32 / 255.0));
            let report = |label: &str, a: &[[f32; 3]], b: &[[f32; 3]]| {
                let mut de: Vec<f64> = a
                    .iter()
                    .zip(b)
                    .map(|(p, q)| delta_e2000(lab(*p), lab(*q)))
                    .collect();
                de.sort_by(f64::total_cmp);
                let mean = de.iter().sum::<f64>() / de.len() as f64;
                let p95 = de[de.len() * 95 / 100];
                let l_shift = a
                    .iter()
                    .zip(b)
                    .map(|(p, q)| lab(*q)[0] - lab(*p)[0])
                    .sum::<f64>()
                    / a.len() as f64;
                println!(
                    "MEASURED: {name:?} {label}: dE2000 mean {mean:.2}, p95 {p95:.2}, mean L shift {l_shift:+.2}"
                );
            };
            let pa: Vec<[f32; 3]> = a.pixels().map(linear).collect();
            let pb: Vec<[f32; 3]> = b.pixels().map(linear).collect();
            report("per pixel", &pa, &pb);
            // Tone and colour with fine detail averaged out: 8x8 blocks.
            let blocks = |px: &[[f32; 3]]| {
                let (w, h) = (a.width() as usize, a.height() as usize);
                let mut out = Vec::new();
                for by in 0..h / 8 {
                    for bx in 0..w / 8 {
                        let mut sum = [0.0f32; 3];
                        for y in 0..8 {
                            for x in 0..8 {
                                let p = px[(by * 8 + y) * w + bx * 8 + x];
                                (0..3).for_each(|c| sum[c] += p[c] / 64.0);
                            }
                        }
                        out.push(sum);
                    }
                }
                out
            };
            report("8x8 blocks", &blocks(&pa), &blocks(&pb));
        }
    }

    /// The port gives the image crate's Lanczos3 byte for byte, at the 2x
    /// shrink and at a ratio that is not a whole number.
    #[test]
    fn the_parallel_lanczos_is_the_image_crates_to_the_byte() {
        let mut seed = 0x9e37_79b9_u32;
        let noise = RgbaImage::from_fn(301, 203, |_, _| {
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 24) as u8
            };
            image::Rgba([next(), next(), next(), 255])
        });
        for (w, h) in [(150, 101), (97, 64), (300, 203)] {
            let theirs =
                image::imageops::resize(&noise, w, h, image::imageops::FilterType::Lanczos3);
            let ours = lanczos3_to(DynamicImage::ImageRgba8(noise.clone()), (w, h));
            assert_eq!(ours.to_rgba8().as_raw(), theirs.as_raw(), "{w}x{h}");
        }
    }

    #[test]
    fn the_written_size_is_rounded_as_the_full_size_export_rounds_it() {
        assert_eq!(fit_within((6960, 4640), (2048, 1365)), (2048, 1365));
        assert_eq!(fit_within((3999, 2666), (2048, 1365)), (2048, 1365));
        assert_eq!(develop_size((2048, 1365)), (4096, 2730));
    }

    #[test]
    fn a_size_at_or_above_the_photo_leaves_it_alone() {
        let full = photo(1200, 800);
        let (same, px_scale) = shrink_to(Cow::Borrowed(&full), (1200, 800));
        assert!(matches!(same, Cow::Borrowed(_)));
        assert_eq!(px_scale, 1.0);
    }
}
