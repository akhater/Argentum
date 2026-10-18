//! From sensor data to linear RGB, and down to a preview's size.
//!
//! Not tools: the pipeline runs `develop` between the last `Raw` tool and the
//! first `SceneLinear` one, and `resize` straight after it when a smaller
//! picture is asked for. Both are cached like tools. When demosaicing becomes
//! a choice of its own it can become a tool; until then this is the app's
//! develop, unchanged.

use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use rawler::rawimage::{RawImage, RawPhotometricInterpretation};
use rayon::prelude::*;

use crate::{Error, Result, Rgb};

/// Bumped whenever `develop` would give a different picture, so cached
/// results made by an older one are not reused.
pub(crate) const DEVELOP_VERSION: u32 = 1;

/// The ceiling on a developed value. The app's full-quality decode keeps
/// scene-linear values far above white for its tone mapping, and so does the
/// engine, at every size: a preview clipped at white would not show what an
/// exposure change does to a highlight.
const CLAMP_LIMIT: f32 = 1000.0;

// Copied from src-tauri/src/raw_processing.rs @ 2bcb953d, `develop_internal`,
// its default path: full-quality demosaic, the camera matrix to linear sRGB,
// no display encode. Left out, as not the develop's job here: the decode
// hook (`mods::decode::on_raw_decoded` - its highlight recovery is the
// `dt_highlights` tool), the multi-exposure white balance exception,
// orientation, cancellation and the fast and linear-mode variants.
pub(crate) fn develop(raw: &RawImage) -> Result<Rgb> {
    let mut raw_image = raw.clone();
    let is_linear_format = matches!(
        raw_image.photometric,
        RawPhotometricInterpretation::LinearRaw
    );

    let original_white_level = raw_image
        .whitelevel
        .0
        .first()
        .cloned()
        .unwrap_or(u16::MAX as u32) as f32;
    let original_black_level = raw_image
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);

    // rawler clips at the white level it is given. Highlight recovery puts
    // values above it on purpose, so it is given none, and the scale is put
    // back by hand below.
    for level in raw_image.whitelevel.0.iter_mut() {
        *level = u32::MAX;
    }

    let mut developer = RawDevelop::default();
    if is_linear_format {
        developer
            .steps
            .retain(|&step| step != ProcessingStep::SRgb && step != ProcessingStep::Demosaic);
    } else {
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    }

    let developed = developer
        .develop_intermediate(&raw_image)
        .map_err(|e| Error::Decode(e.to_string()))?;
    drop(raw_image);

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    let dim = developed.dim();
    let (width, height) = (dim.w, dim.h);
    let scale = |v: f32| (v * rescale_factor).max(0.0).clamp(0.0, CLAMP_LIMIT);

    let pixels: Vec<[f32; 3]> = match developed {
        Intermediate::ThreeColor(pixels) => pixels
            .data
            .par_iter()
            .map(|p| [scale(p[0]), scale(p[1]), scale(p[2])])
            .collect(),
        Intermediate::Monochrome(pixels) => pixels
            .data
            .par_iter()
            .map(|&p| {
                let v = (p * rescale_factor).clamp(0.0, CLAMP_LIMIT);
                [v, v, v]
            })
            .collect(),
        Intermediate::FourColor(_) => {
            return Err(Error::Decode(
                "four-colour sensors are not developed (nor are they in the app)".into(),
            ));
        }
    };
    Ok(Rgb::new(width, height, pixels))
}

/// The size `develop` gives `raw`, from its geometry alone: the steps of
/// rawler's `develop_intermediate` (rawler `src/imgop/develop.rs` @ 16b9b010)
/// that decide the size, without touching a pixel. A mosaic comes out of
/// demosaic at its active area, and the default crop is then taken.
pub(crate) fn developed_size_px(raw: &RawImage) -> (usize, usize) {
    use rawler::imgop::{Dim2, Point, Rect};

    let demosaics = !matches!(raw.photometric, RawPhotometricInterpretation::LinearRaw);
    let full = Rect::new(Point::new(0, 0), Dim2::new(raw.width, raw.height));
    let mosaic = raw.cpp == 1
        && matches!(&raw.photometric, RawPhotometricInterpretation::Cfa(c) if c.cfa.is_rgb());
    let mut dim = if demosaics && mosaic {
        raw.active_area.unwrap_or(full).d
    } else {
        full.d
    };
    if let Some(mut crop) = raw.crop_area {
        if demosaics && let Some(active) = raw.active_area {
            crop = crop.intersection(&active).adapt(&active);
        }
        let original_width = raw.active_area.map(|a| a.d.w).unwrap_or(raw.width);
        if original_width > 0 {
            let scale = dim.w as f32 / original_width as f32;
            if (scale - 1.0).abs() > 1e-6 {
                crop.scale(scale);
            }
        }
        if !crop.is_empty() && crop.d != dim {
            dim = crop.d;
        }
    }
    (dim.w, dim.h)
}

/// A picture of `width_px` x `height_px` fitted in `max_w_px` x `max_h_px`,
/// keeping its shape and never enlarged: the scale, and the size it comes to.
/// The one fit: the preview's resize and the engine's first view both use it.
pub(crate) fn fit_px(
    width_px: usize,
    height_px: usize,
    max_w_px: u32,
    max_h_px: u32,
) -> (f64, usize, usize) {
    let (w, h) = (width_px.max(1) as f64, height_px.max(1) as f64);
    let scale = (max_w_px as f64 / w).min(max_h_px as f64 / h).min(1.0);
    (
        scale,
        ((w * scale).round() as usize).max(1),
        ((h * scale).round() as usize).max(1),
    )
}

/// Box-filter `rgb` down to fit in `max_w_px` x `max_h_px`, or `None` when it
/// already does. Each output pixel is the plain average of the input pixels
/// under it.
pub(crate) fn resize(rgb: &Rgb, max_w_px: u32, max_h_px: u32) -> Option<Rgb> {
    let (_, w, h) = fit_px(rgb.width, rgb.height, max_w_px, max_h_px);
    if (w, h) == (rgb.width, rgb.height) {
        return None;
    }
    let span = |o: usize, out: usize, input: usize| {
        let a = o * input / out;
        let b = ((o + 1) * input / out).max(a + 1).min(input);
        a..b
    };
    let pixels: Vec<[f32; 3]> = (0..h)
        .into_par_iter()
        .flat_map_iter(|oy| {
            let ys = span(oy, h, rgb.height);
            (0..w).map(move |ox| {
                let xs = span(ox, w, rgb.width);
                let mut sum = [0.0f64; 3];
                for y in ys.clone() {
                    for p in &rgb.pixels[y * rgb.width + xs.start..y * rgb.width + xs.end] {
                        for c in 0..3 {
                            sum[c] += p[c] as f64;
                        }
                    }
                }
                let n = (ys.len() * xs.len()) as f64;
                [
                    (sum[0] / n) as f32,
                    (sum[1] / n) as f32,
                    (sum[2] / n) as f32,
                ]
            })
        })
        .collect();
    Some(Rgb {
        width: w,
        height: h,
        pixels,
        full_width: rgb.full_width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_keeps_its_shape_and_its_full_width() {
        let rgb = Rgb::new(600, 400, vec![[0.25, 0.5, 1.0]; 600 * 400]);
        let small = resize(&rgb, 150, 150).expect("smaller");
        assert_eq!((small.width, small.height), (150, 100));
        assert_eq!(small.full_width, 600);
        assert_eq!(small.scale(), 0.25);
        assert!(small.pixels.iter().all(|p| *p == [0.25, 0.5, 1.0]));
        assert!(resize(&rgb, 4000, 4000).is_none(), "never enlarged");
    }

    #[test]
    fn the_box_filter_averages() {
        let pixels = (0..16).map(|i| [i as f32, 0.0, 0.0]).collect();
        let rgb = Rgb::new(4, 4, pixels);
        let half = resize(&rgb, 2, 2).unwrap();
        assert_eq!(half.pixels[0][0], (0.0 + 1.0 + 4.0 + 5.0) / 4.0);
        assert_eq!(half.pixels[3][0], (10.0 + 11.0 + 14.0 + 15.0) / 4.0);
    }
}
