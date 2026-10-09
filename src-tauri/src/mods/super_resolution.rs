//! Local photo enlargement with Real-ESRGAN.
//!
//! The model is downloaded lazily into the same per-user model directory used
//! by the other AI features. Inference is tiled so normal camera files do not
//! require the whole image to fit in the ONNX Runtime working set at once.
//!
//! It runs on the graphics card where `gpu_runtime` can provide one (Windows,
//! DirectML) and on the CPU everywhere else, with the same output.
//!
//! The enlargement holds the framed photo and its inpainting; the look travels
//! in its sidecar and is applied when it is opened. For a raw, that means the
//! enlargement must open looking like the raw does, although the editor treats
//! it as an ordinary image: see `encode_for_reopening` and `carry_raw_look`.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use glam::DVec3;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgb, Rgb32FImage};
use ndarray::{Array4, Ix4};
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sysinfo::System;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as TokioMutex;

use super::gpu_runtime::{self, Device};
use crate::white_balance::{self, WhiteBalance};

const TILE_SIZES: [u32; 7] = [512, 384, 256, 192, 128, 96, 64];
const MIN_TILE_OVERLAP: u32 = 8;
// 48px at a 512px tile. Measured against a single whole-image pass on an R6
// Mark III crop: the mean error within the blended seams stayed under one level
// in 255 (0.6) in detail and 0.2 in smooth sky, where 96px gave 0.4 and 0.1.
// Halving it cuts a 32 MP photo from 187 tiles to 150.
const TILE_OVERLAP_NUMERATOR: u32 = 3;
const TILE_OVERLAP_DENOMINATOR: u32 = 32;
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

struct LoadedModel {
    session: Mutex<Session>,
    device: Device,
    path: PathBuf,
}

static MODEL_X2: OnceLock<Arc<LoadedModel>> = OnceLock::new();
static MODEL_X4: OnceLock<Arc<LoadedModel>> = OnceLock::new();
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

/// The bytes held by the last enlargement, for mods::memory's report. Zero if
/// the slot is busy: that thread is never made to wait.
pub fn result_bytes() -> usize {
    RESULT
        .get()
        .and_then(|slot| slot.try_lock().ok())
        .and_then(|g| g.as_ref().map(|img| img.as_bytes().len()))
        .unwrap_or(0)
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

fn model_slot(kind: ModelKind) -> &'static OnceLock<Arc<LoadedModel>> {
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

async fn get_model(app_handle: &tauri::AppHandle, kind: ModelKind) -> Result<Arc<LoadedModel>> {
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
    // Before anything else touches ONNX Runtime: this may choose which build loads.
    let device = gpu_runtime::prepare(app_handle).await;
    let _ = ort::init().with_name("AI-Super-Resolution").commit();
    let (session, device) = gpu_runtime::session(&path, device)
        .with_context(|| format!("Could not load the {} model file", kind.label()))?;
    let model = Arc::new(LoadedModel {
        session: Mutex::new(session),
        device,
        path,
    });
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
    model: &LoadedModel,
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
                        "Upscaling tile {}/{} on the {} using {}px tiles...",
                        tile_number + 1,
                        total_tiles,
                        model.device.label(),
                        tile_size
                    ),
                },
            );

            let input = padded_tile(image, *x0, *y0, tile_size);
            let output = {
                let mut session = model
                    .session
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

/// Runs on the model's device, and once more on the CPU if the GPU fails
/// partway: out of video memory on a large 4x tile, or a driver reset.
fn run_with_fallback(
    image: &Rgb32FImage,
    model: &LoadedModel,
    app_handle: &tauri::AppHandle,
    scale: u32,
) -> Result<Rgb32FImage> {
    match run_model(image, model, app_handle, scale) {
        Err(error) if model.device == Device::Gpu => {
            log::warn!("GPU enlargement failed, retrying on the CPU: {error:#}");
            let (session, device) = gpu_runtime::session(&model.path, Device::Cpu)?;
            let cpu = LoadedModel {
                session: Mutex::new(session),
                device,
                path: model.path.clone(),
            };
            run_model(image, &cpu, app_handle, scale)
        }
        result => result,
    }
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
    let (source_path, sidecar_path) = crate::file_management::parse_virtual_path(path);
    let bytes = fs::read(&source_path)
        .with_context(|| format!("Could not read {}", source_path.display()))?;
    let settings = crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default();
    let image = crate::image_loader::load_base_image_from_bytes(
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
    let adjustments = crate::exif_processing::load_sidecar(&sidecar_path).adjustments;
    // Inpainting is part of the photo, not of its look, and its patches are
    // placed in source coordinates the enlargement no longer has. Composite
    // them first, onto the same decoded image the editor composites them onto.
    let patched = crate::image_loader::composite_patches_on_image(&image, &adjustments)
        .map_err(|error| anyhow!("Could not apply the inpainted areas: {error}"))?;
    // Enlarge the photo as it is framed, not the whole sensor: the crop, with
    // the straightening, rotation, flips, perspective and lens correction it is
    // measured in. Enlarging everything and cropping afterwards spends most of
    // the work on pixels that are thrown away. The export frames a photo with
    // the same call, so the result matches what an export would contain.
    let (framed, _) = crate::adjustment_utils::apply_all_transformations(patched, &adjustments);
    let mut framed = framed.into_owned();
    if crate::formats::is_raw_file(&source_path) {
        encode_for_reopening(&mut framed);
    }
    Ok(framed)
}

/// A raw decodes to scene-linear light, and is framed and enlarged as such. The
/// enlargement is reopened as an ordinary image, which the editor decodes with
/// the sRGB curve (`srgb_to_linear` in shader.wgsl). Encoding with exactly that
/// curve's inverse hands the editor back the linear values the raw started
/// from, so the look carried in the sidecar is applied once, as on the raw.
///
/// This replaced their CPU preview curve, which baked a tone curve into the
/// enlargement that the carried look then applied a second time on top.
/// Light above 1.0 is clipped: a 16-bit TIFF has nowhere to keep it.
fn encode_for_reopening(image: &mut DynamicImage) {
    use rayon::prelude::*;

    let mut rgb = image.to_rgb32f();
    rgb.par_chunks_mut(3).for_each(|pixel| {
        for channel in pixel {
            *channel = srgb_encode(*channel);
        }
    });
    *image = DynamicImage::ImageRgb32F(rgb);
}

fn srgb_encode(linear: f32) -> f32 {
    let c = linear.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

async fn upscale(
    path: String,
    app_handle: tauri::AppHandle,
    model: Arc<LoadedModel>,
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
            run_with_fallback(&source.to_rgb32f(), &model, &app_handle, scale).with_context(
                || {
                    format!(
                        "Cannot upscale '{}': the model could not process this image",
                        path
                    )
                },
            )?,
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

pub async fn save(original_path: String, app_handle: tauri::AppHandle) -> Result<String, String> {
    let image = result_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .ok_or_else(|| "No super-resolution result found in memory".to_string())?;
    save_image(&original_path, image, &app_handle).map_err(|error| error.to_string())
}

fn save_image(
    original_path: &str,
    image: DynamicImage,
    app_handle: &tauri::AppHandle,
) -> Result<String> {
    let (source_path, sidecar_path) = crate::file_management::parse_virtual_path(original_path);
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
    write_super_resolution_sidecar(&source_path, &sidecar_path, &output_path, app_handle)?;
    Ok(output_path.to_string_lossy().to_string())
}

fn strip_non_transferable_adjustments(adjustments: &mut Value) {
    let Some(object) = adjustments.as_object_mut() else {
        return;
    };

    // These values refer to coordinates, masks, or pixels in the source
    // image, or describe framing already applied before enlargement. Copying
    // them would crop, rotate or correct the enlarged photo a second time.
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

    // Lens correction is part of that framing (distortion moves where the
    // crop lands), and the depth-map blur is tied to the old dimensions. Both
    // are already in the pixels: lensBlur*, lensDistortion*, lensTca*,
    // lensVignette*, the lens profile and its mode.
    let lens_keys: Vec<String> = object
        .keys()
        .filter(|key| key.starts_with("lens"))
        .cloned()
        .collect();
    for key in lens_keys {
        object.remove(&key);
    }
}

/// `source_sidecar` is the edited photo's own sidecar, a virtual copy's when
/// one was enlarged, so the edits carried over match the framing applied.
///
/// A raw always gets one, even unedited: its default look is a raw's, and the
/// enlargement would otherwise open with an ordinary image's.
fn write_super_resolution_sidecar(
    source_path: &Path,
    source_sidecar: &Path,
    output_path: &Path,
    app_handle: &tauri::AppHandle,
) -> Result<()> {
    let is_raw = crate::formats::is_raw_file(source_path);
    if !source_sidecar.exists() && !is_raw {
        crate::exif_processing::write_rrexif_sidecar(&source_path.to_string_lossy(), output_path)
            .map_err(|error| anyhow!(error))?;
        return Ok(());
    }

    let mut metadata = crate::exif_processing::load_sidecar(source_sidecar);
    if is_raw {
        let settings = crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default();
        carry_raw_look(
            &mut metadata.adjustments,
            white_balance::as_shot_white_balance(&source_path.to_string_lossy()),
            crate::image_processing::resolve_tonemapper_override(&settings, true),
            crate::image_processing::resolve_tonemapper_override(&settings, false),
        );
    }
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

/// Restate a raw's look for its enlargement, which the editor opens as an
/// ordinary image: same pixels as the raw (`encode_for_reopening`), but a
/// different as-shot white and a different view transform.
///
/// `raw_override` and `non_raw_override` are the tone-mapper override from
/// Settings for each kind of image, which wins over the sidecar's choice.
fn carry_raw_look(
    adjustments: &mut Value,
    as_shot: WhiteBalance,
    raw_override: Option<u32>,
    non_raw_override: Option<u32>,
) {
    if !adjustments.is_object() {
        *adjustments = json!({});
    }
    carry_white_balance(adjustments, as_shot);
    carry_view_transform(adjustments, raw_override, non_raw_override);
}

/// The editor balances a photo by adapting it from its as-shot white to the
/// chosen one. A raw's as-shot white is the camera's, and the enlargement's
/// pixels already carry that balance; but an ordinary image's as-shot white is
/// D65. Copied as it stands, the chosen white adapts the enlargement from D65
/// instead. On 2026-10-09 that turned an R6 Mark III photo balanced to 4076 K
/// plainly blue. So it is restated as the white that adapts D65 by the gains
/// the raw got, with the relative sliders folded in.
fn carry_white_balance(adjustments: &mut Value, as_shot: WhiteBalance) {
    let chosen = white_balance::from_adjustments(adjustments, as_shot);
    let gains = white_balance::adaptation_log_gains(as_shot, chosen);
    let restated = if gains.iter().all(|gain| gain.abs() < 1e-6) {
        None
    } else {
        let restated = restated_from_reference(gains);
        if restated.is_none() {
            log::warn!(
                "Super resolution: could not restate the white balance; the enlargement opens as shot"
            );
        }
        restated
    };
    adjustments["whiteBalance"] = restated
        .and_then(|white| serde_json::to_value(white).ok())
        .unwrap_or(Value::Null);
    adjustments["temperature"] = json!(0);
    adjustments["tint"] = json!(0);
}

/// The white whose adaptation from D65 has these log gains, which are
/// ln(lms(as shot) / lms(chosen)). Picking, as neutral under D65, the colour
/// whose LMS is white's times lms(chosen) / lms(as shot) lands on it.
fn restated_from_reference(gains: [f32; 3]) -> Option<WhiteBalance> {
    let rgb_to_lms = white_balance::rgb_to_lms().as_dmat3();
    let white_lms = rgb_to_lms * DVec3::ONE;
    let sample_lms = white_lms * DVec3::from_array(gains.map(|gain| (-f64::from(gain)).exp()));
    let sample = rgb_to_lms.inverse() * sample_lms;
    white_balance::pick_white_balance(sample.to_array(), WhiteBalance::reference())
}

/// What a raw's view transform does after the sRGB curve that an ordinary
/// image's does not (shader.wgsl, ahead of brightness and the curves).
enum RawViewCurve {
    /// AgX, which both kinds of image get alike.
    Nothing,
    /// The basic tone mapper's raw branch.
    Basic,
    /// Raw tone rendering: the camera base curve, or the auto-matched one.
    Points(Vec<(f32, f32)>),
}

fn raw_view_curve(adjustments: &Value, raw_override: Option<u32>) -> RawViewCurve {
    let mode = adjustments["rawToneRendering"]
        .as_str()
        .unwrap_or("default");
    if mode == "baseCurve" || mode == "autoMatched" {
        let mut points = curve_points(&adjustments["rawToneCurve"]);
        if mode == "baseCurve" && points.len() < 2 {
            points = crate::mods::raw_tone::base_curve()
                .into_iter()
                .map(|point| (point.x, point.y))
                .collect();
        }
        if points.len() >= 2 {
            points.truncate(16);
            return RawViewCurve::Points(points);
        }
    }
    let agx = match raw_override {
        Some(mode) => mode == 1,
        None => adjustments["toneMapper"].as_str() == Some("agx"),
    };
    if agx {
        RawViewCurve::Nothing
    } else {
        RawViewCurve::Basic
    }
}

/// An ordinary image's basic tone mapper is the bare sRGB curve, so the raw's
/// extra step can ride in the luma curve, which runs per channel after it just
/// as that step does. AgX needs nothing. Only brightness sits between the two
/// places, so a global brightness move lands a little differently.
fn carry_view_transform(
    adjustments: &mut Value,
    raw_override: Option<u32>,
    non_raw_override: Option<u32>,
) {
    let view = raw_view_curve(adjustments, raw_override);
    let agx = matches!(view, RawViewCurve::Nothing);
    adjustments["toneMapper"] = json!(if agx { "agx" } else { "basic" });
    adjustments["rawToneRendering"] = json!("default");
    if non_raw_override.is_some_and(|mode| (mode == 1) != agx) {
        log::warn!(
            "Super resolution: the tone-mapper override in Settings renders ordinary images with a different tone mapper, so the enlargement will not match the raw"
        );
    }

    // A scene-referred LUT replaces the view transform's output rather than
    // following it, so folding the raw's step into the curves would apply it
    // to the LUT's result too.
    let has_lut = !adjustments["lutPath"].is_null() || !adjustments["lutData"].is_null();
    if !agx && has_lut && adjustments["lutIsSceneReferred"].as_bool() == Some(true) {
        log::warn!(
            "Super resolution: a scene-referred LUT is in use; the raw's tone curve is not carried over"
        );
        return;
    }
    match view {
        RawViewCurve::Nothing => {}
        RawViewCurve::Basic => fold_into_luma_curve(adjustments, &raw_basic_curve),
        RawViewCurve::Points(points) => {
            fold_into_luma_curve(adjustments, &|s| apply_curve(s, &points))
        }
    }
}

/// The basic tone mapper's raw branch after the sRGB curve (shader.wgsl): a
/// 1.1 gamma lift, then three quarters of a smoothstep.
fn raw_basic_curve(srgb: f32) -> f32 {
    let lifted = srgb.clamp(0.0, 1.0).powf(1.0 / 1.1);
    let s_curve = lifted * lifted * (3.0 - 2.0 * lifted);
    lifted + (s_curve - lifted) * 0.75
}

/// Where the folded luma curve is sampled: closest together in the shadows,
/// where the raw step bends hardest. Sixteen points is the shader's limit.
const FOLD_SAMPLES: [f32; 16] = [
    0.0, 6.0, 16.0, 30.0, 48.0, 68.0, 90.0, 112.0, 134.0, 156.0, 178.0, 200.0, 220.0, 236.0, 248.0,
    255.0,
];

/// Replace the luma curve with `view` followed by it. The red, green and blue
/// curves still follow, as they did. A hidden curves section would hide the
/// raw's step too, so it is shown, with its curves left out as they were.
fn fold_into_luma_curve(adjustments: &mut Value, view: &dyn Fn(f32) -> f32) {
    let curves_shown = adjustments["sectionVisibility"]["curves"].as_bool() != Some(false);
    let luma = if curves_shown {
        curve_points(&adjustments["curves"]["luma"])
    } else {
        Vec::new()
    };
    let folded: Vec<Value> = FOLD_SAMPLES
        .iter()
        .map(|&x| json!({ "x": x, "y": apply_curve(view(x / 255.0), &luma) * 255.0 }))
        .collect();

    if !adjustments["curves"].is_object() || !curves_shown {
        let identity = json!([{ "x": 0, "y": 0 }, { "x": 255, "y": 255 }]);
        adjustments["curves"] = json!({ "red": identity, "green": identity, "blue": identity });
    }
    if !curves_shown {
        adjustments["sectionVisibility"]["curves"] = json!(true);
    }
    adjustments["curves"]["luma"] = Value::Array(folded);
    adjustments["pointCurves"] = adjustments["curves"].clone();
    adjustments["curveMode"] = json!("point");
}

fn curve_points(value: &Value) -> Vec<(f32, f32)> {
    value
        .as_array()
        .map(|points| {
            points
                .iter()
                .filter_map(|point| {
                    Some((point["x"].as_f64()? as f32, point["y"].as_f64()? as f32))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// shader.wgsl's `apply_curve`, step for step: a cubic Hermite through up to
/// sixteen points on a 0-255 scale, with tangents limited to stay monotone.
fn apply_curve(value: f32, points: &[(f32, f32)]) -> f32 {
    let count = points.len().min(16);
    if count < 2 {
        return value;
    }
    let points = &points[..count];
    let x = value * 255.0;
    if x <= points[0].0 {
        return points[0].1 / 255.0;
    }
    if x >= points[count - 1].0 {
        return points[count - 1].1 / 255.0;
    }
    let slope = |a: (f32, f32), b: (f32, f32)| (b.1 - a.1) / (b.0 - a.0).max(0.001);
    for i in 0..count - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        if x > p2.0 {
            continue;
        }
        let p0 = points[i.saturating_sub(1)];
        let p3 = points[(i + 2).min(count - 1)];
        let (before, current, after) = (slope(p0, p1), slope(p1, p2), slope(p2, p3));
        let mut m1 = if i == 0 {
            current
        } else if before * current <= 0.0 {
            0.0
        } else {
            (before + current) / 2.0
        };
        let mut m2 = if i + 1 == count - 1 {
            current
        } else if current * after <= 0.0 {
            0.0
        } else {
            (current + after) / 2.0
        };
        if current != 0.0 {
            let (alpha, beta) = (m1 / current, m2 / current);
            if alpha * alpha + beta * beta > 9.0 {
                let tau = 3.0 / (alpha * alpha + beta * beta).sqrt();
                m1 *= tau;
                m2 *= tau;
            }
        }
        let dx = p2.0 - p1.0;
        if dx <= 0.0 {
            return (p1.1 / 255.0).clamp(0.0, 1.0);
        }
        let t = (x - p1.0) / dx;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p1.1
            + (t3 - 2.0 * t2 + t) * m1 * dx
            + (-2.0 * t3 + 3.0 * t2) * p2.1
            + (t3 - t2) * m2 * dx;
        return (y / 255.0).clamp(0.0, 1.0);
    }
    points[count - 1].1 / 255.0
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
                run_with_fallback(&source.to_rgb32f(), &model, &app_handle, scale)
                    .map_err(|error| format!("Cannot upscale '{}': {}", path, error))?,
            );
            saved.push(save_image(path, result, &app_handle).map_err(|error| error.to_string())?);
        }
        Ok(saved)
    })
    .await
    .map_err(|error| format!("Super-resolution batch failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_overlap_by_48px_at_512_and_never_less_than_the_minimum() {
        assert_eq!(tile_overlap(512), 48);
        assert_eq!(tile_overlap(64), MIN_TILE_OVERLAP);
    }

    #[test]
    fn framing_already_applied_is_not_carried_over_but_the_look_is() {
        let mut adjustments = serde_json::json!({
            "exposure": 0.5,
            "crop": { "x": 10, "y": 20, "width": 300, "height": 200 },
            "rotation": 1.5,
            "orientationSteps": 1,
            "transformVertical": 12,
            "lensDistortionEnabled": true,
            "lensDistortionAmount": 100,
            "lensMaker": "Canon",
            "lensModel": "RF24-105mm F4 L IS USM",
            "lensCorrectionMode": "auto",
            "lensBlurEnabled": true,
            "masks": [],
            "temperature": 12,
        });
        strip_non_transferable_adjustments(&mut adjustments);
        let mut kept: Vec<&str> = adjustments
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        kept.sort_unstable();
        assert_eq!(kept, ["exposure", "temperature"]);
    }

    /// The editor's decode for an ordinary image (`srgb_to_linear`, shader.wgsl).
    fn editor_srgb_to_linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    #[test]
    fn the_editor_decodes_an_enlarged_raw_back_to_its_light() {
        for i in 0..=1000 {
            let linear = i as f32 / 1000.0;
            let decoded = editor_srgb_to_linear(srgb_encode(linear));
            assert!(
                (decoded - linear).abs() < 1e-5,
                "{linear} came back as {decoded}"
            );
        }
        assert!(
            (srgb_encode(3.0) - 1.0).abs() < 1e-6,
            "light above 1.0 clips to white"
        );
    }

    /// What the editor adapts a photo by, up to the overall scale it divides
    /// out (`apply_white_balance` normalises by the adapted white's luma).
    fn white_balance_gains(adjustments: &Value, as_shot: WhiteBalance) -> [f32; 2] {
        let chosen = white_balance::from_adjustments(adjustments, as_shot);
        let [l, m, s] = white_balance::adaptation_log_gains(as_shot, chosen);
        [l - m, s - m]
    }

    #[test]
    fn an_enlarged_raw_is_balanced_by_the_gains_the_raw_was() {
        let as_shot = WhiteBalance {
            temperature: 5600.0,
            tint: 4.0,
        };
        for raw in [
            // The photo it was found on: balanced to 4076 K, enlarged, opened blue.
            json!({ "whiteBalance": { "temperature": 4076.1086, "tint": 7.88477 } }),
            json!({ "whiteBalance": null, "temperature": 12, "tint": -6 }),
            json!({ "whiteBalance": { "temperature": 9000.0, "tint": -20.0 }, "temperature": -5, "tint": 3 }),
        ] {
            let mut enlarged = raw.clone();
            carry_white_balance(&mut enlarged, as_shot);
            let expected = white_balance_gains(&raw, as_shot);
            let got = white_balance_gains(&enlarged, WhiteBalance::reference());
            for (e, g) in expected.iter().zip(got) {
                assert!(
                    (e - g).abs() < 2e-3,
                    "{raw}: raw {expected:?}, enlargement {got:?}"
                );
            }
            assert_eq!(enlarged["temperature"], 0);
            assert_eq!(enlarged["tint"], 0);
        }
    }

    #[test]
    fn a_raw_left_as_shot_opens_enlarged_as_shot() {
        let mut adjustments = json!({ "exposure": 0.3 });
        carry_white_balance(
            &mut adjustments,
            WhiteBalance {
                temperature: 5600.0,
                tint: 4.0,
            },
        );
        assert!(adjustments["whiteBalance"].is_null());
    }

    fn worst_levels(luma: &Value, expected: impl Fn(f32) -> f32) -> f32 {
        let luma = curve_points(luma);
        (0..=255)
            .map(|i| {
                let s = i as f32 / 255.0;
                (apply_curve(s, &luma) - expected(s)).abs() * 255.0
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn the_raw_basic_tone_curve_rides_in_the_luma_curve() {
        let mut adjustments = json!({ "toneMapper": "basic" });
        carry_view_transform(&mut adjustments, None, None);
        assert_eq!(adjustments["toneMapper"], "basic");
        assert_eq!(adjustments["curveMode"], "point");
        assert_eq!(adjustments["pointCurves"], adjustments["curves"]);
        let worst = worst_levels(&adjustments["curves"]["luma"], raw_basic_curve);
        assert!(worst < 1.0, "off by {worst} levels");
    }

    #[test]
    fn a_luma_curve_of_its_own_still_follows_the_raw_tone_curve() {
        let own = vec![(0.0, 12.0), (128.0, 140.0), (255.0, 250.0)];
        let red = json!([{ "x": 0, "y": 0 }, { "x": 128, "y": 120 }, { "x": 255, "y": 255 }]);
        let mut adjustments = json!({
            "curves": {
                "luma": own.iter().map(|(x, y)| json!({ "x": x, "y": y })).collect::<Vec<_>>(),
                "red": red,
            },
        });
        carry_view_transform(&mut adjustments, None, None);
        let worst = worst_levels(&adjustments["curves"]["luma"], |s| {
            apply_curve(raw_basic_curve(s), &own)
        });
        assert!(worst < 1.5, "off by {worst} levels");
        assert_eq!(adjustments["curves"]["red"], red);
    }

    #[test]
    fn the_camera_base_curve_rides_in_the_luma_curve() {
        let mut adjustments = json!({ "rawToneRendering": "baseCurve", "toneMapper": "agx" });
        carry_view_transform(&mut adjustments, None, None);
        assert_eq!(adjustments["toneMapper"], "basic");
        assert_eq!(adjustments["rawToneRendering"], "default");
        let base: Vec<(f32, f32)> = crate::mods::raw_tone::base_curve()
            .into_iter()
            .map(|point| (point.x, point.y))
            .collect();
        let worst = worst_levels(&adjustments["curves"]["luma"], |s| apply_curve(s, &base));
        assert!(worst < 1.5, "off by {worst} levels");
    }

    #[test]
    fn agx_is_the_same_for_both_and_needs_no_curve() {
        let luma = json!([{ "x": 0, "y": 10 }, { "x": 255, "y": 250 }]);
        let mut adjustments = json!({ "toneMapper": "agx", "curves": { "luma": luma } });
        carry_view_transform(&mut adjustments, None, None);
        assert_eq!(adjustments["toneMapper"], "agx");
        assert_eq!(adjustments["curves"]["luma"], luma);

        // The Settings override is what the raw rendered with, not the sidecar.
        let mut overridden = json!({ "toneMapper": "basic", "curves": { "luma": luma } });
        carry_view_transform(&mut overridden, Some(1), None);
        assert_eq!(overridden["toneMapper"], "agx");
        assert_eq!(overridden["curves"]["luma"], luma);
    }

    #[test]
    fn an_unedited_raw_gets_a_raws_default_look() {
        let mut adjustments = Value::Null;
        carry_raw_look(
            &mut adjustments,
            WhiteBalance {
                temperature: 5600.0,
                tint: 4.0,
            },
            None,
            None,
        );
        assert_eq!(adjustments["toneMapper"], "basic");
        assert!(adjustments["whiteBalance"].is_null());
        let worst = worst_levels(&adjustments["curves"]["luma"], raw_basic_curve);
        assert!(worst < 1.0, "off by {worst} levels");
    }

    /// The saving the overlap was halved for: an R6 Mark III frame.
    #[test]
    fn a_32_megapixel_frame_takes_150_tiles() {
        let overlap = tile_overlap(512);
        let tiles =
            tile_positions(6960, 512, overlap).len() * tile_positions(4640, 512, overlap).len();
        assert_eq!(tiles, 150);
    }
}
