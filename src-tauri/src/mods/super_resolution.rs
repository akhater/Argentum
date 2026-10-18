//! Local photo enlargement with Real-ESRGAN, and Restore before it.
//!
//! One window, one file: Restore (OmniLens at the photo's own size, through a
//! detail mask - `restore.rs`), an enlargement, or Restore then an
//! enlargement. The file is named for what was done: `_Restored`,
//! `_Upscaled_x2`, `_Restored_Upscaled_x4`.
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum ModelKind {
    X2,
    X4,
    /// OmniLens, same size: see `restore.rs`.
    Restore,
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
            Self::Restore => "Restore",
        }
    }

    /// What the download notice calls it.
    fn download_name(self) -> String {
        match self {
            Self::Restore => "OmniLens Restore Model".to_string(),
            _ => format!("Real-ESRGAN {} Super Resolution Model", self.label()),
        }
    }

    const fn scale(self) -> u32 {
        match self {
            Self::X2 => 2,
            Self::X4 => 4,
            Self::Restore => 1,
        }
    }

    /// A model exported at one tile size takes only that size. Real-ESRGAN
    /// takes any, so its tile follows the memory there is.
    const fn fixed_tile(self) -> Option<u32> {
        match self {
            Self::Restore => Some(RESTORE_TILE),
            _ => None,
        }
    }
}

/// OmniLens's window attention fixes the shape it was exported at.
const RESTORE_TILE: u32 = 256;

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
        ModelKind::Restore => ModelSpec {
            url: "https://github.com/akhater/argentum-models/releases/download/omnilens-v1/omnilens_swinir_aodlib_256.onnx",
            filename: "omnilens_swinir_aodlib_256.onnx",
            // SHA-256 of Argentum's ONNX export of SwinIR_AODLib_EAOD.pth.
            sha256: "c28ec5478a6db236e9151961ad1afe29ea23b80fecc8612a436f329e9a0e9630",
        },
    }
}

struct LoadedModel {
    session: Mutex<Session>,
    device: Device,
    path: PathBuf,
    kind: ModelKind,
}

static MODEL_X2: OnceLock<Arc<LoadedModel>> = OnceLock::new();
static MODEL_X4: OnceLock<Arc<LoadedModel>> = OnceLock::new();
static MODEL_RESTORE: OnceLock<Arc<LoadedModel>> = OnceLock::new();
static MODEL_INIT_X2: OnceLock<TokioMutex<()>> = OnceLock::new();
static MODEL_INIT_X4: OnceLock<TokioMutex<()>> = OnceLock::new();
static MODEL_INIT_RESTORE: OnceLock<TokioMutex<()>> = OnceLock::new();
static RESULT: OnceLock<Mutex<Option<Enlargement>>> = OnceLock::new();

/// What the window asked for: Restore, an enlargement, or Restore and then
/// an enlargement. One file either way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    /// Restore's Mask slider, 0..100, when Restore is on.
    pub restore: Option<f32>,
    /// 1 when there is no enlargement.
    pub scale: u32,
}

impl Recipe {
    pub fn new(restore: Option<f32>, scale: u32) -> Result<Self> {
        let restore = restore.map(|mask| {
            if mask.is_finite() {
                mask.clamp(0.0, 100.0)
            } else {
                super::restore::DEFAULT_MASK
            }
        });
        match (restore, scale) {
            (None, 1) => Err(anyhow!("Choose Restore, an enlargement, or both")),
            (_, 1 | 2 | 4) => Ok(Self { restore, scale }),
            _ => Err(anyhow!("Super resolution only supports 2x and 4x")),
        }
    }

    fn enlarge(self) -> Option<ModelKind> {
        ModelKind::from_scale(self.scale).ok()
    }

    /// `photo_Restored`, `photo_Upscaled_x2`, `photo_Restored_Upscaled_x4`.
    fn file_stem(self, stem: &str) -> String {
        let mut name = stem.to_string();
        if self.restore.is_some() {
            name.push_str("_Restored");
        }
        if self.scale > 1 {
            name.push_str(&format!("_Upscaled_x{}", self.scale));
        }
        name
    }
}

/// An enlarged photo, with what is needed to move its masks into it.
struct Enlargement {
    image: DynamicImage,
    scale: u32,
    /// The decoded source photo's size, which AI masks and colour-mask sample
    /// points are placed against.
    source_size: (u32, u32),
    recipe: Recipe,
    /// The RAW's capture-sharpening radius, carried to a restored file so
    /// Capture keeps working on it. None for a JPEG, which has no capture.
    capture_radius: Option<f32>,
}

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

fn result_slot() -> &'static Mutex<Option<Enlargement>> {
    RESULT.get_or_init(|| Mutex::new(None))
}

/// The bytes held by the last enlargement, and by Restore's kept model
/// output, for mods::memory's report. Zero if the slot is busy: that thread
/// is never made to wait.
pub fn result_bytes() -> usize {
    RESULT
        .get()
        .and_then(|slot| slot.try_lock().ok())
        .and_then(|g| g.as_ref().map(|enlarged| enlarged.image.as_bytes().len()))
        .unwrap_or(0)
        + super::restore::kept_bytes()
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
        ModelKind::Restore => &MODEL_RESTORE,
    }
}

fn model_init(kind: ModelKind) -> &'static TokioMutex<()> {
    match kind {
        ModelKind::X2 => MODEL_INIT_X2.get_or_init(|| TokioMutex::new(())),
        ModelKind::X4 => MODEL_INIT_X4.get_or_init(|| TokioMutex::new(())),
        ModelKind::Restore => MODEL_INIT_RESTORE.get_or_init(|| TokioMutex::new(())),
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

        let _ = app_handle.emit("ai-model-download-start", kind.download_name());
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
        let _ = app_handle.emit("ai-model-download-finish", kind.download_name());
        download?;

        if !verify_sha256(&path, spec.sha256)? {
            return Err(anyhow!(
                "The {} failed integrity verification",
                kind.download_name()
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
        kind,
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

/// `progress` hears about each tile; the window shows it, a test need not.
fn run_model(
    image: &Rgb32FImage,
    model: &LoadedModel,
    progress: &dyn Fn(ProgressPayload),
    scale: u32,
) -> Result<Rgb32FImage> {
    let (width, height) = image.dimensions();
    let output_width = width.checked_mul(scale).context("Output width overflow")?;
    let output_height = height
        .checked_mul(scale)
        .context("Output height overflow")?;
    let output_pixels = u64::from(output_width) * u64::from(output_height);
    let (adaptive_tile, output_memory) = adaptive_tile_size(scale, output_pixels)?;
    let tile_size = model.kind.fixed_tile().unwrap_or(adaptive_tile);
    let overlap = tile_overlap(tile_size);
    let verb = if model.kind == ModelKind::Restore {
        "Restoring"
    } else {
        "Upscaling"
    };

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
            progress(ProgressPayload {
                completed: tile_number + 1,
                total: total_tiles,
                message: format!(
                    "{verb} tile {}/{} on the {} using {}px tiles...",
                    tile_number + 1,
                    total_tiles,
                    model.device.label(),
                    tile_size
                ),
            });

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
                    "The {} model returned {}x{} for a {}x{} tile",
                    model.kind.label(),
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
    let progress = |payload: ProgressPayload| {
        let _ = app_handle.emit("super-resolution-progress", payload);
    };
    match run_model(image, model, &progress, scale) {
        Err(error) if model.device == Device::Gpu => {
            log::warn!("GPU enlargement failed, retrying on the CPU: {error:#}");
            let (session, device) = gpu_runtime::session(&model.path, Device::Cpu)?;
            let cpu = LoadedModel {
                session: Mutex::new(session),
                device,
                path: model.path.clone(),
                kind: model.kind,
            };
            run_model(image, &cpu, &progress, scale)
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

/// The framed photo to enlarge, and the size it was decoded at.
fn load_source(path: &str, app_handle: &tauri::AppHandle) -> Result<(DynamicImage, (u32, u32))> {
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
    let source_size = (image.width(), image.height());
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
    Ok((framed, source_size))
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

/// The models a recipe needs, loaded (and downloaded, once) before any work.
struct Models {
    restore: Option<Arc<LoadedModel>>,
    enlarge: Option<Arc<LoadedModel>>,
}

async fn models_for(app_handle: &tauri::AppHandle, recipe: Recipe) -> Result<Models, String> {
    let restore = match recipe.restore {
        Some(_) => Some(
            get_model(app_handle, ModelKind::Restore)
                .await
                .map_err(|error| format!("Cannot prepare Restore: {error}"))?,
        ),
        None => None,
    };
    let enlarge = match recipe.enlarge() {
        Some(kind) => Some(get_model(app_handle, kind).await.map_err(|error| {
            format!(
                "Cannot prepare {} super resolution: {}",
                kind.label(),
                error
            )
        })?),
        None => None,
    };
    Ok(Models { restore, enlarge })
}

/// One framed photo through what the recipe asks for, in the order measured
/// on AK's frames (DEC-25): Restore at its own size, where lens softness
/// lives; then, before an enlargement, capture sharpening at the camera's
/// size; then the enlargement. Restore on its own leaves capture to the
/// editor, live (`mark_restored`).
fn develop(
    framed: &super::restore::Framed,
    recipe: Recipe,
    models: &Models,
    app_handle: &tauri::AppHandle,
) -> Result<Rgb32FImage> {
    let source = framed.source.as_ref();
    let restored = match (recipe.restore, &models.restore) {
        (Some(mask), Some(model)) => {
            let output = match super::restore::kept(&framed.key) {
                Some(kept) => kept,
                None => {
                    let output = Arc::new(run_with_fallback(
                        source,
                        model,
                        app_handle,
                        ModelKind::Restore.scale(),
                    )?);
                    super::restore::keep(framed.key, output.clone());
                    output
                }
            };
            let (w, h) = source.dimensions();
            let mask =
                super::restore::mask_from_detail(&framed.detail, w as usize, h as usize, mask);
            Some(super::restore::blend_through(source, &output, &mask))
        }
        _ => None,
    };
    let Some(model) = &models.enlarge else {
        return restored.context("Nothing to do: choose Restore or an enlargement");
    };
    let native = restored.as_ref().unwrap_or(source);
    let captured = capture_before_enlarging(&framed.path, native, app_handle);
    run_with_fallback(
        captured.as_ref().unwrap_or(native),
        model,
        app_handle,
        recipe.scale,
    )
}

/// A RAW's capture sharpening, as its sidecar asks, applied before it is
/// enlarged - the enlarged file is an ordinary image and would never get it.
/// None for anything else, or if it cannot run: the enlargement goes ahead
/// without it, as it did before, and the log says why.
fn capture_before_enlarging(
    path: &str,
    image: &Rgb32FImage,
    app_handle: &tauri::AppHandle,
) -> Option<Rgb32FImage> {
    let (source_path, sidecar_path) = crate::file_management::parse_virtual_path(path);
    if !crate::formats::is_raw_file(&source_path) {
        return None;
    }
    let adjustments = crate::exif_processing::load_sidecar(&sidecar_path).adjustments;
    let state = app_handle.state::<crate::app_state::AppState>();
    let context = crate::gpu_processing::get_or_init_gpu_context(&state, app_handle)
        .map_err(|error| log::warn!("Enlarging without capture sharpening: {error}"))
        .ok()?;
    super::capture_bake::capture(
        &context,
        image,
        &adjustments,
        &source_path.to_string_lossy(),
    )
    .map_err(|error| log::warn!("Enlarging without capture sharpening: {error:#}"))
    .ok()
    .flatten()
}

/// The framed photo for `path`, decoded once while the window has it open.
fn framed_for(path: &str, app_handle: &tauri::AppHandle) -> Result<Arc<super::restore::Framed>> {
    super::restore::framed(path, || {
        let (source, source_size) = load_source(path, app_handle)?;
        Ok((source.to_rgb32f(), source_size))
    })
}

/// The window's preview of a framed photo, from its floats.
fn encode_preview_of(image: &Rgb32FImage) -> Result<String> {
    let (w, h) = image.dimensions();
    let scale = (2400.0 / w.max(h) as f32).min(1.0);
    let small = if scale < 1.0 {
        image::imageops::resize(
            image,
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
            FilterType::Lanczos3,
        )
    } else {
        image.clone()
    };
    encode_preview(&DynamicImage::ImageRgb32F(small))
}

/// Restore's mask for the photo in the window, white where it applies, as a
/// picture to show. The model is not involved: the mask comes from the photo.
pub async fn restore_mask(
    path: String,
    mask: f32,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let framed = framed_for(&path, &app_handle)?;
        let preview = super::restore::mask_preview(&framed, mask, 2400);
        encode_preview(&DynamicImage::ImageLuma8(preview))
    })
    .await
    .map_err(|error| format!("Restore mask failed: {error}"))?
    .map_err(|error| error.to_string())
}

/// A new Mask setting on the result already in the window - Restore alone -
/// without running the model again: its output is kept.
pub async fn restore_reblend(mask: f32) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let framed = super::restore::current_framed().context("No photo is open in the window")?;
        let restored =
            super::restore::kept(&framed.key).context("Restore has not run on this photo yet")?;
        let mut slot = result_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = slot.as_mut().context("No result to update")?;
        if result.recipe.restore.is_none() || result.recipe.scale != 1 {
            return Err(anyhow!("Only Restore on its own updates as the Mask moves"));
        }
        let (w, h) = framed.source.dimensions();
        let plane = super::restore::mask_from_detail(&framed.detail, w as usize, h as usize, mask);
        let blended = super::restore::blend_through(&framed.source, &restored, &plane);
        let preview = encode_preview_of(&blended)?;
        result.image = DynamicImage::ImageRgb32F(blended);
        result.recipe.restore = Some(mask);
        Ok(preview)
    })
    .await
    .map_err(|error| format!("Restore failed: {error}"))?
    .map_err(|error: anyhow::Error| error.to_string())
}

/// The radius capture sharpening uses on this RAW, for a restored copy of it
/// to keep using. Restore only: an enlargement's own model sharpens.
fn capture_radius_for(path: &str, recipe: Recipe) -> Option<f32> {
    let (source_path, _) = crate::file_management::parse_virtual_path(path);
    if recipe.restore.is_none() || recipe.scale != 1 || !crate::formats::is_raw_file(&source_path) {
        return None;
    }
    Some(
        super::sharpen::auto_radius(&source_path.to_string_lossy())
            .unwrap_or(super::sharpen::DEFAULT_RADIUS),
    )
}

/// Load, develop and keep one photo for the window's preview and its Save.
fn develop_path(
    path: &str,
    recipe: Recipe,
    models: &Models,
    app_handle: &tauri::AppHandle,
) -> Result<(Arc<super::restore::Framed>, Enlargement)> {
    let framed = framed_for(path, app_handle)?;
    let result =
        DynamicImage::ImageRgb32F(develop(&framed, recipe, models, app_handle).with_context(
            || format!("Cannot process '{path}': the model could not process this image"),
        )?);
    let enlarged = Enlargement {
        image: result,
        scale: recipe.scale,
        source_size: framed.source_size,
        recipe,
        capture_radius: capture_radius_for(path, recipe),
    };
    Ok((framed, enlarged))
}

async fn upscale(
    path: String,
    app_handle: tauri::AppHandle,
    models: Models,
    recipe: Recipe,
) -> Result<PreviewPayload> {
    let result = tokio::task::spawn_blocking(move || {
        let (framed, enlarged) = develop_path(&path, recipe, &models, &app_handle)?;
        let original_preview = encode_preview_of(&framed.source).with_context(|| {
            format!(
                "Cannot create a preview for '{}'. The decoded image format is not supported",
                path
            )
        })?;
        let result_preview = encode_preview(&enlarged.image)?;
        *result_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(enlarged);
        Ok::<PreviewPayload, anyhow::Error>(PreviewPayload {
            result: result_preview,
            original: original_preview,
        })
    })
    .await
    .map_err(|error| anyhow!("Super-resolution task failed: {error}"))??;
    Ok(result)
}

/// `restore` is Restore's Mask slider when Restore is on; `scale` is 1 for
/// no enlargement.
pub async fn preview(
    path: String,
    scale: u32,
    restore: Option<f32>,
    app_handle: tauri::AppHandle,
) -> Result<PreviewPayload, String> {
    let recipe = Recipe::new(restore, scale).map_err(|error| error.to_string())?;
    let models = models_for(&app_handle, recipe).await?;
    upscale(path, app_handle, models, recipe)
        .await
        .map_err(|error| error.to_string())
}

pub async fn save(original_path: String, app_handle: tauri::AppHandle) -> Result<String, String> {
    let enlarged = result_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .ok_or_else(|| "No super-resolution result found in memory".to_string())?;
    super::restore::forget();
    save_image(&original_path, enlarged, &app_handle).map_err(|error| error.to_string())
}

/// The window closed without saving: let go of what it was holding.
pub fn discard() {
    if let Some(slot) = RESULT.get() {
        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    super::restore::forget();
}

fn save_image(
    original_path: &str,
    enlarged: Enlargement,
    app_handle: &tauri::AppHandle,
) -> Result<String> {
    let image = &enlarged.image;
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
    let output_path = parent.join(format!(
        "{}.{}",
        enlarged.recipe.file_stem(stem),
        output_extension
    ));
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
    write_super_resolution_sidecar(
        &source_path,
        &sidecar_path,
        &output_path,
        &enlarged,
        app_handle,
    )?;
    Ok(output_path.to_string_lossy().to_string())
}

fn strip_non_transferable_adjustments(adjustments: &mut Value) {
    let Some(object) = adjustments.as_object_mut() else {
        return;
    };

    // These values refer to coordinates, masks, or pixels in the source
    // image, or describe framing already applied before enlargement. Copying
    // them would crop, rotate or correct the enlarged photo a second time.
    // Masks are not among them: carry_masks moves them into the enlargement.
    const SOURCE_COORDINATE_KEYS: &[&str] = &[
        "aiPatches",
        "aspectRatio",
        "crop",
        "flipHorizontal",
        "flipVertical",
        "guidedPerspective",
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
    enlarged: &Enlargement,
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
            Some(&source_path.to_string_lossy()),
        );
    }
    let frame = Frame::of(&metadata.adjustments, enlarged);
    carry_masks(&mut metadata.adjustments, &frame);
    strip_non_transferable_adjustments(&mut metadata.adjustments);
    mark_restored(&mut metadata.adjustments, enlarged.capture_radius);

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

/// A restored RAW is saved as an ordinary image, and capture sharpening is a
/// RAW's. Restore was asked for so that capture sharpening would have real
/// edges to work on, so the restored file says what it is, and the radius
/// measured on the RAW's mosaic - which the file no longer has -
/// for `sharpen::from_json`.
fn mark_restored(adjustments: &mut Value, capture_radius: Option<f32>) {
    if let (Some(radius), Some(object)) = (capture_radius, adjustments.as_object_mut()) {
        object.insert(
            super::sharpen::RESTORED_KEY.to_string(),
            json!({ "captureRadius": radius }),
        );
    }
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
    photo: Option<&str>,
) {
    if !adjustments.is_object() {
        *adjustments = json!({});
    }
    carry_white_balance(adjustments, as_shot);
    carry_view_transform(adjustments, raw_override, non_raw_override, photo);
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
    /// Raw tone rendering: the camera base curve, the auto-matched one, or DNG
    /// Standard's. DNG Standard's is applied to the largest and smallest
    /// channel in ProPhoto (ag_dng_standard); carried in the luma curve it runs
    /// on each channel instead, so saturated colours land a little stronger.
    Points(Vec<(f32, f32)>),
}

fn raw_view_curve(
    adjustments: &Value,
    raw_override: Option<u32>,
    photo: Option<&str>,
) -> RawViewCurve {
    // The renderer's own reading of the edit (image_processing.rs), so a photo
    // nobody edited is enlarged in DNG Standard, as it opens.
    let mode = crate::mods::raw_tone::mode_of(adjustments);
    if mode != 0 {
        let mut stored = adjustments["rawToneCurve"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        crate::mods::raw_tone::fill_missing(mode, &mut stored, photo);
        let mut points = curve_points(&Value::Array(stored));
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
    photo: Option<&str>,
) {
    let view = raw_view_curve(adjustments, raw_override, photo);
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

/// Where the enlargement sits in its source. Mask coordinates are measured in
/// the source straightened but not cropped (mask_generation.rs subtracts the
/// crop's corner), so a point moves by the crop and then by the enlargement.
struct Frame {
    origin: (f64, f64),
    scale: f64,
    source_size: (u32, u32),
    size: (u32, u32),
}

impl Frame {
    fn of(adjustments: &Value, enlarged: &Enlargement) -> Self {
        let crop = &adjustments["crop"];
        Self {
            origin: (
                crop["x"].as_f64().unwrap_or(0.0),
                crop["y"].as_f64().unwrap_or(0.0),
            ),
            scale: f64::from(enlarged.scale),
            source_size: enlarged.source_size,
            size: (enlarged.image.width(), enlarged.image.height()),
        }
    }

    fn point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.origin.0) * self.scale,
            (y - self.origin.1) * self.scale,
        )
    }
}

/// Move every mask into the enlargement, so it lands on the same part of the
/// photo. Shapes keep their kind and stay editable. AI masks are bitmaps of
/// the whole source, so they are re-rendered into the enlargement with the
/// editor's own placement. Colour and luminance masks keep their sample point,
/// but select again from the enlargement's pixels, so their edge can differ.
fn carry_masks(adjustments: &mut Value, frame: &Frame) {
    let Some(masks) = adjustments.get_mut("masks").and_then(Value::as_array_mut) else {
        return;
    };
    for sub_mask in masks
        .iter_mut()
        .filter_map(|mask| mask.get_mut("subMasks").and_then(Value::as_array_mut))
        .flatten()
    {
        let kind = sub_mask["type"].as_str().unwrap_or_default().to_string();
        let params = &mut sub_mask["parameters"];
        if !params.is_object() {
            continue;
        }
        match kind.as_str() {
            "radial" => {
                move_point(params, "centerX", "centerY", frame);
                scale_length(params, "radiusX", frame);
                scale_length(params, "radiusY", frame);
            }
            "linear" => {
                move_point(params, "startX", "startY", frame);
                move_point(params, "endX", "endY", frame);
                scale_length(params, "range", frame);
            }
            "brush" | "flow" | "clone" | "heal" | "liquify" | "retouch" => {
                let lines = params.get_mut("lines").and_then(Value::as_array_mut);
                for line in lines.into_iter().flatten() {
                    scale_length(line, "brushSize", frame);
                    let points = line.get_mut("points").and_then(Value::as_array_mut);
                    for point in points.into_iter().flatten() {
                        move_point(point, "x", "y", frame);
                    }
                }
            }
            "ai-subject" | "ai-foreground" | "ai-sky" | "ai-depth" | "quick-eraser" => {
                rerender_ai_mask(params, frame);
                move_point(params, "startX", "startY", frame);
                move_point(params, "endX", "endY", frame);
            }
            "color" | "luminance" => move_sample_point(params, frame),
            _ => {}
        }
    }
}

/// The panel parks the coordinates a mask kind does not use at -10000.
const PARKED: f64 = -9999.0;

fn move_point(value: &mut Value, x_key: &str, y_key: &str, frame: &Frame) {
    let (Some(x), Some(y)) = (value[x_key].as_f64(), value[y_key].as_f64()) else {
        return;
    };
    if x <= PARKED || y <= PARKED {
        return;
    }
    let (x, y) = frame.point(x, y);
    value[x_key] = json!(x);
    value[y_key] = json!(y);
}

fn scale_length(value: &mut Value, key: &str, frame: &Frame) {
    if let Some(length) = value[key].as_f64() {
        value[key] = json!(length * frame.scale);
    }
}

/// The source's orientation, flips and straightening an AI or colour mask
/// was made under. In the enlargement they are already applied.
fn take_placement(params: &mut Value) -> (f32, bool, bool, u8) {
    let placement = (
        params["rotation"].as_f64().unwrap_or(0.0) as f32,
        params["flipHorizontal"].as_bool().unwrap_or(false),
        params["flipVertical"].as_bool().unwrap_or(false),
        params["orientationSteps"].as_u64().unwrap_or(0) as u8,
    );
    params["rotation"] = json!(0);
    params["flipHorizontal"] = json!(false);
    params["flipVertical"] = json!(false);
    params["orientationSteps"] = json!(0);
    placement
}

/// Render the AI mask as the editor would over the enlarged frame of the
/// source, and keep that as the enlargement's own whole-image bitmap.
fn rerender_ai_mask(params: &mut Value, frame: &Frame) {
    let Some(data) = params["maskDataBase64"].as_str().map(str::to_owned) else {
        return;
    };
    let (rotation, flip_horizontal, flip_vertical, orientation_steps) = take_placement(params);
    let placement = crate::mask_generation::TransformParams {
        rotation,
        flip_horizontal,
        flip_vertical,
        orientation_steps,
        width: frame.size.0,
        height: frame.size.1,
        scale: frame.scale as f32,
        crop_offset: (
            (frame.origin.0 * frame.scale) as f32,
            (frame.origin.1 * frame.scale) as f32,
        ),
    };
    let rendered = crate::mask_generation::generate_ai_bitmap_from_base64(&data, &placement)
        .and_then(|mask| {
            let mut png = Cursor::new(Vec::new());
            mask.write_to(&mut png, ImageFormat::Png).ok()?;
            Some(format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(png.get_ref())
            ))
        });
    if rendered.is_none() {
        log::warn!(
            "Super resolution: an AI mask could not be moved into the enlargement; it is left empty"
        );
    }
    params["maskDataBase64"] = rendered.map_or(Value::Null, Value::String);
}

/// A colour or luminance mask's sample point is in the source as decoded.
/// This runs mask_generation.rs's placement the other way: orientation, then
/// flips, then straightening, into the frame the other masks are measured in.
fn move_sample_point(params: &mut Value, frame: &Frame) {
    let (Some(x), Some(y)) = (params["targetX"].as_f64(), params["targetY"].as_f64()) else {
        return;
    };
    let (rotation, flip_horizontal, flip_vertical, orientation_steps) = take_placement(params);
    let (x, y) = straightened_from_source(
        (x, y),
        frame.source_size,
        f64::from(rotation),
        flip_horizontal,
        flip_vertical,
        orientation_steps,
    );
    let (x, y) = frame.point(x, y);
    params["targetX"] = json!(x);
    params["targetY"] = json!(y);
}

fn straightened_from_source(
    (x, y): (f64, f64),
    (width, height): (u32, u32),
    rotation_degrees: f64,
    flip_horizontal: bool,
    flip_vertical: bool,
    orientation_steps: u8,
) -> (f64, f64) {
    let (width, height) = (f64::from(width), f64::from(height));
    let (w, h) = if orientation_steps % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let (x, y) = match orientation_steps {
        1 => (w - y, x),
        2 => (w - x, h - y),
        3 => (y, h - x),
        _ => (x, y),
    };
    let x = if flip_horizontal { w - x } else { x };
    let y = if flip_vertical { h - y } else { y };
    let (sin, cos) = rotation_degrees.to_radians().sin_cos();
    let (dx, dy) = (x - w / 2.0, y - h / 2.0);
    (dx * cos - dy * sin + w / 2.0, dx * sin + dy * cos + h / 2.0)
}

pub async fn batch(
    paths: Vec<String>,
    scale: u32,
    restore: Option<f32>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let recipe = Recipe::new(restore, scale).map_err(|error| error.to_string())?;
    let models = models_for(&app_handle, recipe).await?;
    tokio::task::spawn_blocking(move || {
        let mut saved = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let _ = app_handle.emit(
                "super-resolution-progress",
                ProgressPayload {
                    completed: index,
                    total: paths.len(),
                    message: format!("Processing photo {}/{}...", index + 1, paths.len()),
                },
            );
            let (_, enlarged) = develop_path(path, recipe, &models, &app_handle)
                .map_err(|error| format!("Cannot process '{}': {}", path, error))?;
            saved.push(save_image(path, enlarged, &app_handle).map_err(|error| error.to_string())?);
        }
        super::restore::forget();
        Ok(saved)
    })
    .await
    .map_err(|error| format!("Super-resolution batch failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_named_for_what_was_done() {
        let name = |restore, scale| Recipe::new(restore, scale).unwrap().file_stem("IMG_1");
        assert_eq!(name(Some(60.0), 1), "IMG_1_Restored");
        assert_eq!(name(None, 2), "IMG_1_Upscaled_x2");
        assert_eq!(name(None, 4), "IMG_1_Upscaled_x4");
        assert_eq!(name(Some(60.0), 4), "IMG_1_Restored_Upscaled_x4");
    }

    #[test]
    fn a_recipe_does_something_and_only_known_scales() {
        assert!(Recipe::new(None, 1).is_err());
        assert!(Recipe::new(Some(60.0), 3).is_err());
        assert_eq!(Recipe::new(Some(60.0), 1).unwrap().enlarge(), None);
        assert_eq!(Recipe::new(None, 2).unwrap().enlarge(), Some(ModelKind::X2));
    }

    /// Only a restored RAW at its own size keeps capture sharpening: an
    /// enlargement's model sharpens, and a JPEG never had capture.
    #[test]
    fn only_a_restored_raw_carries_its_capture_radius() {
        let restore = Recipe::new(Some(60.0), 1).unwrap();
        let both = Recipe::new(Some(60.0), 2).unwrap();
        assert_eq!(capture_radius_for("a.jpg", restore), None);
        assert_eq!(capture_radius_for("a.CR3", both), None);
        assert!(capture_radius_for("a.CR3", restore).is_some());

        let mut adjustments = json!({ "exposure": 1.0 });
        mark_restored(&mut adjustments, Some(0.72));
        assert_eq!(adjustments["agRestore"]["captureRadius"], json!(0.72f32));
        let mut untouched = json!({ "exposure": 1.0 });
        mark_restored(&mut untouched, None);
        assert!(untouched.get("agRestore").is_none());
    }

    /// Restore on a real RAW through the code the window runs: their decode,
    /// a framed region encoded for reopening, OmniLens in 256 px tiles, the
    /// detail mask. Writes as_shot, restored and mask PNGs and prints the time.
    ///
    /// `AG_RESTORE_RAW` the RAW; `AG_RESTORE_MODEL` the ONNX file;
    /// `AG_RESTORE_REGION` x,y,w,h of the decoded frame; `AG_RESTORE_OUT` a
    /// folder; `AG_ORT_DYLIB` the ONNX Runtime library to load (the DirectML
    /// build for the graphics card); `AG_RESTORE_DEVICE` gpu or cpu.
    #[test]
    #[ignore = "reads AK's photos and a local model; run by hand"]
    fn restore_on_a_real_raw() {
        let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
        let raw = var("AG_RESTORE_RAW");
        let model_path = PathBuf::from(var("AG_RESTORE_MODEL"));
        let out = PathBuf::from(var("AG_RESTORE_OUT"));
        let region: Vec<u32> = var("AG_RESTORE_REGION")
            .split(',')
            .map(|v| v.trim().parse().expect("x,y,w,h"))
            .collect();
        let _ = ort::init_from(var("AG_ORT_DYLIB")).commit();
        let device = if var("AG_RESTORE_DEVICE") == "gpu" {
            Device::Gpu
        } else {
            Device::Cpu
        };

        let bytes = fs::read(&raw).expect("read the RAW");
        let settings = crate::app_settings::AppSettings::default();
        let decoded =
            crate::image_loader::load_base_image_from_bytes(&bytes, &raw, false, &settings, None)
                .expect("decode");
        println!(
            "capture radius recorded at decode: {:?}",
            crate::mods::sharpen::auto_radius(&raw)
        );
        let mut framed = decoded.crop_imm(region[0], region[1], region[2], region[3]);
        encode_for_reopening(&mut framed);
        let source = framed.to_rgb32f();

        let (session, device) = gpu_runtime::session(&model_path, device).expect("session");
        let model = LoadedModel {
            session: Mutex::new(session),
            device,
            path: model_path,
            kind: ModelKind::Restore,
        };
        let started = std::time::Instant::now();
        let restored = run_model(&source, &model, &|_| {}, 1).expect("restore");
        let seconds = started.elapsed().as_secs_f32();
        let megapixels = f64::from(source.width() * source.height()) / 1e6;
        println!(
            "restore: {seconds:.1} s for {megapixels:.2} MP on the {} ({:.1} s/MP)",
            model.device.label(),
            seconds as f64 / megapixels
        );
        let blended =
            crate::mods::restore::blend(&source, &restored, crate::mods::restore::DEFAULT_MASK);
        fs::create_dir_all(&out).expect("out");
        let png = |image: &Rgb32FImage, name: &str| {
            DynamicImage::ImageRgb32F(image.clone())
                .to_rgb8()
                .save(out.join(name))
                .expect("save");
        };
        png(&source, "as_shot.png");
        png(&restored, "omnilens_whole.png");
        png(&blended, "restored_default_mask.png");
        crate::mods::restore::mask_image(&source, crate::mods::restore::DEFAULT_MASK)
            .save(out.join("mask.png"))
            .expect("save");

        // What an enlargement starts from: the restored photo with the RAW's
        // own capture sharpening, through the editor's engine.
        let sidecar = crate::exif_processing::get_primary_sidecar_path(Path::new(&raw));
        let adjustments = crate::exif_processing::load_sidecar(&sidecar).adjustments;
        let context = crate::mods::capture_bake::test_device().expect("a graphics device");
        let started = std::time::Instant::now();
        let captured = crate::mods::capture_bake::capture(&context, &blended, &adjustments, &raw)
            .expect("capture ran")
            .expect("capture is on for this RAW");
        println!(
            "capture before enlarging: {:.2} s for {megapixels:.2} MP",
            started.elapsed().as_secs_f32()
        );
        png(&captured, "restored_then_captured.png");
    }

    #[test]
    fn restore_runs_at_its_one_tile_and_enlargements_choose_theirs() {
        assert_eq!(ModelKind::Restore.fixed_tile(), Some(256));
        assert_eq!(ModelKind::Restore.scale(), 1);
        assert_eq!(ModelKind::X4.fixed_tile(), None);
    }

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
        assert_eq!(kept, ["exposure", "masks", "temperature"]);
    }

    /// A 2x enlargement of the crop starting at (100, 50) of a 400x300 photo.
    fn frame() -> Frame {
        Frame {
            origin: (100.0, 50.0),
            scale: 2.0,
            source_size: (400, 300),
            size: (300, 200),
        }
    }

    fn one_mask(kind: &str, parameters: Value) -> Value {
        json!({ "masks": [{ "subMasks": [{ "type": kind, "parameters": parameters }] }] })
    }

    fn carried(kind: &str, parameters: Value) -> Value {
        let mut adjustments = one_mask(kind, parameters);
        carry_masks(&mut adjustments, &frame());
        adjustments["masks"][0]["subMasks"][0]["parameters"].take()
    }

    #[test]
    fn shapes_move_with_the_crop_and_grow_with_the_enlargement() {
        let radial = carried(
            "radial",
            json!({ "centerX": 160, "centerY": 90, "radiusX": 40, "radiusY": 20, "rotation": -27, "feather": 0.6,
                    "startX": -10000, "startY": -10000 }),
        );
        assert_eq!(radial["centerX"], 120.0);
        assert_eq!(radial["centerY"], 80.0);
        assert_eq!(radial["radiusX"], 80.0);
        assert_eq!(radial["radiusY"], 40.0);
        assert_eq!(radial["rotation"], -27);
        assert_eq!(radial["feather"], 0.6);
        assert_eq!(radial["startX"], -10000, "a parked coordinate stays parked");

        let linear = carried(
            "linear",
            json!({ "startX": 110, "startY": 60, "endX": 210, "endY": 60, "range": 25 }),
        );
        assert_eq!(
            [
                &linear["startX"],
                &linear["startY"],
                &linear["endX"],
                &linear["endY"],
                &linear["range"]
            ],
            [20.0, 20.0, 220.0, 20.0, 50.0]
        );

        let brush = carried(
            "brush",
            json!({ "lines": [{ "tool": "brush", "brushSize": 30, "feather": 0.5,
                                "points": [{ "x": 120, "y": 70 }, { "x": 130, "y": 75 }] }] }),
        );
        let line = &brush["lines"][0];
        assert_eq!(line["brushSize"], 60.0);
        assert_eq!(line["feather"], 0.5);
        assert_eq!(line["points"][1], json!({ "x": 60.0, "y": 50.0 }));
    }

    fn png_data_url(mask: &image::GrayImage) -> String {
        let mut png = Cursor::new(Vec::new());
        mask.write_to(&mut png, ImageFormat::Png).unwrap();
        format!(
            "data:image/png;base64,{}",
            general_purpose::STANDARD.encode(png.get_ref())
        )
    }

    fn decode(data_url: &str) -> image::GrayImage {
        let data = &data_url[data_url.find(',').unwrap() + 1..];
        image::load_from_memory(&general_purpose::STANDARD.decode(data).unwrap())
            .unwrap()
            .to_luma8()
    }

    #[test]
    fn an_ai_mask_is_redrawn_over_the_enlargement() {
        // A whole-photo mask with one lit block, made on an unstraightened photo.
        let mut source = image::GrayImage::new(400, 300);
        for y in 70..90 {
            for x in 150..170 {
                source.put_pixel(x, y, image::Luma([255]));
            }
        }
        let parameters = carried(
            "ai-subject",
            json!({ "maskDataBase64": png_data_url(&source), "rotation": 0.0,
                    "startX": 150, "startY": 70, "endX": 170, "endY": 90 }),
        );
        let mask = decode(parameters["maskDataBase64"].as_str().unwrap());
        assert_eq!(mask.dimensions(), (300, 200));
        // The block sits at (150..170, 70..90) - (100, 50), doubled.
        assert_eq!(mask.get_pixel(100, 40)[0], 255);
        assert_eq!(mask.get_pixel(139, 79)[0], 255);
        assert_eq!(mask.get_pixel(99, 40)[0], 0);
        assert_eq!(mask.get_pixel(140, 80)[0], 0);
        assert_eq!(parameters["startX"], 100.0);
        assert_eq!(parameters["endY"], 80.0);
    }

    #[test]
    fn a_straightened_ai_mask_is_redrawn_where_the_editor_draws_it() {
        let mut source = image::GrayImage::new(400, 300);
        for (x, y, p) in source.enumerate_pixels_mut() {
            *p = image::Luma([((x * 7 + y * 3) % 256) as u8]);
        }
        let data = png_data_url(&source);
        let parameters = carried(
            "ai-foreground",
            json!({ "maskDataBase64": data, "rotation": -4.4, "flipHorizontal": true }),
        );
        assert_eq!(parameters["rotation"], 0);
        assert_eq!(parameters["flipHorizontal"], false);
        // What the editor would draw over the same 2x frame of the source.
        let expected = crate::mask_generation::generate_ai_bitmap_from_base64(
            &data,
            &crate::mask_generation::TransformParams {
                rotation: -4.4,
                flip_horizontal: true,
                flip_vertical: false,
                orientation_steps: 0,
                width: 300,
                height: 200,
                scale: 2.0,
                crop_offset: (200.0, 100.0),
            },
        )
        .unwrap();
        assert_eq!(
            decode(parameters["maskDataBase64"].as_str().unwrap()),
            expected
        );
    }

    /// mask_generation.rs's placement for a colour mask: from the straightened
    /// frame back to the source as decoded, at scale 1.
    fn editor_source_from_straightened(
        (x, y): (f64, f64),
        (width, height): (f64, f64),
        rotation: f64,
        flip_horizontal: bool,
        flip_vertical: bool,
        orientation_steps: u8,
    ) -> (f64, f64) {
        let (w, h) = if orientation_steps % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        let (sin, cos) = rotation.to_radians().sin_cos();
        let (dx, dy) = (x - w / 2.0, y - h / 2.0);
        let (x, y) = (
            dx * cos + dy * sin + w / 2.0,
            -dx * sin + dy * cos + h / 2.0,
        );
        let x = if flip_horizontal { w - x } else { x };
        let y = if flip_vertical { h - y } else { y };
        match orientation_steps {
            1 => (y, w - x),
            2 => (w - x, h - y),
            3 => (h - y, x),
            _ => (x, y),
        }
    }

    #[test]
    fn a_colour_sample_point_lands_on_the_same_pixel() {
        for steps in 0..4u8 {
            for (flip_h, flip_v) in [(false, false), (true, false), (false, true)] {
                let source = (123.0, 77.0);
                let straightened =
                    straightened_from_source(source, (400, 300), 7.5, flip_h, flip_v, steps);
                let back = editor_source_from_straightened(
                    straightened,
                    (400.0, 300.0),
                    7.5,
                    flip_h,
                    flip_v,
                    steps,
                );
                assert!(
                    (back.0 - source.0).abs() < 1e-9 && (back.1 - source.1).abs() < 1e-9,
                    "orientation {steps}, flips {flip_h}/{flip_v}: {source:?} came back as {back:?}"
                );
            }
        }

        let colour = carried(
            "color",
            json!({ "targetX": 140, "targetY": 90, "tolerance": 20, "rotation": 0 }),
        );
        assert_eq!([&colour["targetX"], &colour["targetY"]], [80.0, 80.0]);
        assert_eq!(colour["tolerance"], 20);
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
        let mut adjustments = json!({ "rawToneRendering": "default", "toneMapper": "basic" });
        carry_view_transform(&mut adjustments, None, None, None);
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
            "rawToneRendering": "default",
            "curves": {
                "luma": own.iter().map(|(x, y)| json!({ "x": x, "y": y })).collect::<Vec<_>>(),
                "red": red,
            },
        });
        carry_view_transform(&mut adjustments, None, None, None);
        let worst = worst_levels(&adjustments["curves"]["luma"], |s| {
            apply_curve(raw_basic_curve(s), &own)
        });
        assert!(worst < 1.5, "off by {worst} levels");
        assert_eq!(adjustments["curves"]["red"], red);
    }

    #[test]
    fn the_camera_base_curve_rides_in_the_luma_curve() {
        let mut adjustments = json!({ "rawToneRendering": "baseCurve", "toneMapper": "agx" });
        carry_view_transform(&mut adjustments, None, None, None);
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
        let mut adjustments = json!({ "rawToneRendering": "default", "toneMapper": "agx", "curves": { "luma": luma } });
        carry_view_transform(&mut adjustments, None, None, None);
        assert_eq!(adjustments["toneMapper"], "agx");
        assert_eq!(adjustments["curves"]["luma"], luma);

        // The Settings override is what the raw rendered with, not the sidecar.
        let mut overridden = json!({ "rawToneRendering": "default", "toneMapper": "basic", "curves": { "luma": luma } });
        carry_view_transform(&mut overridden, Some(1), None, None);
        assert_eq!(overridden["toneMapper"], "agx");
        assert_eq!(overridden["curves"]["luma"], luma);
    }

    #[test]
    fn an_unedited_raw_is_enlarged_in_dng_standard_as_it_opens() {
        let mut adjustments = Value::Null;
        carry_raw_look(
            &mut adjustments,
            WhiteBalance {
                temperature: 5600.0,
                tint: 4.0,
            },
            None,
            None,
            None,
        );
        assert_eq!(adjustments["toneMapper"], "basic");
        assert!(adjustments["whiteBalance"].is_null());
        let points: Vec<(f32, f32)> = crate::mods::dng_standard::curve_for_photo(None)
            .into_iter()
            .map(|p| (p.x, p.y))
            .collect();
        let worst = worst_levels(&adjustments["curves"]["luma"], |s| apply_curve(s, &points));
        assert!(worst < 1.0, "off by {worst} levels");
    }

    #[test]
    fn an_edit_in_rapidraws_rendering_keeps_its_look() {
        let mut adjustments = json!({ "rawToneRendering": "default" });
        carry_view_transform(&mut adjustments, None, None, None);
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
