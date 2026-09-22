//! Local photo enlargement with Real-ESRGAN.
//!
//! The model is downloaded lazily into the same per-user model directory used
//! by the other AI features. Inference is tiled so normal camera files do not
//! require the whole image to fit in the ONNX Runtime working set at once.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgb, Rgb32FImage};
use ndarray::{Array4, Ix4};
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sysinfo::System;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as TokioMutex;

const TILE_SIZES: [u32; 7] = [512, 384, 256, 192, 128, 96, 64];
const MIN_TILE_OVERLAP: u32 = 8;
const TILE_OVERLAP_NUMERATOR: u32 = 3;
const TILE_OVERLAP_DENOMINATOR: u32 = 16;
const MEMORY_HEADROOM_NUMERATOR: u64 = 7;
const MEMORY_HEADROOM_DENOMINATOR: u64 = 10;

#[derive(Clone, Copy)]
enum ModelKind {
    X2,
    X4,
}

impl ModelKind {
    fn from_scale(scale: u32) -> Result<Self> {
        match scale {
            2 => Ok(Self::X2),
            4 => Ok(Self::X4),
            _ => Err(anyhow!("Super resolution only supports 2x and 4x")),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::X2 => "2x",
            Self::X4 => "4x",
        }
    }
}

struct ModelSpec {
    url: &'static str,
    filename: &'static str,
    sha256: &'static str,
}

fn model_spec(kind: ModelKind) -> ModelSpec {
    match kind {
        ModelKind::X2 => ModelSpec {
            url: "https://huggingface.co/fernandotonon/QtMeshEditor-realesrgan-onnx/resolve/main/RealESRGAN_x2plus.onnx?download=true",
            filename: "realesrgan_x2plus.onnx",
            // SHA-256 of the published Real-ESRGAN x2plus ONNX artifact.
            sha256: "35d016524a6eb7e9a99f192cfa81b6cf1f80fdf8f1b51605d77f0763f5ccba7f",
        },
        ModelKind::X4 => ModelSpec {
            url: "https://huggingface.co/mhmtaufiq/realesrgan-onnx/resolve/main/RealESRGAN_x4plus.onnx?download=true",
            filename: "realesrgan_x4plus.onnx",
            // SHA-256 of the published Real-ESRGAN x4plus ONNX artifact.
            sha256: "3767c17388381cfca3d7196a4a517737fefc22b57c9fd98d3bae78e98e2bebc9",
        },
    }
}

static MODEL_X2: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
static MODEL_X4: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
static MODEL_INIT_X2: OnceLock<TokioMutex<()>> = OnceLock::new();
static MODEL_INIT_X4: OnceLock<TokioMutex<()>> = OnceLock::new();
static RESULT: OnceLock<Mutex<Option<DynamicImage>>> = OnceLock::new();

#[derive(Serialize)]
pub struct PreviewPayload {
    pub result: String,
    pub original: String,
}

#[derive(Clone, Serialize)]
struct ProgressPayload {
    completed: usize,
    total: usize,
    message: String,
}

fn result_slot() -> &'static Mutex<Option<DynamicImage>> {
    RESULT.get_or_init(|| Mutex::new(None))
}

fn models_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app_handle.path().app_data_dir()?.join("models");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn verify_sha256(path: &Path, expected: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(hex::encode(hasher.finalize()) == expected)
}

fn model_slot(kind: ModelKind) -> &'static OnceLock<Arc<Mutex<Session>>> {
    match kind {
        ModelKind::X2 => &MODEL_X2,
        ModelKind::X4 => &MODEL_X4,
    }
}

fn model_init(kind: ModelKind) -> &'static TokioMutex<()> {
    match kind {
        ModelKind::X2 => MODEL_INIT_X2.get_or_init(|| TokioMutex::new(())),
        ModelKind::X4 => MODEL_INIT_X4.get_or_init(|| TokioMutex::new(())),
    }
}

async fn ensure_model(app_handle: &tauri::AppHandle, kind: ModelKind) -> Result<PathBuf> {
    let spec = model_spec(kind);
    let dir = models_dir(app_handle)?;
    let path = dir.join(spec.filename);

    if !verify_sha256(&path, spec.sha256)? {
        if path.exists() {
            fs::remove_file(&path)?;
        }

        let _ = app_handle.emit(
            "ai-model-download-start",
            format!("Real-ESRGAN {} Super Resolution Model", kind.label()),
        );
        let download = async {
            let bytes = reqwest::get(spec.url)
                .await
                .with_context(|| {
                    format!(
                        "Could not download the {} model; check your internet connection",
                        kind.label()
                    )
                })?
                .error_for_status()
                .with_context(|| format!("The {} model server returned an error", kind.label()))?
                .bytes()
                .await
                .with_context(|| format!("Could not read the downloaded {} model", kind.label()))?;
            if bytes.is_empty() {
                return Err(anyhow!("Super-resolution model download was empty"));
            }

            let temporary = path.with_extension("onnx.download");
            {
                let mut file = fs::File::create(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
            }
            fs::rename(&temporary, &path).or_else(|rename_error| -> std::io::Result<()> {
                if path.exists() {
                    fs::remove_file(&path)?;
                    fs::rename(&temporary, &path)?;
                    Ok(())
                } else {
                    Err(rename_error)
                }
            })?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        let _ = app_handle.emit(
            "ai-model-download-finish",
            format!("Real-ESRGAN {} Super Resolution Model", kind.label()),
        );
        download?;

        if !verify_sha256(&path, spec.sha256)? {
            return Err(anyhow!(
                "Real-ESRGAN {} model failed integrity verification",
                kind.label()
            ));
        }
    }

    Ok(path)
}

async fn get_model(app_handle: &tauri::AppHandle, kind: ModelKind) -> Result<Arc<Mutex<Session>>> {
    let slot = model_slot(kind);
    if let Some(model) = slot.get() {
        return Ok(model.clone());
    }

    let guard = model_init(kind).lock().await;
    if let Some(model) = slot.get() {
        drop(guard);
        return Ok(model.clone());
    }

    let path = ensure_model(app_handle, kind).await?;
    let _ = ort::init().with_name("AI-Super-Resolution").commit();
    let session = Session::builder()
        .with_context(|| format!("Could not initialize the {} model runtime", kind.label()))?
        .commit_from_file(&path)
        .with_context(|| format!("Could not load the {} model file", kind.label()))?;
    let model = Arc::new(Mutex::new(session));
    let _ = slot.set(model.clone());
    drop(guard);
    Ok(model)
}

fn tile_overlap(tile_size: u32) -> u32 {
    (tile_size * TILE_OVERLAP_NUMERATOR / TILE_OVERLAP_DENOMINATOR)
        .max(MIN_TILE_OVERLAP)
        .min(tile_size.saturating_sub(1))
}

fn tile_positions(length: u32, tile_size: u32, overlap: u32) -> Vec<u32> {
    if length <= tile_size {
        return vec![0];
    }

    let last = length - tile_size;
    let step = tile_size - overlap;
    let mut positions = vec![0];
    let mut position = 0;
    while position < last {
        position = (position + step).min(last);
        if positions.last().copied() != Some(position) {
            positions.push(position);
        }
    }
    positions
}

fn padded_tile(image: &Rgb32FImage, x0: u32, y0: u32, tile_size: u32) -> Array4<f32> {
    let (width, height) = image.dimensions();
    let mut array = Array4::<f32>::zeros((1, 3, tile_size as usize, tile_size as usize));

    for y in 0..tile_size {
        for x in 0..tile_size {
            let source_x = (x0 + x).min(width.saturating_sub(1));
            let source_y = (y0 + y).min(height.saturating_sub(1));
            let pixel = image.get_pixel(source_x, source_y);
            array[[0, 0, y as usize, x as usize]] = pixel[0].clamp(0.0, 1.0);
            array[[0, 1, y as usize, x as usize]] = pixel[1].clamp(0.0, 1.0);
            array[[0, 2, y as usize, x as usize]] = pixel[2].clamp(0.0, 1.0);
        }
    }

    array
}

fn edge_weight(index: u32, length: u32, at_start: bool, at_end: bool, overlap: u32) -> f32 {
    let distance_from_edge = index.min(length.saturating_sub(1).saturating_sub(index));
    let ramp = if at_start && at_end {
        1.0
    } else if at_start {
        ((length.saturating_sub(1).saturating_sub(index) + 1) as f32 / (overlap + 1) as f32)
            .min(1.0)
    } else if at_end {
        ((index + 1) as f32 / (overlap + 1) as f32).min(1.0)
    } else {
        ((distance_from_edge + 1) as f32 / (overlap + 1) as f32).min(1.0)
    };
    ramp.max(0.05)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn adaptive_tile_size(scale: u32, output_pixels: u64) -> Result<(u32, u64)> {
    let bytes_per_output_pixel = (size_of::<f32>() as u64 * 4) + 3;
    let output_memory = output_pixels
        .checked_mul(bytes_per_output_pixel)
        .context("The requested image is too large to address")?;
    let mut system = System::new();
    system.refresh_memory();
    let available_memory = system.available_memory();

    if available_memory > 0
        && output_memory
            > available_memory * MEMORY_HEADROOM_NUMERATOR / MEMORY_HEADROOM_DENOMINATOR
    {
        return Err(anyhow!(
            "This result needs about {} for its output buffers, but only {} RAM is currently available. Reduce the source dimensions or use a smaller scale.",
            format_bytes(output_memory),
            format_bytes(available_memory)
        ));
    }

    let working_memory = if available_memory > 0 {
        available_memory
            .saturating_sub(output_memory)
            .saturating_mul(6)
            / 10
    } else {
        128 * 1024 * 1024
    };

    for tile_size in TILE_SIZES {
        let input_pixels = u64::from(tile_size) * u64::from(tile_size);
        let output_tile_pixels = input_pixels * u64::from(scale) * u64::from(scale);
        let input_bytes = input_pixels * 3 * size_of::<f32>() as u64;
        let output_bytes = output_tile_pixels * 3 * size_of::<f32>() as u64;
        // Leave room for ONNX Runtime's intermediate tensors and allocator overhead.
        let estimated_tile_memory = (input_bytes + output_bytes).saturating_mul(2);
        if estimated_tile_memory <= working_memory {
            return Ok((tile_size, output_memory));
        }
    }

    Err(anyhow!(
        "There is not enough available RAM for super resolution at {}x. Close other applications or use a smaller image.",
        scale
    ))
}

fn run_model(
    image: &Rgb32FImage,
    model: &Mutex<Session>,
    app_handle: &tauri::AppHandle,
    scale: u32,
) -> Result<Rgb32FImage> {
    let (width, height) = image.dimensions();
    let output_width = width.checked_mul(scale).context("Output width overflow")?;
    let output_height = height
        .checked_mul(scale)
        .context("Output height overflow")?;
    let output_pixels = u64::from(output_width) * u64::from(output_height);
    let (tile_size, output_memory) = adaptive_tile_size(scale, output_pixels)?;
    let overlap = tile_overlap(tile_size);

    let positions_x = tile_positions(width, tile_size, overlap);
    let positions_y = tile_positions(height, tile_size, overlap);
    let total_tiles = positions_x.len() * positions_y.len();
    let accumulator_len = output_pixels
        .checked_mul(3)
        .and_then(|length| usize::try_from(length).ok())
        .context("The result is too large for this system")?;
    let weights_len =
        usize::try_from(output_pixels).context("The result is too large for this system")?;
    let mut accumulator = Vec::new();
    accumulator
        .try_reserve_exact(accumulator_len)
        .map_err(|_| {
            anyhow!(
                "Could not allocate {} for super-resolution output",
                format_bytes(output_memory)
            )
        })?;
    accumulator.resize(accumulator_len, 0.0f32);
    let mut weights = Vec::new();
    weights
        .try_reserve_exact(weights_len)
        .map_err(|_| anyhow!("Could not allocate memory for super-resolution seams"))?;
    weights.resize(weights_len, 0.0f32);

    for (tile_index, y0) in positions_y.iter().enumerate() {
        for (x_index, x0) in positions_x.iter().enumerate() {
            let tile_number = tile_index * positions_x.len() + x_index;
            let _ = app_handle.emit(
                "super-resolution-progress",
                ProgressPayload {
                    completed: tile_number + 1,
                    total: total_tiles,
                    message: format!(
                        "Upscaling tile {}/{} using {}px tiles...",
                        tile_number + 1,
                        total_tiles,
                        tile_size
                    ),
                },
            );

            let input = padded_tile(image, *x0, *y0, tile_size);
            let output = {
                let mut session = model
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let outputs = session.run(ort::inputs![Tensor::from_array(input)?])?;
                outputs[0]
                    .try_extract_array::<f32>()?
                    .to_owned()
                    .into_dimensionality::<Ix4>()
                    .map_err(|error| anyhow!("Unexpected super-resolution output shape: {error}"))?
            };

            let tile_output_width = output.shape()[3] as u32;
            let tile_output_height = output.shape()[2] as u32;
            if tile_output_width != tile_size * scale || tile_output_height != tile_size * scale {
                return Err(anyhow!(
                    "Real-ESRGAN {}x model returned {}x{} for a {}x{} tile",
                    scale,
                    tile_output_width,
                    tile_output_height,
                    tile_size,
                    tile_size
                ));
            }

            let at_start_x = *x0 == 0;
            let at_end_x = *x0 + tile_size >= width;
            let at_start_y = *y0 == 0;
            let at_end_y = *y0 + tile_size >= height;
            for local_y in 0..tile_output_height {
                let global_y = y0.saturating_mul(scale) + local_y;
                if global_y >= output_height {
                    continue;
                }
                let input_y = local_y / scale;
                let y_weight = edge_weight(input_y, tile_size, at_start_y, at_end_y, overlap);
                for local_x in 0..tile_output_width {
                    let global_x = x0.saturating_mul(scale) + local_x;
                    if global_x >= output_width {
                        continue;
                    }
                    let input_x = local_x / scale;
                    let weight =
                        y_weight * edge_weight(input_x, tile_size, at_start_x, at_end_x, overlap);
                    let output_index =
                        (global_y as usize * output_width as usize) + global_x as usize;
                    let base = output_index * 3;
                    accumulator[base] +=
                        output[[0, 0, local_y as usize, local_x as usize]] * weight;
                    accumulator[base + 1] +=
                        output[[0, 1, local_y as usize, local_x as usize]] * weight;
                    accumulator[base + 2] +=
                        output[[0, 2, local_y as usize, local_x as usize]] * weight;
                    weights[output_index] += weight;
                }
            }
        }
    }

    let mut result = Rgb32FImage::new(output_width, output_height);
    for (index, pixel) in result.pixels_mut().enumerate() {
        let base = index * 3;
        let divisor = weights[index].max(0.0001);
        *pixel = Rgb([
            (accumulator[base] / divisor).clamp(0.0, 1.0),
            (accumulator[base + 1] / divisor).clamp(0.0, 1.0),
            (accumulator[base + 2] / divisor).clamp(0.0, 1.0),
        ]);
    }
    Ok(result)
}

fn encode_preview(image: &DynamicImage) -> Result<String> {
    let preview = if image.width() > 2400 || image.height() > 2400 {
        image.resize(2400, 2400, FilterType::Lanczos3)
    } else {
        image.clone()
    }
    .to_rgb8();
    let mut bytes = Cursor::new(Vec::new());
    preview.write_to(&mut bytes, ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(bytes.get_ref())
    ))
}

fn load_source(path: &str, app_handle: &tauri::AppHandle) -> Result<DynamicImage> {
    let (source_path, _) = crate::file_management::parse_virtual_path(path);
    let bytes = fs::read(&source_path)
        .with_context(|| format!("Could not read {}", source_path.display()))?;
    let settings = crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default();
    let mut image = crate::image_loader::load_base_image_from_bytes(
        &bytes,
        &source_path.to_string_lossy(),
        false,
        &settings,
        None,
    )
    .map_err(|error| {
        anyhow!(
            "Cannot decode '{}'. This file format or pixel data is not supported for super resolution: {}",
            source_path.display(),
            error
        )
    })?;
    if crate::formats::is_raw_file(&source_path) {
        crate::image_processing::apply_cpu_default_raw_processing(&mut image);
    }
    Ok(image)
}

async fn upscale(
    path: String,
    app_handle: tauri::AppHandle,
    model: Arc<Mutex<Session>>,
    scale: u32,
) -> Result<PreviewPayload> {
    let result = tokio::task::spawn_blocking(move || {
        let source = load_source(&path, &app_handle)?;
        let original_preview = encode_preview(&source).with_context(|| {
            format!(
                "Cannot create a preview for '{}'. The decoded image format is not supported",
                path
            )
        })?;
        let result = DynamicImage::ImageRgb32F(
            run_model(&source.to_rgb32f(), &model, &app_handle, scale).with_context(|| {
                format!(
                    "Cannot upscale '{}': the model could not process this image",
                    path
                )
            })?,
        );
        let result_preview = encode_preview(&result)?;
        *result_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        Ok::<PreviewPayload, anyhow::Error>(PreviewPayload {
            result: result_preview,
            original: original_preview,
        })
    })
    .await
    .map_err(|error| anyhow!("Super-resolution task failed: {error}"))??;
    Ok(result)
}

pub async fn preview(
    path: String,
    scale: u32,
    app_handle: tauri::AppHandle,
) -> Result<PreviewPayload, String> {
    let kind = ModelKind::from_scale(scale).map_err(|error| error.to_string())?;
    let model = get_model(&app_handle, kind).await.map_err(|error| {
        format!(
            "Cannot prepare {} super resolution: {}",
            kind.label(),
            error
        )
    })?;
    upscale(path, app_handle, model, scale)
        .await
        .map_err(|error| error.to_string())
}

pub async fn save(original_path: String) -> Result<String, String> {
    let image = result_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .ok_or_else(|| "No super-resolution result found in memory".to_string())?;
    save_image(&original_path, image).map_err(|error| error.to_string())
}

fn save_image(original_path: &str, image: DynamicImage) -> Result<String> {
    let (source_path, _) = crate::file_management::parse_virtual_path(original_path);
    let parent = source_path
        .parent()
        .context("Could not determine parent directory")?;
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("upscaled");
    let is_raw = crate::formats::is_raw_file(&source_path);
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (output_extension, format) = if is_raw || extension == "tif" || extension == "tiff" {
        ("tif", ImageFormat::Tiff)
    } else if extension == "jpg" || extension == "jpeg" {
        ("jpg", ImageFormat::Jpeg)
    } else if extension == "webp" {
        ("webp", ImageFormat::WebP)
    } else {
        ("png", ImageFormat::Png)
    };
    let output_path = parent.join(format!("{}_Upscaled.{}", stem, output_extension));
    let save_result = if format == ImageFormat::Tiff {
        image.to_rgb16().save_with_format(&output_path, format)
    } else {
        image.to_rgb8().save_with_format(&output_path, format)
    };
    save_result.with_context(|| {
        format!(
            "Could not save the enlarged image as {}",
            output_path.display()
        )
    })?;
    write_super_resolution_sidecar(&source_path, &output_path)?;
    Ok(output_path.to_string_lossy().to_string())
}

fn strip_non_transferable_adjustments(adjustments: &mut Value) {
    let Some(object) = adjustments.as_object_mut() else {
        return;
    };

    // These values refer to coordinates, masks, or pixels in the source
    // image. They cannot be copied to the new dimensions after enlargement.
    const SOURCE_COORDINATE_KEYS: &[&str] = &[
        "aiPatches",
        "aspectRatio",
        "crop",
        "flipHorizontal",
        "flipVertical",
        "guidedPerspective",
        "masks",
        "orientationSteps",
        "rotation",
        "transformAspect",
        "transformDistortion",
        "transformHorizontal",
        "transformRotate",
        "transformScale",
        "transformVertical",
        "transformXOffset",
        "transformYOffset",
    ];

    for key in SOURCE_COORDINATE_KEYS {
        object.remove(*key);
    }

    // A depth-map blur is also tied to the old image dimensions.
    let lens_blur_keys: Vec<String> = object
        .keys()
        .filter(|key| key.starts_with("lensBlur"))
        .cloned()
        .collect();
    for key in lens_blur_keys {
        object.remove(&key);
    }
}

fn write_super_resolution_sidecar(source_path: &Path, output_path: &Path) -> Result<()> {
    let source_sidecar = crate::exif_processing::get_primary_sidecar_path(source_path);
    if !source_sidecar.exists() {
        crate::exif_processing::write_rrexif_sidecar(&source_path.to_string_lossy(), output_path)
            .map_err(|error| anyhow!(error))?;
        return Ok(());
    }

    let mut metadata = crate::exif_processing::load_sidecar(&source_sidecar);
    strip_non_transferable_adjustments(&mut metadata.adjustments);

    let output_sidecar = crate::exif_processing::get_primary_sidecar_path(output_path);
    let json = serde_json::to_string_pretty(&metadata)
        .context("Could not serialize the filtered super-resolution sidecar")?;
    fs::write(&output_sidecar, json).with_context(|| {
        format!(
            "Could not write the filtered sidecar {}",
            output_sidecar.display()
        )
    })?;

    crate::exif_processing::write_rrexif_sidecar(&source_path.to_string_lossy(), output_path)
        .map_err(|error| anyhow!(error))?;
    Ok(())
}

pub async fn batch(
    paths: Vec<String>,
    scale: u32,
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let kind = ModelKind::from_scale(scale).map_err(|error| error.to_string())?;
    let model = get_model(&app_handle, kind).await.map_err(|error| {
        format!(
            "Cannot prepare {} super resolution: {}",
            kind.label(),
            error
        )
    })?;
    tokio::task::spawn_blocking(move || {
        let mut saved = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let _ = app_handle.emit(
                "super-resolution-progress",
                ProgressPayload {
                    completed: index,
                    total: paths.len(),
                    message: format!("Upscaling photo {}/{}...", index + 1, paths.len()),
                },
            );
            let source = load_source(path, &app_handle).map_err(|error| error.to_string())?;
            let result = DynamicImage::ImageRgb32F(
                run_model(&source.to_rgb32f(), &model, &app_handle, scale)
                    .map_err(|error| format!("Cannot upscale '{}': {}", path, error))?,
            );
            saved.push(save_image(path, result).map_err(|error| error.to_string())?);
        }
        Ok(saved)
    })
    .await
    .map_err(|error| format!("Super-resolution batch failed: {error}"))?
}
