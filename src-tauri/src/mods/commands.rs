//! Tauri commands for Argentum's own features.

use crate::app_state::AppState;
use crate::get_cached_full_warped_image;
use crate::mods::auto_wb::{self, AutoWhiteBalance, DetectMode};

/// Solve white balance from the point the user clicked, in normalised image
/// coordinates (0..1, origin top-left).
///
/// Takes coordinates rather than a sampled colour on purpose, and this is the
/// fix for the picker never settling. The frontend used to read the pixel off
/// the *processed preview* and send that — an image with the current white
/// balance already applied, plus exposure, curves and everything else. Each
/// click therefore solved from its own previous output, moved the sliders, and
/// changed what the next click would see. Clicking one spot repeatedly wandered
/// forever instead of converging.
///
/// Sampling here uses the geometry-only cache, keyed on crop and rotation, which
/// no colour slider can move. Same point, same answer. It is also the exact
/// image auto-WB analyses, so the wand and the picker finally agree.
#[tauri::command]
pub async fn solve_white_balance_at_point(
    x: f32,
    y: f32,
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
) -> Result<AutoWhiteBalance, String> {
    let image = get_cached_full_warped_image(&state, &js_adjustments)?;

    let result = auto_wb::white_balance_at(image.as_ref(), x, y)
        .ok_or_else(|| "That colour is too dark or too saturated to balance from".to_string())?;

    log::info!(
        "[wb_picker] ({:.3}, {:.3}) -> illuminant xy ({:.4}, {:.4}) ~{:.0}K -> temp {:.1} tint {:.1}",
        x,
        y,
        result.x,
        result.y,
        result.temperature_k,
        result.temperature,
        result.tint
    );

    Ok(result)
}

/// Detect the scene illuminant and return the white balance that neutralises it.
///
/// Runs on the geometry-corrected image already cached for the editor, so it
/// sees the same pixels you do — crop and rotation included.
#[tauri::command]
pub async fn detect_auto_white_balance(
    js_adjustments: serde_json::Value,
    mode: DetectMode,
    state: tauri::State<'_, AppState>,
) -> Result<AutoWhiteBalance, String> {
    let image = get_cached_full_warped_image(&state, &js_adjustments)?;

    // That image has already been through `apply_cpu_default_raw_processing`,
    // which gamma-encodes and boosts contrast. The detection needs scene-linear
    // data, so ask it to undo that first — see `to_scene_linear`.
    let started = std::time::Instant::now();
    let result = auto_wb::auto_white_balance(image.as_ref(), mode, true)
        .ok_or_else(|| "Could not detect an illuminant in this image".to_string())?;

    log::info!(
        "[auto_wb] {:?}: illuminant xy ({:.4}, {:.4}) ~{:.0}K -> temp {:.1} tint {:.1} in {:.1?}",
        mode,
        result.x,
        result.y,
        result.temperature_k,
        result.temperature,
        result.tint,
        started.elapsed()
    );

    Ok(result)
}

/// Sample the RGB of the finished picture at a point, in normalised image
/// coordinates (0..1, origin top-left).
///
/// This exists because the displayed photo cannot be read from the frontend at
/// all. RapidRAW renders the editor view to a **native WGPU surface composited
/// behind the webview** — there is no canvas and no `<img>` carrying the live
/// result. An earlier readout sampled the only large image in the DOM, which is
/// the cached `_medium.jpg` thumbnail, regenerated on save and never while a
/// slider moves. It read 207/177/179 at correct white balance and 214/189/192
/// at temperature -100, on a photo that had gone completely blue.
///
/// So the pixel has to be produced here, by actually rendering. A one-pixel ROI
/// keeps that cheap: the GPU does the same work it would for the preview, over
/// a single texel.
///
/// **Masks are not applied.** They need bitmaps assembled per render, and this
/// is a measuring instrument for global colour — white balance, and later DCP,
/// highlight recovery and filmic. If a masked reading is ever wanted, that is
/// where to start.
#[tauri::command]
pub async fn sample_processed_pixel(
    x: f32,
    y: f32,
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<[u8; 3], String> {
    let context = crate::gpu_processing::get_or_init_gpu_context(&state, &app_handle)?;

    let mut adjustments = js_adjustments.clone();
    crate::adjustment_utils::hydrate_adjustments(&state, &mut adjustments);

    let base = crate::get_cached_full_warped_image(&state, &adjustments)?;
    let (width, height) = (base.width(), base.height());
    if width == 0 || height == 0 {
        return Err("No image to sample".to_string());
    }

    let px = (x.clamp(0.0, 1.0) * (width - 1) as f32).round() as u32;
    let py = (y.clamp(0.0, 1.0) * (height - 1) as f32).round() as u32;

    let loaded = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded")?;

    let tonemapper =
        crate::image_processing::resolve_tonemapper_override_from_handle(&app_handle, loaded.is_raw);
    let mut all = crate::image_processing::get_all_adjustments_from_json(
        &adjustments,
        loaded.is_raw,
        tonemapper,
    );
    // Clipping indicators paint pure red or blue over blown pixels, which would
    // be read back as if it were the colour of the picture.
    all.global.show_clipping = 0;

    let lut = adjustments["lutPath"]
        .as_str()
        .and_then(|p| crate::lut_processing::get_or_load_lut(&state, p).ok());

    // A distinct hash, so this never collides with the preview's own cache entry.
    let hash = crate::cache_utils::calculate_full_job_hash(&loaded.path, &adjustments)
        .wrapping_add(0x5247_4250);

    let processed = crate::gpu_processing::process_and_get_dynamic_image(
        &context,
        &state,
        base.as_ref(),
        hash,
        crate::gpu_processing::RenderRequest {
            adjustments: all,
            mask_bitmaps: &[],
            lut,
            roi: Some(crate::gpu_processing::Roi {
                x: px,
                y: py,
                width: 1,
                height: 1,
            }),
        },
        "sample_processed_pixel",
    )?;

    let rgb = processed.to_rgb8();
    let p = rgb.get_pixel(0, 0);
    Ok([p[0], p[1], p[2]])
}
