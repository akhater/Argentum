//! Argentum's GPU work on the input, before RapidRAW's shader runs. One anchor.
//!
//! `gpu_processing.rs` calls `run` once per render, after it has the input
//! texture and before its own passes, and renders whatever comes back - or
//! its own texture, untouched, when `None` does. That call is the whole
//! upstream cost of every multi-pass tool Argentum will add: sharpening now,
//! and the next thing that needs whole-image passes rather than one pixel at a
//! time (denoise, diffuse or sharpen) goes in here, not in another line of
//! theirs. See `modules.wgsl` for the per-pixel stages, which are the same
//! idea one level down. AK's decision, 2026-10-10.
//!
//! ORDER IS THE CONTRACT
//!
//! Tools run top to bottom as written in `run`, each on the previous one's
//! output. Sharpening is first because capture sharpening undoes the lens; a
//! denoiser would go before it, so it is not sharpening the noise.

use std::sync::Arc;

use image::DynamicImage;

use super::clipping;
use super::sharpen;
use super::sharpen_gpu::{self, Job, MaskView, Target};
use crate::app_state::AppState;
use crate::gpu_processing::RenderRequest;
use crate::image_processing::GpuContext;

/// Renders that are thumbnails or small previews of a whole look - the photo
/// at a few hundred pixels, where sharpening is below what the image can show
/// - or not a photo at all. These are RapidRAW's own caller names.
///
/// The LUT export renders an identity cube laid out as an image; sharpening it
/// would bend the colours of the exported LUT. Their export zeroes their own
/// sharpness for the same reason.
const NOT_SHARPENED: &[&str] = &[
    "generate_thumbnail_data",
    "generate_uncropped_preview",
    "generate_preset_preview",
    "generate_all_community_previews",
    "generate_lut_previews",
    "export_lut",
];

/// The anchor. Called from `process_and_get_dynamic_image_inner`, under their
/// processor lock, so the kept result can be reused between frames.
#[allow(clippy::too_many_arguments)]
pub fn run(
    context: &GpuContext,
    state: &tauri::State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    caller_id: &str,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: &RenderRequest,
) -> Option<wgpu::TextureView> {
    if NOT_SHARPENED.contains(&caller_id) {
        return None;
    }
    let (px_scale, full) = render_scale(state, transform_hash, width);
    let (view, contrast) = sharpening(
        context,
        input,
        width,
        height,
        request,
        px_scale,
        full.as_deref().unwrap_or(base_image),
        transform_hash,
        Target::Kept,
    );
    // The transform cache matched, so this is the editor's render of the open
    // photo: what auto found is what its slider should say. `try_lock`
    // because this runs under their processor lock, and a readout is not
    // worth any risk of waiting on another.
    if let Some(threshold) = contrast
        && full.is_some()
        && let Ok(open) = state.original_image.try_lock()
        && let Some(open) = open.as_ref()
    {
        sharpen::remember_shown_contrast(&open.path, threshold);
    }
    view
}

/// For a render that builds its own processor outside their lock - the 16-bit
/// export. Always full resolution, and its texture is its own.
pub fn run_for_export(
    context: &GpuContext,
    base_image: &DynamicImage,
    input: &wgpu::TextureView,
    request: &RenderRequest,
) -> Option<wgpu::TextureView> {
    let (width, height) = (base_image.width(), base_image.height());
    // Any hash will do for the measurement cache as long as it is this
    // image's; the buffer address in the key tells renders apart.
    sharpening(
        context,
        input,
        width,
        height,
        request,
        1.0,
        base_image,
        0,
        Target::Fresh,
    )
    .0
}

#[allow(clippy::too_many_arguments)]
fn sharpening(
    context: &GpuContext,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: &RenderRequest,
    px_scale: f32,
    measure_on: &DynamicImage,
    measure_key: u64,
    target: Target,
) -> (Option<wgpu::TextureView>, Option<f32>) {
    let global = &request.adjustments.global;
    let params = global.ag_sharpen;
    let mask_view = match global.show_clipping {
        clipping::SHARPEN_MASK => MaskView::Capture,
        clipping::SHARPEN_MASK_USM => MaskView::Sharpen,
        _ => MaskView::Off,
    };
    let is_raw = global.is_raw_image == 1;
    if !sharpen_gpu::needs_work(&params, width, height, px_scale, mask_view) {
        // Nothing to measure, nothing to run - and let the engine drop what it
        // was keeping. Asked before measuring: at fit-to-screen a RAW's
        // capture sharpening is skipped, and the measurement is a full pass
        // over the full-resolution photo that would have bought nothing.
        let none = sharpen_gpu::run(
            context,
            &Job {
                src: input,
                width,
                height,
                is_raw,
                params,
                contrast: 0.0,
                usm_contrast: 0.0,
                px_scale,
                mask_view: MaskView::Off,
                masks: &[],
                region: None,
            },
            target,
        );
        return (none, None);
    }

    let thresholds = sharpen::thresholds(&params, measure_on, measure_key, is_raw);
    let region = request.roi.as_ref().map(|r| [r.x, r.y, r.width, r.height]);
    let view = sharpen_gpu::run(
        context,
        &Job {
            src: input,
            width,
            height,
            is_raw,
            params,
            contrast: thresholds.capture,
            usm_contrast: thresholds.usm,
            px_scale,
            mask_view,
            masks: request.mask_bitmaps,
            region,
        },
        target,
    );
    (view, thresholds.measured)
}

/// How many render pixels there are per full-resolution pixel, and the
/// full-resolution picture when there is one to measure on.
///
/// The editor's preview is RapidRAW's full-resolution transformed image,
/// downscaled - and it keeps that image, under the same transform hash the
/// render is given. When they match, this render is that picture at
/// `width / full width`. When they do not, the render is an export or a
/// readout of the full image, which is 1:1. The callers that render small
/// previews of other photos are in `NOT_SHARPENED`.
fn render_scale(
    state: &tauri::State<AppState>,
    transform_hash: u64,
    width: u32,
) -> (f32, Option<Arc<DynamicImage>>) {
    let cache = state
        .full_transformed_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match cache.as_ref() {
        Some((hash, full, _)) if *hash == transform_hash && full.width() > 0 => (
            (width as f32 / full.width() as f32).min(1.0),
            Some(Arc::clone(full)),
        ),
        _ => (1.0, None),
    }
}
