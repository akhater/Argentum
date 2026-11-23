use std::io::Cursor;

use base64::{Engine as _, engine::general_purpose};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage, RgbaImage};
use serde_json::Value;

use crate::ai_connector;
use crate::ai_processing;
use crate::app_settings::load_settings;
use crate::app_state::AppState;
use crate::image_loader::composite_patches_on_image;
use crate::image_processing::apply_linear_to_srgb;
use crate::mask_generation::{AiPatchDefinition, MaskDefinition, generate_mask_bitmap};
use crate::mods::touch_up;
use crate::resolve_warped_image_for_masks;

fn prepare_source_image(
    patch_id: &str,
    current_adjustments: &Value,
    state: &tauri::State<'_, AppState>,
) -> Result<(DynamicImage, bool), String> {
    let mut source_image_adjustments = current_adjustments.clone();
    if let Some(patches) = source_image_adjustments
        .get_mut("aiPatches")
        .and_then(|v| v.as_array_mut())
    {
        patches.retain(|p| p.get("id").and_then(|id| id.as_str()) != Some(patch_id));
    }

    let is_raw = {
        let guard = state.original_image.lock().unwrap();
        guard.as_ref().map(|img| img.is_raw).unwrap_or(false)
    };

    let (base_image, _) = crate::get_original_image(state)?;
    let composited = composite_patches_on_image(&base_image, &source_image_adjustments)
        .map_err(|e| format!("Failed to prepare source image: {}", e))?;

    let source_image = if is_raw {
        apply_linear_to_srgb(composited)
    } else {
        composited
    };

    Ok((source_image, is_raw))
}

/// The size of the photo as shown (strokes are recorded on it): the raw size,
/// turned by quarter turns.
// RapidRAW f3c68838: src-tauri/src/inpainting.rs (`transformed_dimensions`)
fn transformed_dimensions(img_w: u32, img_h: u32, adjustments: &Value) -> (u32, u32) {
    let orientation_steps = adjustments
        .get("orientationSteps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if orientation_steps % 2 == 1 {
        (img_h, img_w)
    } else {
        (img_w, img_h)
    }
}

/// The patch's brushed mask, drawn on the photo as shown and mapped back to
/// raw pixels through its geometry.
// RapidRAW f3c68838: src-tauri/src/inpainting.rs (`raw_patch_mask`)
fn raw_patch_mask(
    patch_definition: &AiPatchDefinition,
    current_adjustments: &Value,
    state: &tauri::State<'_, AppState>,
    (trans_w, trans_h): (u32, u32),
) -> Result<image::GrayImage, String> {
    let mask_def = MaskDefinition {
        id: patch_definition.id.clone(),
        name: patch_definition.name.clone(),
        visible: patch_definition.visible,
        invert: patch_definition.invert,
        opacity: 100.0,
        adjustments: Value::Null,
        sub_masks: patch_definition.sub_masks.clone(),
    };

    let warped_image =
        resolve_warped_image_for_masks(state, current_adjustments, std::slice::from_ref(&mask_def));

    let mask_bitmap = generate_mask_bitmap(
        &mask_def,
        trans_w,
        trans_h,
        1.0,
        (0.0, 0.0),
        warped_image.as_deref(),
    )
    .ok_or("Failed to generate mask bitmap for this patch")?;

    Ok(crate::image_processing::inverse_transform_mask(
        mask_bitmap,
        current_adjustments,
    ))
}

fn calculate_mask_bounds(
    mask_bitmap: &image::GrayImage,
) -> Result<(usize, usize, usize, usize), String> {
    let (img_w, img_h) = mask_bitmap.dimensions();
    let mask_raw = mask_bitmap.as_raw();
    let img_w_usize = img_w as usize;
    let img_h_usize = img_h as usize;

    let mut min_y = img_h_usize;
    let mut max_y = 0;

    for y in 0..img_h_usize {
        let row_start = y * img_w_usize;
        if mask_raw[row_start..row_start + img_w_usize]
            .iter()
            .any(|&p| p > 0)
        {
            min_y = y;
            break;
        }
    }

    if min_y == img_h_usize {
        return Err("Mask is empty.".to_string());
    }

    for y in (min_y..img_h_usize).rev() {
        let row_start = y * img_w_usize;
        if mask_raw[row_start..row_start + img_w_usize]
            .iter()
            .any(|&p| p > 0)
        {
            max_y = y;
            break;
        }
    }

    let mut min_x = img_w_usize;
    let mut max_x = 0;
    for y in min_y..=max_y {
        let row_start = y * img_w_usize;
        let row = &mask_raw[row_start..row_start + img_w_usize];
        if let Some(first) = row.iter().position(|&p| p > 0)
            && first < min_x
        {
            min_x = first;
        }
        if let Some(last) = row.iter().rposition(|&p| p > 0)
            && last > max_x
        {
            max_x = last;
        }
    }

    Ok((min_x, max_x, min_y, max_y))
}

#[allow(clippy::too_many_arguments)]
fn encode_patch_result(
    color_image: &RgbImage,
    mask_image: &image::GrayImage,
    offset_x: u32,
    offset_y: u32,
    width: u32,
    height: u32,
    is_srgb: bool,
    quality: u8,
    mask_as_png: bool,
) -> Result<String, String> {
    let mut color_buf = Cursor::new(Vec::with_capacity(32768));
    color_image
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut color_buf,
            quality,
        ))
        .map_err(|e| e.to_string())?;
    let color_base64 = general_purpose::STANDARD.encode(color_buf.get_ref());

    let mut mask_buf = Cursor::new(Vec::with_capacity(32768));
    if mask_as_png {
        mask_image
            .write_to(&mut mask_buf, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
    } else {
        mask_image
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut mask_buf,
                quality,
            ))
            .map_err(|e| e.to_string())?;
    }
    let mask_base64 = general_purpose::STANDARD.encode(mask_buf.get_ref());

    let result_json = serde_json::json!({
        "color": color_base64,
        "mask": mask_base64,
        "offsetX": offset_x,
        "offsetY": offset_y,
        "width": width,
        "height": height,
        "isSrgbEncoded": is_srgb
    })
    .to_string();

    Ok(result_json)
}

#[tauri::command]
pub async fn generate_manual_cleanup_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    source_point: (f64, f64),
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (source_image, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let (img_w, img_h) = source_image.dimensions();

    let (trans_w, trans_h) = transformed_dimensions(img_w, img_h, &current_adjustments);
    let mask_bitmap = raw_patch_mask(
        &patch_definition,
        &current_adjustments,
        &state,
        (trans_w, trans_h),
    )?;

    let (min_x, max_x, min_y, max_y) = calculate_mask_bounds(&mask_bitmap)?;

    let center_x = (min_x + max_x) as f64 / 2.0;
    let center_y = (min_y + max_y) as f64 / 2.0;

    let source_point_untransformed = crate::image_processing::inverse_transform_point(
        source_point.0,
        source_point.1,
        trans_w as f64,
        trans_h as f64,
        &current_adjustments,
    );

    let offset_x = (source_point_untransformed.0 - center_x).round() as i32;
    let offset_y = (source_point_untransformed.1 - center_y).round() as i32;

    let min_x_u32 = min_x as u32;
    let min_y_u32 = min_y as u32;
    let crop_w = (max_x - min_x + 1) as u32;
    let crop_h = (max_y - min_y + 1) as u32;

    let sub_masks_val = serde_json::to_value(&patch_definition.sub_masks).unwrap_or(Value::Null);
    let mut is_heal = false;
    if let Some(arr) = sub_masks_val.as_array() {
        for sm in arr {
            if let Some(t) = sm.get("type").and_then(|v| v.as_str())
                && t.eq_ignore_ascii_case("heal")
            {
                is_heal = true;
                break;
            }
        }
    }
    if !is_heal && patch_definition.name.to_lowercase().contains("heal") {
        is_heal = true;
    }

    let mut color_image = RgbImage::new(crop_w, crop_h);

    if !is_heal {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px_x = x as u32;
                let px_y = y as u32;
                if mask_bitmap.get_pixel(px_x, px_y)[0] > 0 {
                    let src_x = (px_x as i32 + offset_x).clamp(0, img_w as i32 - 1) as u32;
                    let src_y = (px_y as i32 + offset_y).clamp(0, img_h as i32 - 1) as u32;
                    let src_px = source_image.get_pixel(src_x, src_y);

                    let dest_x = px_x - min_x_u32;
                    let dest_y = px_y - min_y_u32;
                    color_image.put_pixel(dest_x, dest_y, Rgb([src_px[0], src_px[1], src_px[2]]));
                }
            }
        }
    } else {
        // RapidRAW 19427777: src-tauri/src/clone_heal.rs (`heal_patch`, multigrid to a tolerance)
        color_image = crate::mods::clone_heal::heal_patch(
            &source_image,
            &mask_bitmap,
            (min_x, max_x, min_y, max_y),
            (offset_x, offset_y),
        );
    }

    let output_mask =
        image::imageops::crop_imm(&mask_bitmap, min_x_u32, min_y_u32, crop_w, crop_h).to_image();

    encode_patch_result(
        &color_image,
        &output_mask,
        min_x_u32,
        min_y_u32,
        crop_w,
        crop_h,
        is_raw,
        100,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn invoke_generative_replace_with_mask_def(
    path: String,
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    use_fast_inpaint: bool,
    task_id: Option<String>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let _guard = task_id
        .as_ref()
        .map(|id| crate::app_state::AiTaskGuard::new(&state.active_ai_tasks, id.clone()));
    let cancel_flag = _guard.as_ref().map(|g| &g.token);

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let (source_image, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let (img_w, img_h) = source_image.dimensions();

    let orientation_steps = current_adjustments
        .get("orientationSteps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;
    let (trans_w, trans_h) = if orientation_steps % 2 == 1 {
        (img_h, img_w)
    } else {
        (img_w, img_h)
    };

    let mask_def_for_generation = MaskDefinition {
        id: patch_definition.id.clone(),
        name: patch_definition.name.clone(),
        visible: patch_definition.visible,
        invert: patch_definition.invert,
        opacity: 100.0,
        adjustments: serde_json::Value::Null,
        sub_masks: patch_definition.sub_masks.clone(),
    };

    let warped_image = resolve_warped_image_for_masks(
        &state,
        &current_adjustments,
        std::slice::from_ref(&mask_def_for_generation),
    );

    let mask_bitmap = generate_mask_bitmap(
        &mask_def_for_generation,
        trans_w,
        trans_h,
        1.0,
        (0.0, 0.0),
        warped_image.as_deref(),
    )
    .ok_or("Failed to generate mask bitmap for AI replace")?;

    let mask_bitmap =
        crate::image_processing::inverse_transform_mask(mask_bitmap, &current_adjustments);

    let (min_x, max_x, min_y, max_y) = calculate_mask_bounds(&mask_bitmap)?;

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let patch_rgba = if use_fast_inpaint {
        let lama_model = ai_processing::get_or_init_lama_model(
            &app_handle,
            &state.ai_state,
            &state.ai_init_lock,
        )
        .await
        .map_err(|e| e.to_string())?;

        if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
            return Err("Task cancelled".to_string());
        }

        ai_processing::run_lama_inpainting(&source_image, &mask_bitmap, &lama_model)
            .map_err(|e| e.to_string())?
    } else if settings.ai_provider.as_deref() == Some("ai-connector")
        && let Some(address) = settings.ai_connector_address
    {
        let base_url = format!("http://{}", address);

        let mut rgba_mask = RgbaImage::new(img_w, img_h);
        for (src_val, dst_chunk) in mask_bitmap.as_raw().iter().zip(rgba_mask.chunks_mut(4)) {
            let intensity = *src_val;
            dst_chunk[0] = intensity;
            dst_chunk[1] = intensity;
            dst_chunk[2] = intensity;
            dst_chunk[3] = 255;
        }
        let mask_image_dynamic = DynamicImage::ImageRgba8(rgba_mask);

        let (real_path_buf, _) = crate::file_management::parse_virtual_path(&path);
        let real_path_str = real_path_buf.to_string_lossy().to_string();

        let inpaint_fut = ai_connector::process_inpainting(
            &base_url,
            &real_path_str,
            &source_image,
            &mask_image_dynamic,
            patch_definition.prompt,
            None,
        );

        if let Some(token) = cancel_flag.as_ref() {
            tokio::select! {
                res = inpaint_fut => res.map_err(|e| e.to_string())?,
                _ = token.wait_for_cancel() => {
                    return Err("Task cancelled".to_string());
                }
            }
        } else {
            inpaint_fut.await.map_err(|e| e.to_string())?
        }
    } else {
        return Err(
            "No generative backend configured or connection invalid. Please check your AI settings."
                .to_string(),
        );
    };

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let (patch_w, patch_h) = patch_rgba.dimensions();
    let final_patch = if patch_w != img_w || patch_h != img_h {
        image::imageops::resize(
            &patch_rgba,
            img_w,
            img_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        patch_rgba.clone()
    };

    let min_x_u32 = min_x as u32;
    let min_y_u32 = min_y as u32;
    let crop_w = (max_x - min_x + 1) as u32;
    let crop_h = (max_y - min_y + 1) as u32;

    let mut color_image = RgbImage::new(crop_w, crop_h);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px_x = x as u32;
            let px_y = y as u32;
            let mask_value = mask_bitmap.get_pixel(px_x, px_y)[0];

            let out_x = px_x - min_x_u32;
            let out_y = px_y - min_y_u32;

            let px = if mask_value > 0 {
                *final_patch.get_pixel(px_x, px_y)
            } else {
                source_image.get_pixel(px_x, px_y)
            };
            color_image.put_pixel(out_x, out_y, Rgb([px[0], px[1], px[2]]));
        }
    }

    let output_mask =
        image::imageops::crop_imm(&mask_bitmap, min_x_u32, min_y_u32, crop_w, crop_h).to_image();

    encode_patch_result(
        &color_image,
        &output_mask,
        min_x_u32,
        min_y_u32,
        crop_w,
        crop_h,
        is_raw,
        95,
        true,
    )
}

#[tauri::command]
pub async fn generate_liquify_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    _source_point: (f64, f64),
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (source_dynamic, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let (img_w, img_h) = source_dynamic.dimensions();
    let source_image = source_dynamic.to_rgb8();
    let (trans_w, trans_h) = transformed_dimensions(img_w, img_h, &current_adjustments);

    // RapidRAW f3c68838: src-tauri/src/inpainting.rs (`generate_liquify_patch`)
    let strokes = touch_up::liquify_strokes(&patch_definition.sub_masks, |x, y| {
        crate::image_processing::inverse_transform_point(
            x,
            y,
            trans_w as f64,
            trans_h as f64,
            &current_adjustments,
        )
    });
    let (color_image, mask_image, (offset_x, offset_y)) =
        touch_up::liquify_patch(&source_image, &strokes)
            .ok_or("No brush strokes found for Liquify.")?;

    encode_patch_result(
        &color_image,
        &mask_image,
        offset_x,
        offset_y,
        color_image.width(),
        color_image.height(),
        is_raw,
        100,
        false,
    )
}

#[tauri::command]
pub async fn generate_retouch_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (source_dynamic, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let (img_w, img_h) = source_dynamic.dimensions();
    let source_image = source_dynamic.to_rgb8();

    // RapidRAW f3c68838: src-tauri/src/inpainting.rs (`generate_retouch_patch`)
    let retouch = patch_definition
        .sub_masks
        .iter()
        .find(|sm| sm.mask_type.eq_ignore_ascii_case("retouch"))
        .ok_or("No brush strokes found for Retouch.")?;
    let intensity = retouch
        .parameters
        .get("intensity")
        .and_then(|v| v.as_f64())
        .unwrap_or(50.0) as f32;
    let brush_radius = retouch
        .parameters
        .get("lines")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter(|line| !touch_up::is_eraser_line(line))
        .map(touch_up::line_radius)
        .fold(0.0_f32, f32::max);

    let mask_bitmap = raw_patch_mask(
        &patch_definition,
        &current_adjustments,
        &state,
        transformed_dimensions(img_w, img_h, &current_adjustments),
    )?;
    let bounds = calculate_mask_bounds(&mask_bitmap)
        .map_err(|_| "No brush strokes found for Retouch.".to_string())?;
    let (min_x, max_x, min_y, max_y) = bounds;

    let color_image =
        touch_up::retouch_patch(&source_image, bounds, brush_radius, intensity / 100.0);

    let crop_w = (max_x - min_x + 1) as u32;
    let crop_h = (max_y - min_y + 1) as u32;
    let output_mask =
        image::imageops::crop_imm(&mask_bitmap, min_x as u32, min_y as u32, crop_w, crop_h)
            .to_image();

    encode_patch_result(
        &color_image,
        &output_mask,
        min_x as u32,
        min_y as u32,
        crop_w,
        crop_h,
        is_raw,
        100,
        false,
    )
}
