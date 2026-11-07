//! Argentum's GPU work on the input, before RapidRAW's shader runs. One anchor.
//!
//! `gpu_processing.rs` calls `run` once per render, after it has the input
//! texture and before its own passes, and renders whatever comes back - or
//! its own texture, untouched, when nothing does - with the request that
//! comes back. That call is the whole upstream cost of every multi-pass tool
//! Argentum will add: Noise Reduction and sharpening now, and the next thing
//! that needs whole-image passes rather than one pixel at a time (diffuse or
//! sharpen) goes in here, not in another line of theirs. See `modules.wgsl`
//! for the per-pixel stages, which are the same idea one level down. Decided
//! 2026-10-10.
//!
//! ORDER IS THE CONTRACT
//!
//! Tools run top to bottom as written in `develop`, each on the previous
//! one's output. Noise Reduction is first, so sharpening does not sharpen
//! the noise it would then fight (darktable's order; `denoise_gpu.rs`).
//! Sharpening is next because capture sharpening undoes the lens.

use std::sync::{Arc, Mutex};

use image::DynamicImage;

use super::clipping;
use super::denoise_gpu;
use super::sharpen;
use super::sharpen_gpu::{self, Job, MaskView, Target};
use crate::app_state::AppState;
use crate::gpu_processing::RenderRequest;
use crate::image_processing::GpuContext;

/// Renders that are thumbnails or small previews of a whole look - the photo
/// at a few hundred pixels, where sharpening is below what the image can
/// show - or not a photo at all. These are RapidRAW's own caller names.
/// Their Noise Reduction stays in RapidRAW's shader, where it was.
///
/// The LUT export renders an identity cube laid out as an image; sharpening it
/// would bend the colours of the exported LUT. Their export zeroes their own
/// sharpness for the same reason.
const NOT_STAGED: &[&str] = &[
    "generate_thumbnail_data",
    "generate_uncropped_preview",
    "generate_preset_preview",
    "generate_all_community_previews",
    "generate_lut_previews",
    "export_lut",
];

/// What the stage hands RapidRAW's shader.
pub struct Staged<'a> {
    /// The texture to read instead of theirs, when the stage made one.
    pub view: Option<wgpu::TextureView>,
    /// The request to render it with: without the global Noise Reduction
    /// when the stage has already run it.
    pub request: RenderRequest<'a>,
}

/// Where one render sits in the photo, for the tools that work in the full
/// picture's pixels.
pub struct At<'p> {
    /// Render pixels per full-resolution pixel.
    pub px_scale: f32,
    /// The full picture sharpening measures its contrast threshold on, and
    /// the key that measurement is kept under.
    pub measure_on: &'p DynamicImage,
    pub measure_key: u64,
    /// What the input's pixels are, and which photo: `sharpen_gpu::Job`'s
    /// `source` and `family`. 0 is unknown.
    pub source: u64,
    pub family: u64,
}

/// The anchor. Called from `process_and_get_dynamic_image_inner`, under their
/// processor lock, so the kept results can be reused between frames.
#[allow(clippy::too_many_arguments)]
pub fn run<'a>(
    context: &GpuContext,
    state: &tauri::State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    caller_id: &str,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: RenderRequest<'a>,
) -> Staged<'a> {
    if NOT_STAGED.contains(&caller_id) {
        return Staged {
            view: None,
            request,
        };
    }
    let (px_scale, full) = render_scale(state, transform_hash, width);
    let at = At {
        px_scale,
        measure_on: full.as_deref().unwrap_or(base_image),
        measure_key: transform_hash,
        // Which picture this is, whichever texture of theirs holds it: their
        // cache rebuilds the texture from the same image after every
        // thumbnail render.
        source: sharpen::picture_key(base_image, transform_hash),
        // The drag preview is a smaller copy of the same full picture.
        family: full
            .as_deref()
            .map_or(0, |full| sharpen::picture_key(full, transform_hash)),
    };
    let (staged, contrast) = develop(context, input, width, height, request, &at, Target::Kept);
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
    staged
}

/// For a render that builds its own processor outside their lock - the 16-bit
/// export. Always full resolution, and its textures are its own.
pub fn run_for_export<'a>(
    context: &GpuContext,
    base_image: &DynamicImage,
    input: &wgpu::TextureView,
    request: RenderRequest<'a>,
) -> Staged<'a> {
    let (width, height) = (base_image.width(), base_image.height());
    // Any hash will do for the measurement cache: its key holds a sample of
    // this image's bytes (sharpen::picture_key), which tells photos apart.
    let at = At {
        px_scale: 1.0,
        measure_on: base_image,
        measure_key: 0,
        source: 0,
        family: 0,
    };
    develop(context, input, width, height, request, &at, Target::Fresh).0
}

/// How many times a texture the stage handed back has been written over:
/// RapidRAW's blurs, which read it, are kept between renders on that
/// (`BlurSource`, gpu_processing.rs).
pub fn writes() -> u64 {
    denoise_gpu::writes() + sharpen_gpu::writes()
}

/// Forget every kept result, so the next render builds them afresh.
pub fn reset() {
    denoise_gpu::reset();
    sharpen_gpu::reset();
}

/// The stage on one render of `input`: Noise Reduction, then sharpening.
/// Also what auto found for sharpening's contrast threshold, when it looked.
pub fn develop<'a>(
    context: &GpuContext,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: RenderRequest<'a>,
    at: &At,
    target: Target,
) -> (Staged<'a>, Option<f32>) {
    struct Made {
        view: Option<wgpu::TextureView>,
        denoised: bool,
        contrast: Option<f32>,
    }
    let made = guarded(|| {
        let global = &request.adjustments.global;
        let denoise = denoise_gpu::Job {
            src: input,
            source: at.source,
            family: at.family,
            width,
            height,
            is_raw: global.is_raw_image == 1,
            luma: global.luma_noise_reduction,
            color: global.color_noise_reduction,
            px_scale: at.px_scale,
        };
        let denoised = denoise_gpu::run(context, &denoise, target);
        let (src, source) = match &denoised {
            Some(view) => (view, denoise.result_source()),
            None => (input, at.source),
        };
        let (sharpened, contrast) = sharpening(
            context,
            src,
            width,
            height,
            &request,
            at,
            (source, at.family),
            target,
        );
        Some(Made {
            denoised: denoised.is_some(),
            view: sharpened.or(denoised),
            contrast,
        })
    });
    let mut request = request;
    let Some(made) = made else {
        return (
            Staged {
                view: None,
                request,
            },
            None,
        );
    };
    if made.denoised {
        request.adjustments.global.luma_noise_reduction = 0.0;
        request.adjustments.global.color_noise_reduction = 0.0;
    }
    (
        Staged {
            view: made.view,
            request,
        },
        made.contrast,
    )
}

/// Never take their render down with ours.
///
/// This runs inside their preview worker, holding their processor lock. A
/// panic here poisons that lock and ends the worker thread, after which no
/// preview is drawn until the app restarts - which is what a bug in this
/// stage did on 2026-10-10. So a failure skips the stage for that frame,
/// says why in the log, and lets the engines start clean next time; their
/// render goes ahead on their own texture, with their own Noise Reduction.
fn guarded<T>(stage: impl FnOnce() -> Option<T>) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(stage)) {
        Ok(made) => made,
        Err(payload) => {
            let why = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".into());
            log::error!("[input stage] skipped this frame, it panicked: {why}");
            reset();
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sharpening(
    context: &GpuContext,
    input: &wgpu::TextureView,
    width: u32,
    height: u32,
    request: &RenderRequest,
    at: &At,
    (source, family): (u64, u64),
    target: Target,
) -> (Option<wgpu::TextureView>, Option<f32>) {
    let global = &request.adjustments.global;
    let params = global.ag_sharpen;
    let px_scale = at.px_scale;
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

    let thresholds = sharpen::thresholds(&params, at.measure_on, at.measure_key, is_raw);
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::sharpen_gpu::tests::{device, read, upload};

    /// A noisy grey field, the same every run.
    fn noisy(w: u32, h: u32) -> (Vec<[f32; 4]>, DynamicImage) {
        let px: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let n = ((i.wrapping_mul(2_654_435_761) >> 7) % 1000) as f32 / 1000.0 - 0.5;
                let v = 0.2 + 0.04 * n;
                [v, v, v, 1.0]
            })
            .collect();
        let image = DynamicImage::ImageRgb32F(image::ImageBuffer::from_fn(w, h, |x, y| {
            let [r, g, b, _] = px[(y * w + x) as usize];
            image::Rgb([r, g, b])
        }));
        (px, image)
    }

    fn spread(px: &[[f32; 4]]) -> f32 {
        let n = px.len() as f32;
        let mean = px.iter().map(|p| p[1]).sum::<f32>() / n;
        (px.iter().map(|p| (p[1] - mean).powi(2)).sum::<f32>() / n).sqrt()
    }

    fn request(luma_nr: u32) -> RenderRequest<'static> {
        RenderRequest {
            adjustments: crate::image_processing::get_all_adjustments_from_json(
                &serde_json::json!({
                    "agSharpen": { "amount": 100 },
                    "lumaNoiseReduction": luma_nr,
                }),
                true,
                crate::as_shot::AsShot::reference(),
                Default::default(),
                None,
            ),
            mask_bitmaps: &[],
            lut: None,
            roi: None,
        }
    }

    /// Noise Reduction runs on the input, before sharpening sees it: the
    /// staged texture of a noisy grey field is much quieter with it, and
    /// RapidRAW's shader is told not to run it a second time. At 0 nothing
    /// changes. Called by `sharpen_gpu`'s `every_backend_we_offer`.
    pub(crate) fn noise_reduction_runs_before_sharpening(ctx: &GpuContext) {
        let (w, h) = (96u32, 64u32);
        let (px, image) = noisy(w, h);
        let src = upload(ctx, w, h, &px);
        let at = At {
            px_scale: 1.0,
            measure_on: &image,
            measure_key: 0,
            source: 0,
            family: 0,
        };
        let stage = |luma_nr| develop(ctx, &src, w, h, request(luma_nr), &at, Target::Fresh).0;

        let sharpened = stage(0);
        assert_eq!(
            sharpened.request.adjustments.global.luma_noise_reduction,
            0.0
        );
        let sharpened = read(ctx, sharpened.view.as_ref().expect("sharpened"), w, h);

        let denoised = stage(100);
        assert_eq!(
            denoised.request.adjustments.global.luma_noise_reduction, 0.0,
            "their shader must not run the global amount again"
        );
        let denoised = read(ctx, denoised.view.as_ref().expect("staged"), w, h);
        assert!(
            spread(&denoised) < spread(&sharpened) * 0.5,
            "denoised before sharpening: {} against {}",
            spread(&denoised),
            spread(&sharpened)
        );
    }

    /// A mask's own Noise Reduction still runs, in RapidRAW's shader, on top
    /// of a global amount the stage ran first: through the whole render, the
    /// half of a noisy grey field under the mask comes out much quieter than
    /// the other. Sharpening off: after it, this noise is past what RapidRAW's
    /// maths smooths, which is why the global amounts moved before it.
    /// Called by `sharpen_gpu`'s `every_backend_we_offer`.
    pub(crate) fn a_masks_own_noise_reduction_still_runs(ctx: &GpuContext) {
        let (w, h) = (128u32, 64u32);
        let (_, image) = noisy(w, h);
        let mask = sharpen_gpu::MaskBitmap::from_fn(w, h, |x, _| {
            image::Luma([if x < w / 2 { 255 } else { 0 }])
        });
        let js = serde_json::json!({
            "agSharpen": { "capture": false, "amount": 0 },
            "lumaNoiseReduction": 20,
            "masks": [{
                "id": "m1",
                "name": "Mask",
                "visible": true,
                "invert": false,
                "adjustments": { "lumaNoiseReduction": 100 },
                "subMasks": [{
                    "id": "s1",
                    "type": "radial",
                    "visible": true,
                    "mode": "additive",
                    "parameters": {},
                }],
            }],
        });
        let request = RenderRequest {
            adjustments: crate::image_processing::get_all_adjustments_from_json(
                &js,
                true,
                crate::as_shot::AsShot::reference(),
                Default::default(),
                None,
            ),
            mask_bitmaps: std::slice::from_ref(&mask),
            lut: None,
            roi: None,
        };
        let out = crate::mods::export_precision::render_high_precision(ctx, &image, request)
            .expect("renders");
        let out = out.as_rgba32f().expect("float");
        // Away from the frame's edge and the mask's.
        let half = |x0: u32| -> Vec<[f32; 4]> {
            (8..h - 8)
                .flat_map(|y| (x0 + 8..x0 + w / 2 - 8).map(move |x| (x, y)))
                .map(|(x, y)| out.get_pixel(x, y).0)
                .collect()
        };
        let (inside, outside) = (spread(&half(0)), spread(&half(w / 2)));
        assert!(
            inside < outside * 0.6,
            "the mask's own Noise Reduction: {inside} inside against {outside} outside"
        );
    }

    /// A photo that is not a RAW (JPEG, HEIC, TIFF) comes in sRGB-encoded and
    /// leaves the pass the same way: quieter, no brighter or darker.
    /// Called by `sharpen_gpu`'s `every_backend_we_offer`.
    pub(crate) fn a_non_raw_photo_keeps_its_encoding(ctx: &GpuContext) {
        let (w, h) = (96u32, 64u32);
        let (px, image) = noisy(w, h);
        let src = upload(ctx, w, h, &px);
        let at = At {
            px_scale: 1.0,
            measure_on: &image,
            measure_key: 0,
            source: 0,
            family: 0,
        };
        let request = RenderRequest {
            adjustments: crate::image_processing::get_all_adjustments_from_json(
                &serde_json::json!({
                    "agSharpen": { "capture": false, "amount": 0 },
                    "lumaNoiseReduction": 100,
                }),
                false,
                crate::as_shot::AsShot::reference(),
                Default::default(),
                None,
            ),
            mask_bitmaps: &[],
            lut: None,
            roi: None,
        };
        let staged = develop(ctx, &src, w, h, request, &at, Target::Fresh).0;
        let out = read(ctx, staged.view.as_ref().expect("denoised"), w, h);
        let mean = |px: &[[f32; 4]]| px.iter().map(|p| p[1]).sum::<f32>() / px.len() as f32;
        assert!(
            (mean(&out) - mean(&px)).abs() < 0.005,
            "the level moved: {} from {}",
            mean(&out),
            mean(&px)
        );
        assert!(spread(&out) < spread(&px) * 0.5);
        assert!(out.iter().all(|p| p[3] == 1.0), "alpha kept");
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn noise_reduction_runs_before_sharpening_here() {
        let ctx = device(false).expect("a GPU");
        noise_reduction_runs_before_sharpening(&ctx);
        a_masks_own_noise_reduction_still_runs(&ctx);
        a_non_raw_photo_keeps_its_encoding(&ctx);
    }
}
