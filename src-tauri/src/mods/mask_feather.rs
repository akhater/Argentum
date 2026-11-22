//! A mask's feather: the Gaussian blur `apply_grow_and_feather` runs on it.
//!
//! The same kernel and arithmetic as `imageproc::filter::gaussian_blur_f32`
//! (radius ceil(2 sigma), edges clamped, f32 sums in kernel order, each pass
//! truncated to u8), so the bytes match it; rows run on rayon threads instead
//! of one.

use std::f32::consts::PI;

use image::GrayImage;
use rayon::prelude::*;

// RapidRAW 15a9ff2e: src-tauri/src/mask_generation.rs
fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let radius = (2.0 * sigma).ceil() as usize;
    let pdf =
        |x: f32| (sigma * (2.0 * PI).sqrt()).recip() * (-x.powi(2) / (2.0 * sigma.powi(2))).exp();
    let mut kernel = vec![0.0f32; 2 * radius + 1];
    for i in 0..=radius {
        let value = pdf(i as f32);
        kernel[radius + i] = value;
        kernel[radius - i] = value;
    }
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|k| *k /= sum);
    kernel
}

// RapidRAW 15a9ff2e: src-tauri/src/mask_generation.rs
fn saturate_u8(value: f32) -> u8 {
    if value < 255.0 {
        if value > 0.0 { value as u8 } else { 0 }
    } else {
        255
    }
}

/// `mask` blurred by a Gaussian of standard deviation `sigma_px`.
// RapidRAW 15a9ff2e: src-tauri/src/mask_generation.rs
pub fn gaussian_blur_mask(mask: &GrayImage, sigma_px: f32) -> GrayImage {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    if w == 0 || h == 0 {
        return mask.clone();
    }
    let kernel = gaussian_kernel(sigma_px);
    let radius = (kernel.len() / 2) as isize;
    let tap = |center: usize, offset: usize, len: usize| {
        (center as isize + offset as isize - radius).clamp(0, len as isize - 1) as usize
    };

    let mut horizontal = vec![0u8; w * h];
    horizontal
        .par_chunks_exact_mut(w)
        .zip(mask.as_raw().par_chunks_exact(w))
        .for_each(|(out, row)| {
            for (x, value) in out.iter_mut().enumerate() {
                let mut acc = 0.0f32;
                for (i, k) in kernel.iter().enumerate() {
                    acc += row[tap(x, i, w)] as f32 * k;
                }
                *value = saturate_u8(acc);
            }
        });

    let mut blurred = vec![0u8; w * h];
    blurred
        .par_chunks_exact_mut(w)
        .enumerate()
        .for_each(|(y, out)| {
            let mut acc = vec![0.0f32; w];
            for (i, k) in kernel.iter().enumerate() {
                let row = &horizontal[tap(y, i, h) * w..][..w];
                for (a, &v) in acc.iter_mut().zip(row) {
                    *a += v as f32 * k;
                }
            }
            for (value, a) in out.iter_mut().zip(acc) {
                *value = saturate_u8(a);
            }
        });

    GrayImage::from_raw(w as u32, h as u32, blurred).expect("mask buffer size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    /// Masks shaped like the editor's: a hard radial, a soft gradient, and
    /// speckle (an AI mask's ragged edge), at an odd size.
    fn masks(w: u32, h: u32) -> Vec<GrayImage> {
        let radial = GrayImage::from_fn(w, h, |x, y| {
            let (dx, dy) = (x as f32 - w as f32 * 0.4, y as f32 - h as f32 * 0.55);
            Luma([if dx * dx + dy * dy < (w.min(h) as f32 * 0.3).powi(2) {
                255
            } else {
                0
            }])
        });
        let gradient = GrayImage::from_fn(w, h, |x, _| Luma([(x * 255 / (w - 1)) as u8]));
        let mut state = 0x2545_f491u32;
        let speckle = GrayImage::from_fn(w, h, |_, _| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            Luma([if state % 7 < 3 {
                255
            } else {
                (state >> 8) as u8
            }])
        });
        vec![radial, gradient, speckle]
    }

    #[test]
    fn same_bytes_as_imageproc() {
        // Feathers 25, 50 and 100 on a 1000 px short side (MAX_FEATHER_SIGMA_PERCENTAGE
        // 0.025), and kernels wider than the mask.
        for sigma in [0.5, 1.0, 6.25, 12.5, 25.0, 300.0] {
            for (n, mask) in masks(357, 201).iter().enumerate() {
                let ours = gaussian_blur_mask(mask, sigma);
                let theirs = imageproc::filter::gaussian_blur_f32(mask, sigma);
                assert!(
                    ours.as_raw() == theirs.as_raw(),
                    "mask {n}, sigma {sigma}: the bytes differ"
                );
            }
        }
    }

    #[test]
    fn an_empty_mask_stays_empty() {
        let empty = GrayImage::new(0, 0);
        assert_eq!(gaussian_blur_mask(&empty, 4.0).dimensions(), (0, 0));
    }

    /// A 24 MP mask at feathers 25, 50 and 100, imageproc against this.
    /// `cargo test --profile measure --lib mods::mask_feather:: -- --ignored --nocapture`
    #[test]
    #[ignore = "timing, run by hand"]
    fn feather_time() {
        let (w, h) = (6000, 4000);
        let base = w.min(h) as f32;
        for mask in masks(w, h) {
            for feather in [25.0f32, 50.0, 100.0] {
                let sigma = feather / 100.0 * base * 0.025;
                let at = std::time::Instant::now();
                let theirs = imageproc::filter::gaussian_blur_f32(&mask, sigma);
                let before = at.elapsed();
                let at = std::time::Instant::now();
                let ours = gaussian_blur_mask(&mask, sigma);
                let after = at.elapsed();
                assert!(ours.as_raw() == theirs.as_raw());
                println!(
                    "feather {feather} (sigma {sigma} px): imageproc {:.0} ms, parallel {:.0} ms",
                    before.as_secs_f64() * 1e3,
                    after.as_secs_f64() * 1e3
                );
            }
        }
    }
}
