//! Argentum's GPU work on the input, before RapidRAW's shader runs. One anchor.
//!
//! `gpu_processing.rs` calls `run` once per render, after it has the input
//! texture and before its own passes, and renders whatever comes back - or
//! its own texture, untouched, when `None` does. That call is the whole
//! upstream cost of every multi-pass tool Argentum will add: sharpening now,
//! and the next thing that needs whole-image passes rather than one pixel at a
//! time (denoise, diffuse or sharpen) goes in here, not in another line of
//! theirs. See `modules.wgsl` for the per-pixel stages, which are the same
//! idea one level down. Decided 2026-10-10.
//!
//! ORDER IS THE CONTRACT
//!
//! Tools run top to bottom as written in `run`, each on the previous one's
//! output. Sharpening is first because capture sharpening undoes the lens; a
//! denoiser would go before it, so it is not sharpening the noise.

use std::sync::{Arc, Mutex};

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
    guarded(|| {
        stage(
            context,
            state,
            base_image,
            transform_hash,
            input,
            width,
            height,
            request,
        )
    })
}

/// Never take their render down with ours.
///
/// This runs inside their preview worker, holding their processor lock. A
/// panic here poisons that lock and ends the worker thread, after which no
/// preview is drawn until the app restarts - which is what a bug in this
/// stage did on 2026-10-10. So a failure skips sharpening for that frame,
/// says why in the log, and lets the engine start clean next time; their
/// render goes ahead on their own texture.
fn guarded(stage: impl FnOnce() -> Option<wgpu::TextureView>) -> Option<wgpu::TextureView> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(stage)) {
        Ok(view) => view,
        Err(payload) => {
            let why = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".into());
            log::error!("[sharpen] skipped this frame, the input stage panicked: {why}");
            sharpen_gpu::reset();
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stage(
    context: &GpuContext,
    state: &tauri::State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: &RenderRequest,
) -> Option<wgpu::TextureView> {
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
        // Which picture this is, whichever texture of theirs holds it: their
        // cache rebuilds the texture from the same image after every
        // thumbnail render.
        (
            sharpen::picture_key(base_image, transform_hash),
            // The drag preview is a smaller copy of the same full picture.
            full.as_deref()
                .map_or(0, |full| sharpen::picture_key(full, transform_hash)),
        ),
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
    // Any hash will do for the measurement cache: its key holds a sample of
    // this image's bytes (sharpen::picture_key), which tells photos apart.
    guarded(|| {
        sharpening(
            context,
            input,
            width,
            height,
            request,
            1.0,
            base_image,
            0,
            (0, 0),
            Target::Fresh,
        )
        .0
    })
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
    (source, family): (u64, u64),
    target: Target,
) -> (Option<wgpu::TextureView>, Option<f32>) {
    let global = &request.adjustments.global;
    let params = global.ag_sharpen;
    let mask_view = match global.show_clipping {
        clipping::SHARPEN_MASK => MaskView::Capture,
        clipping::SHARPEN_MASK_USM => MaskView::Sharpen,
        clipping::SHARPEN_MASK_LOCAL if params.local_show > 0 => {
            MaskView::Local(params.local_show as usize - 1)
        }
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
                source: 0,
                family: 0,
                width,
                height,
                is_raw,
                params,
                contrast: 0.0,
                usm_contrast: 0.0,
                local_contrast: [0.0; sharpen::MAX_LOCAL],
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
            source,
            family,
            width,
            height,
            is_raw,
            params,
            contrast: thresholds.capture,
            usm_contrast: thresholds.usm,
            local_contrast: thresholds.local,
            px_scale,
            mask_view,
            masks: request.mask_bitmaps,
            region,
        },
        target,
    );
    (view, thresholds.measured)
}

/// The full-resolution picture the editor's preview is a smaller copy of: the
/// open photo, warped, rotated and cropped, under its transform. RapidRAW kept
/// it in `full_transformed_cache` until 1.6.6, whose staged preview (lib.rs
/// `compute_transformed_preview`) crops a smaller working copy instead. Kept
/// here so sharpening still measures the preview on what the export renders.
static FULL_PICTURE: Mutex<Option<(u64, Arc<DynamicImage>)>> = Mutex::new(None);

fn photo_key(path: &str, transform_hash: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    transform_hash.hash(&mut h);
    h.finish()
}

/// The key of the open photo under this transform. `try_lock`, because this
/// runs under their processor lock: no key, no match, and the render is taken
/// as 1:1, which is what it is for every render but the editor's.
fn open_photo_key(state: &tauri::State<AppState>, transform_hash: u64) -> Option<u64> {
    let open = state.original_image.try_lock().ok()?;
    Some(photo_key(&open.as_ref()?.path, transform_hash))
}

/// Keep the full picture of `path` under `transform_hash`, making it with
/// `make` only when the one kept is for another photo or transform.
pub fn keep_full_picture(
    path: &str,
    transform_hash: u64,
    make: impl FnOnce() -> Arc<DynamicImage>,
) {
    let key = photo_key(path, transform_hash);
    let mut kept = FULL_PICTURE.lock().unwrap_or_else(|e| e.into_inner());
    if kept.as_ref().is_none_or(|(hash, _)| *hash != key) {
        *kept = None;
        *kept = Some((key, make()));
    }
}

/// Drop the kept picture: the photo's pixels changed (mods/redecode.rs).
pub fn forget_full_picture() {
    *FULL_PICTURE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The kept picture, for the memory count (mods/memory.rs).
pub fn full_picture() -> Option<Arc<DynamicImage>> {
    FULL_PICTURE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(_, img)| Arc::clone(img))
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
    let unmatched = || (EXPORT_SCALE.get(), None);
    let Some(key) = open_photo_key(state, transform_hash) else {
        return unmatched();
    };
    let kept = FULL_PICTURE.lock().unwrap_or_else(|e| e.into_inner());
    match kept.as_ref() {
        Some((hash, full)) if *hash == key && full.width() > 0 => (
            (width as f32 / full.width() as f32).min(1.0),
            Some(Arc::clone(full)),
        ),
        _ => unmatched(),
    }
}

thread_local! {
    /// Render pixels per full-resolution pixel of an export developed below
    /// full size; 1.0 for every other render.
    static EXPORT_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Run `render` as an export developed at `px_scale` of full resolution - the
/// headless export, which develops at the size it writes - so sharpening's
/// radii shrink with the picture, as they do for the editor's preview.
pub fn at_export_scale<R>(px_scale: f32, render: impl FnOnce() -> R) -> R {
    let previous = EXPORT_SCALE.replace(px_scale.clamp(1.0e-3, 1.0));
    let result = render();
    EXPORT_SCALE.set(previous);
    result
}
