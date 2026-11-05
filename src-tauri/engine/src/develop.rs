//! From sensor data to linear RGB, and down to a preview's size.
//!
//! Not tools: the pipeline runs `develop` between the last `Raw` tool and the
//! first `SceneLinear` one, and `resize` straight after it when a smaller
//! picture is asked for. Both are cached like tools. When demosaicing becomes
//! a choice of its own it can become a tool; until then this is the app's
//! develop, unchanged, at full size - and at half size for a preview that
//! half the photo still covers, so a screen-sized result never takes
//! sensor-sized work.

use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use rawler::imgop::sensor::bayer::Demosaic;
use rawler::imgop::sensor::bayer::superpixel::Superpixel3Channel;
use rawler::imgop::{Dim2, Point, Rect};
use rawler::pixarray::PixF32;
use rawler::rawimage::{RawImage, RawImageData, RawPhotometricInterpretation};
use rayon::prelude::*;

use crate::{Error, Result, Rgb};

/// How big a develop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scale {
    Full,
    /// Each 2 x 2 block of a Bayer mosaic one pixel. Anything else develops
    /// at full size.
    Half,
}

/// Bumped whenever `develop` would give a different picture, so cached
/// results made by an older one are not reused.
pub(crate) const DEVELOP_VERSION: u32 = 3;

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
// orientation, cancellation and the fast and linear-mode variants. At
// `Scale::Half`, `half_size` takes the place of rawler's demosaic.
pub(crate) fn develop(raw: &RawImage, scale: Scale) -> Result<Rgb> {
    let full_width = developed_size_px(raw).0;
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

    let developed = if scale == Scale::Half && bayer(&raw_image) {
        half_size(raw_image)?
    } else {
        let developed = developer
            .develop_intermediate(&raw_image)
            .map_err(|e| Error::Decode(e.to_string()))?;
        drop(raw_image);
        developed
    };

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    let dim = developed.dim();
    let (width, height) = (dim.w, dim.h);
    let scale = |v: f32| (v * rescale_factor).max(0.0).clamp(0.0, CLAMP_LIMIT);

    let pixels: Vec<[f32; 3]> = match developed {
        // In place: a second full-size picture here would be the develop's
        // largest allocation.
        Intermediate::ThreeColor(pixels) => {
            let mut data = pixels.data;
            data.par_iter_mut()
                .for_each(|p| *p = [scale(p[0]), scale(p[1]), scale(p[2])]);
            data
        }
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
    Ok(Rgb {
        full_width,
        ..Rgb::new(width, height, pixels)
    })
}

/// A Bayer mosaic: two by two, red, green and blue.
fn bayer(raw: &RawImage) -> bool {
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
        return false;
    };
    raw.cpp == 1
        && config.colors.plane_count() == 3
        && matches!(config.cfa.name.as_str(), "RGGB" | "BGGR" | "GBRG" | "GRBG")
}

/// `raw`, a Bayer mosaic, demosaiced at half size and taken through the rest
/// of rawler's develop.
///
/// The way previews are made below full size elsewhere: LibRaw's `half_size`
/// (RAWmakase's draft decode, `crates/rawmakase-native/src/raw/mod.rs`
/// @ 7c3c81ab, `Decode::Half`) and darktable's preview pipe
/// (`dt_iop_clip_and_zoom_demosaic_half_size_f`, `src/develop/imageop_math.c`
/// @ d5db8356)
/// both make each 2 x 2 block of photosites one pixel, its two greens
/// averaged, and so does RAWmakase's own reader since replacing LibRaw
/// (`crates/rawmakase-decode/src/demosaic.rs` @ 183e09fd, `half`,
/// pch/rawmakase#526). Here that is rawler's own `Superpixel3Channel`
/// (`src/imgop/sensor/bayer/superpixel.rs` @ 16b9b010). The steps around it
/// are rawler's `develop_intermediate` (`src/imgop/develop.rs` @ 16b9b010):
/// the levels scaled first, the active area as the demosaic's region; then
/// the half-size picture goes through white balance, the camera matrix and
/// the default crop as three colours a pixel, the crop scaled to it as
/// rawler scales it for its quarter-size demosaic.
fn half_size(mut raw: RawImage) -> Result<Intermediate> {
    let RawPhotometricInterpretation::Cfa(config) = raw.photometric.clone() else {
        return Err(Error::Decode("a half-size develop needs a mosaic".into()));
    };
    let whole = Rect::new(Point::new(0, 0), Dim2::new(raw.width, raw.height));
    let active = raw
        .active_area
        .filter(|a| {
            a.p.x + a.d.w <= raw.width && a.p.y + a.d.h <= raw.height && a.d.w >= 2 && a.d.h >= 2
        })
        .unwrap_or(whole);

    raw.apply_scaling()
        .map_err(|e| Error::Decode(e.to_string()))?;
    let RawImageData::Float(mosaic) =
        std::mem::replace(&mut raw.data, RawImageData::Float(Vec::new()))
    else {
        return Err(Error::Decode("the levels were not scaled".into()));
    };
    let mosaic = PixF32::new_with(mosaic, raw.width, raw.height);
    let half = Superpixel3Channel::new().demosaic(&mosaic, &config.cfa, &config.colors, active);
    drop(mosaic);

    // Within the active area (`adapt` asserts it), floored as rawler does.
    let mut crop = raw
        .crop_area
        .map(|c| c.intersection(&active))
        .filter(|c| c.d.w > 0 && c.d.h > 0)
        .map(|c| c.adapt(&active));
    if let Some(crop) = &mut crop {
        crop.scale(half.width as f32 / active.d.w as f32);
    }
    raw.width = half.width;
    raw.height = half.height;
    raw.cpp = 3;
    raw.photometric = RawPhotometricInterpretation::LinearRaw;
    raw.active_area = None;
    raw.crop_area = crop;
    raw.data = RawImageData::Float(half.data.into_flattened());

    let mut developer = RawDevelop::default();
    developer.steps.retain(|step| {
        matches!(
            step,
            ProcessingStep::WhiteBalance | ProcessingStep::Calibrate | ProcessingStep::CropDefault
        )
    });
    developer
        .develop_intermediate(&raw)
        .map_err(|e| Error::Decode(e.to_string()))
}

/// The size `develop` gives `raw`, from its geometry alone
/// (`developed_frame_px`).
pub(crate) fn developed_size_px(raw: &RawImage) -> (usize, usize) {
    let crop = developed_frame_px(raw).crop;
    (crop[2], crop[3])
}

/// Where the image `develop` gives `raw` lies, from its geometry alone: the
/// steps of rawler's `develop_intermediate` (rawler `src/imgop/develop.rs` @
/// 16b9b010) that decide it, without touching a pixel. A mosaic comes out of
/// demosaic at its active area, and the default crop is then taken.
pub(crate) fn developed_frame_px(raw: &RawImage) -> DevelopedFrame {
    use rawler::imgop::{Dim2, Point, Rect};

    let demosaics = !matches!(raw.photometric, RawPhotometricInterpretation::LinearRaw);
    let full = Rect::new(Point::new(0, 0), Dim2::new(raw.width, raw.height));
    let mosaic = raw.cpp == 1
        && matches!(&raw.photometric, RawPhotometricInterpretation::Cfa(c) if c.cfa.is_rgb());
    let decoded = if demosaics && mosaic {
        raw.active_area.unwrap_or(full).d
    } else {
        full.d
    };
    let mut developed = Rect::new(Point::new(0, 0), decoded);
    if let Some(mut crop) = raw.crop_area {
        if demosaics && let Some(active) = raw.active_area {
            crop = crop.intersection(&active).adapt(&active);
        }
        let original_width = raw.active_area.map(|a| a.d.w).unwrap_or(raw.width);
        if original_width > 0 {
            let scale = decoded.w as f32 / original_width as f32;
            if (scale - 1.0).abs() > 1e-6 {
                crop.scale(scale);
            }
        }
        if !crop.is_empty() && crop.d != decoded {
            developed = crop;
        }
    }
    DevelopedFrame {
        decoded: [decoded.w, decoded.h],
        crop: [developed.p.x, developed.p.y, developed.d.w, developed.d.h],
    }
}

/// The image `develop` makes, inside the one demosaic makes: the active area
/// of a mosaic, which a DNG's opcodes apply to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevelopedFrame {
    /// The demosaiced image's width and height, in pixels.
    pub decoded: [usize; 2],
    /// The developed image in it: left, top, width and height, in pixels.
    pub crop: [usize; 4],
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

/// Box-filter `rgb` to `w` x `h` pixels, or `None` when it is that size
/// already. Each output pixel is the plain average of the input pixels under
/// it. The pipeline asks for the size the whole photo fits at, whichever
/// develop the picture came from, so a preview is the same size either way.
pub(crate) fn resize(rgb: &Rgb, w: usize, h: usize) -> Option<Rgb> {
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

    const BLACK: u16 = 1000;
    const WHITE: u32 = 16000;

    /// A mosaic with no colour matrix, so a develop gives its demosaiced
    /// camera colours: repeatable noise of about 60 counts, 20 above black.
    fn dark_noisy_mosaic() -> (RawImage, Vec<u16>) {
        use rawler::cfa::PlaneColor;
        use rawler::decoders::Camera;
        use rawler::rawimage::{BlackLevel, CFAConfig, WhiteLevel};

        let (width, height) = (64, 64);
        let mut state = 12345_u32;
        let mut uniform = move || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 16) as f32 / 65536.0
        };
        let samples: Vec<u16> = (0..width * height)
            .map(|_| {
                let sum: f32 = (0..12).map(|_| uniform()).sum();
                (BLACK as f32 + 20.0 + (sum - 6.0) * 60.0) as u16
            })
            .collect();
        let cfa = rawler::CFA::new("RGGB");
        let colors = PlaneColor::new("RGB");
        let raw = RawImage::new_with_data(
            Camera {
                cfa: cfa.clone(),
                plane_color: colors.clone(),
                ..Default::default()
            },
            RawImageData::Integer(samples.clone()),
            width,
            height,
            1,
            [1.0; 4],
            RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &colors)),
            Some(BlackLevel::new(&[BLACK], 1, 1, 1)),
            Some(WhiteLevel::new(vec![WHITE])),
            false,
        );
        (raw, samples)
    }

    fn clipped_at_black(sample: u16) -> f32 {
        sample.saturating_sub(BLACK) as f32 / (WHITE - BLACK as u32) as f32
    }

    /// Noise below black is averaged by the demosaic and only then clipped,
    /// so a dark area comes out below what its samples average once each is
    /// clipped, and nothing comes out negative.
    #[test]
    fn a_full_develop_clips_noise_after_demosaicing() {
        let (raw, samples) = dark_noisy_mosaic();
        let clipped_first =
            samples.iter().map(|&v| clipped_at_black(v)).sum::<f32>() / samples.len() as f32;
        let out = develop(&raw, Scale::Full).unwrap().pixels;
        let mean = out.iter().flatten().sum::<f32>() / (out.len() * 3) as f32;
        assert!(
            mean < clipped_first * 0.95,
            "mean {mean}, clipped first {clipped_first}"
        );
        assert!(out.iter().flatten().all(|v| *v >= 0.0));
    }

    /// Each sample clipped, then the two greens averaged: at half size red
    /// and blue are single samples, so clipping after averaging would lower
    /// green alone and leave a dark area more magenta.
    #[test]
    fn a_half_develop_clips_each_sample() {
        let (raw, samples) = dark_noisy_mosaic();
        let out = develop(&raw, Scale::Half).unwrap().pixels;
        let at = |x: usize, y: usize| clipped_at_black(samples[y * 64 + x]);
        for (i, got) in out.iter().enumerate() {
            let (x, y) = (i % 32 * 2, i / 32 * 2);
            let green = (at(x + 1, y) + at(x, y + 1)) / 2.0;
            let want = [at(x, y), green, at(x + 1, y + 1)];
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 1e-6, "{got:?} is not {want:?}");
            }
        }
    }

    #[test]
    fn a_preview_keeps_its_shape_and_its_full_width() {
        let rgb = Rgb::new(600, 400, vec![[0.25, 0.5, 1.0]; 600 * 400]);
        let small = resize(&rgb, 150, 100).expect("smaller");
        assert_eq!(small.full_width, 600);
        assert_eq!(small.scale(), 0.25);
        assert!(small.pixels.iter().all(|p| *p == [0.25, 0.5, 1.0]));
        assert!(resize(&rgb, 600, 400).is_none(), "already that size");
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
