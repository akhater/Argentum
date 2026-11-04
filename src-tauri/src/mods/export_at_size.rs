//! An export developed at the size it is written, not at full size and shrunk.
//!
//! The headless export makes Shoebox's previews: 2048 px from a 6960 px RAW.
//! Rendering all 32 million pixels on the GPU and then throwing nine in ten
//! away cost about 2 s a photo, and the Lanczos shrink of the result another
//! second. The editor already renders at the size it shows (lib.rs
//! `compute_transformed_preview`, RapidRAW): the picture is warped, rotated
//! and cropped at full size, shrunk by area average, and only then
//! developed, with RapidRAW's radii relative to the image (`scale` in
//! `gpu_processing.rs`) and Argentum's sharpening told the scale
//! (`input_stage::at_export_scale`). This module is that, for an export.
//!
//! Measured 2026-11-04 on an R6 Mark III CR3 (6960x4640) to 2048 px, Intel
//! Arc 140V on Vulkan: GPU develop 2.2 s -> 0.09 s, shrink and write 1.0 s ->
//! 0.07 s. The decode is then most of what is left: half of it is the
//! demosaic and RAW clean-up at full size, which a half-size demosaic would
//! cut (rawler has one, `Superpixel3Channel`, but no setting selects it).
//!
//! Not the same pixels as full size then Lanczos: shrinking in linear light
//! before the tone curve, as the editor does, keeps fine texture a little
//! brighter and softer. On the five sheet RAWs, ΔE2000 mean 0.4 to 2.1, p95
//! 1.6 to 5.7, the same with a neutral edit, so it is the resampling and not
//! any one tool (`delta_e_against_the_full_size_export`).

use std::borrow::Cow;

use image::{DynamicImage, GenericImageView};

use crate::image_processing::downscale_f32_image;

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

    #[test]
    fn a_size_at_or_above_the_photo_leaves_it_alone() {
        let full = photo(1200, 800);
        let (same, px_scale) = shrink_to(Cow::Borrowed(&full), (1200, 800));
        assert!(matches!(same, Cow::Borrowed(_)));
        assert_eq!(px_scale, 1.0);
    }
}
