//! RAW tone rendering modes.
//!
//! Default leaves RapidRAW/Argentum's existing RAW view transform alone.
//! Camera Base Curve is a small camera-style view curve. Auto-Matched fits a
//! monotonic tone curve from the scene-linear RAW and the camera JPEG embedded
//! in the same file. DNG Standard is a DNG renderer's default: the camera's
//! exposure offset and Adobe's default tone curve, applied as Adobe applies it
//! (see dng_standard.rs). The curve is applied on the GPU, in `ag_stage_tone`
//! (modules.wgsl); this module only builds the small per-photo data that
//! crosses into the renderer.

use crate::app_settings::AppSettings;
use crate::image_processing::downscale_f32_image;
use image::{DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ToneCurvePoint {
    pub x: f32,
    pub y: f32,
}

/// A restrained Canon-style S curve used when no camera-specific curve has
/// been imported. It is deliberately exposed as a curve, not folded into the
/// shader, so a later per-camera table can replace it without changing the
/// render path.
pub fn base_curve() -> Vec<ToneCurvePoint> {
    vec![
        ToneCurvePoint { x: 0.0, y: 0.0 },
        ToneCurvePoint { x: 8.0, y: 3.0 },
        ToneCurvePoint { x: 24.0, y: 14.0 },
        ToneCurvePoint { x: 48.0, y: 35.0 },
        ToneCurvePoint { x: 80.0, y: 70.0 },
        ToneCurvePoint { x: 112.0, y: 108.0 },
        ToneCurvePoint { x: 144.0, y: 148.0 },
        ToneCurvePoint { x: 176.0, y: 181.0 },
        ToneCurvePoint { x: 208.0, y: 211.0 },
        ToneCurvePoint { x: 240.0, y: 240.0 },
        ToneCurvePoint { x: 255.0, y: 255.0 },
    ]
}

/// The number the shader switches on, `raw_tone_mode` in their
/// `GlobalAdjustments`. 0 is RapidRAW's own rendering.
pub fn mode_index(mode: &str) -> u32 {
    match mode {
        "baseCurve" => 1,
        "autoMatched" => 2,
        crate::mods::dng_standard::MODE => 3,
        _ => 0,
    }
}

/// Which rendering an edit asks for, as `mode_index` numbers it.
///
/// An edit that does not say starts in DNG Standard, as the editor starts one
/// (`INITIAL_ADJUSTMENTS` in adjustments.ts). That covers a photo nobody has
/// edited - no adjustments at all - so its thumbnail and an export of it match
/// what opening it shows, and an edit built from nothing by their library, such
/// as settings pasted onto an unedited photo, which holds only what was pasted.
/// Every edit saved since the choice existed (2026-09-22) names its rendering,
/// so RapidRAW's stays wherever it was chosen.
pub fn mode_of(js: &Value) -> u32 {
    js.get("rawToneRendering")
        .and_then(Value::as_str)
        .map_or_else(|| mode_index(crate::mods::dng_standard::MODE), mode_index)
}

/// The points a mode needs when the edit carries none: Base Curve's fixed
/// curve, or DNG Standard's for this photo's camera. Auto-Matched has no stand-in
/// and renders RapidRAW's until it has been fitted.
pub fn fill_missing(mode: u32, points: &mut Vec<Value>, photo: Option<&str>) {
    if points.len() >= 2 {
        return;
    }
    let curve = match mode {
        1 => base_curve(),
        3 => crate::mods::dng_standard::curve_for_photo(photo),
        _ => return,
    };
    *points = curve
        .into_iter()
        .map(|p| json!({ "x": p.x, "y": p.y }))
        .collect();
}

pub fn curve_for(
    bytes: &[u8],
    path: &str,
    mode: &str,
    settings: &AppSettings,
) -> Result<Vec<ToneCurvePoint>, String> {
    match mode {
        "baseCurve" => Ok(base_curve()),
        "autoMatched" => auto_matched_curve(bytes, path, settings),
        crate::mods::dng_standard::MODE => crate::mods::dng_standard::curve(bytes),
        _ => Err(format!("unknown RAW tone rendering mode: {mode}")),
    }
}

/// Fit tone by matching luminance quantiles between the decoded RAW and the
/// camera JPEG. A monotonic fit is the useful part of RawTherapee's
/// Auto-Matched Curve: it borrows the camera's tonal distribution without
/// importing JPEG sharpening or JPEG noise reduction into the RAW pipeline.
fn auto_matched_curve(
    bytes: &[u8],
    path: &str,
    settings: &AppSettings,
) -> Result<Vec<ToneCurvePoint>, String> {
    let raw = crate::raw_processing::develop_raw_image(
        bytes,
        false,
        settings.raw_highlight_compression.unwrap_or(2.5),
        settings.linear_raw_mode.clone(),
        None,
        Some(path),
    )
    .map_err(|e| format!("could not decode RAW for auto-match: {e}"))?;

    let jpeg = crate::image_loader::embedded_preview_fallback(bytes)
        .ok_or_else(|| "this RAW has no readable embedded JPEG preview".to_string())?;

    let raw = bounded_image(raw);
    let jpeg = bounded_image(jpeg);
    let raw_values = luminance_samples(&raw, true);
    let jpeg_values = luminance_samples(&jpeg, false);
    if raw_values.len() < 32 || jpeg_values.len() < 32 {
        return Err("embedded preview is too small for an auto-matched curve".to_string());
    }

    let mut raw_values = raw_values;
    let mut jpeg_values = jpeg_values;
    raw_values.sort_unstable_by(|a, b| a.total_cmp(b));
    jpeg_values.sort_unstable_by(|a, b| a.total_cmp(b));

    const QUANTILES: [f32; 13] = [
        0.0, 0.01, 0.03, 0.07, 0.15, 0.27, 0.42, 0.58, 0.73, 0.85, 0.93, 0.99, 1.0,
    ];
    Ok(QUANTILES
        .into_iter()
        .map(|q| ToneCurvePoint {
            x: quantile(&raw_values, q) * 255.0,
            y: quantile(&jpeg_values, q) * 255.0,
        })
        .collect())
}

fn bounded_image(image: DynamicImage) -> DynamicImage {
    const MAX_DIM: u32 = 512;
    let (width, height) = image.dimensions();
    if width <= MAX_DIM && height <= MAX_DIM {
        image
    } else if width >= height {
        downscale_f32_image(
            &image,
            MAX_DIM,
            (height as f32 * MAX_DIM as f32 / width as f32) as u32,
        )
    } else {
        downscale_f32_image(
            &image,
            (width as f32 * MAX_DIM as f32 / height as f32) as u32,
            MAX_DIM,
        )
    }
}

fn luminance_samples(image: &DynamicImage, linear: bool) -> Vec<f32> {
    let rgb = image.to_rgb32f();
    let stride = ((rgb.width().max(rgb.height()) as usize).div_ceil(512)).max(1);
    rgb.enumerate_pixels()
        .filter(|(x, y, _)| {
            (*x as usize).is_multiple_of(stride) && (*y as usize).is_multiple_of(stride)
        })
        .map(|(_, _, p)| {
            let luma = (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).max(0.0);
            let encoded = if linear { linear_to_srgb(luma) } else { luma };
            encoded.clamp(0.0, 1.0)
        })
        .collect()
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn quantile(values: &[f32], q: f32) -> f32 {
    let at = q.clamp(0.0, 1.0) * (values.len().saturating_sub(1) as f32);
    let lo = at.floor() as usize;
    let hi = at.ceil() as usize;
    if lo == hi {
        values[lo]
    } else {
        values[lo] + (values[hi] - values[lo]) * (at - lo as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_that_does_not_name_a_rendering_starts_in_dng_standard() {
        assert_eq!(mode_of(&Value::Null), 3);
        assert_eq!(mode_of(&json!({})), 3);
        // Settings pasted onto a photo nobody had edited.
        assert_eq!(mode_of(&json!({ "exposure": 0.3 })), 3);
        // RapidRAW's stays wherever it was chosen.
        assert_eq!(mode_of(&json!({ "rawToneRendering": "default" })), 0);
        assert_eq!(mode_of(&json!({ "rawToneRendering": "baseCurve" })), 1);
        assert_eq!(mode_of(&json!({ "rawToneRendering": "dngStandard" })), 3);
    }

    #[test]
    fn missing_points_are_filled_only_for_modes_that_have_them() {
        let mut points = Vec::new();
        fill_missing(3, &mut points, None);
        assert_eq!(points.len(), 16);
        let mut points = Vec::new();
        fill_missing(1, &mut points, None);
        assert_eq!(points.len(), base_curve().len());
        let mut points = Vec::new();
        fill_missing(2, &mut points, None);
        assert!(points.is_empty(), "Auto-Matched has nothing to stand in");
        let mut kept = vec![json!({ "x": 0, "y": 0 }), json!({ "x": 255, "y": 200 })];
        fill_missing(3, &mut kept, None);
        assert_eq!(kept[1]["y"], 200, "a stored curve is never replaced");
    }

    #[test]
    fn base_curve_is_monotonic_and_bounded() {
        let curve = base_curve();
        assert!(
            curve
                .windows(2)
                .all(|w| w[0].x <= w[1].x && w[0].y <= w[1].y)
        );
        assert_eq!(curve.first().unwrap().x, 0.0);
        assert_eq!(curve.last().unwrap().y, 255.0);
    }
}
