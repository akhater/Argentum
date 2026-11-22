//! Capture sharpening applied to a picture Argentum is about to enlarge.
//!
//! Capture sharpening undoes the camera's own blur, so it belongs at the
//! camera's size: on R6 Mark III frames, 2026-10-11, capture after a 2x
//! enlargement doubled whiskers and outlined them, while capture before it
//! added detail the enlarging model kept (DEC-25, order A: Restore, then
//! Capture, then Enlarge). An enlargement is saved as an ordinary image, which
//! the editor never capture-sharpens, so it is applied here, before the
//! enlarging model runs, and is in the file's pixels.
//!
//! The same engine and the same settings as the editor: the RAW's `agSharpen`
//! through `sharpen::from_json`, run by `sharpen_gpu::run` on a texture of its
//! own and read back. Capture only - the manual Sharpen is output sharpening,
//! and stays live on the enlarged file, where it belongs.

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, Rgb32FImage};
use serde_json::Value;
use wgpu::util::{DeviceExt, TextureDataOrder};

use super::sharpen::{self, MAX_LOCAL};
use super::sharpen_gpu::{self, Job, MaskView, Target};
use crate::image_processing::GpuContext;

/// Rgba16Float.
const BYTES_PER_TEXEL: u32 = 8;

/// `image` capture-sharpened as `adjustments` ask for a RAW at `raw_path`,
/// or None when they ask for none. `image` is sRGB-encoded, as the
/// enlargement's source is (`encode_for_reopening`).
pub fn capture(
    context: &GpuContext,
    image: &Rgb32FImage,
    adjustments: &Value,
    raw_path: &str,
) -> Result<Option<Rgb32FImage>> {
    let mut params = sharpen::from_json(adjustments, true, Some(raw_path));
    if !params.capture_on() {
        return Ok(None);
    }
    params.usm_amount = 0.0;
    params.local = [0.0; MAX_LOCAL];

    let (width, height) = image.dimensions();
    crate::mods::gpu_size_limit::check_fits_gpu(
        width,
        height,
        context.limits.max_texture_dimension_2d,
    )
    .map_err(|e| anyhow!(e))?;
    let dynamic = DynamicImage::ImageRgb32F(image.clone());
    // Not a RAW to the engine: the pixels are sRGB-encoded, and is_raw false
    // is what makes the shader and the threshold linearise them first.
    let thresholds = sharpen::thresholds(&params, &dynamic, 0, false);
    let texels = crate::gpu_processing::to_rgba_f16(&dynamic);
    drop(dynamic);
    let input = context.device.create_texture_with_data(
        &context.queue,
        &wgpu::TextureDescriptor {
            label: Some("Capture before enlarging"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::MipMajor,
        bytemuck::cast_slice(&texels),
    );
    drop(texels);
    let view = input.create_view(&Default::default());
    let job = Job {
        src: &view,
        source: 0,
        family: 0,
        width,
        height,
        is_raw: false,
        params,
        contrast: thresholds.capture,
        usm_contrast: thresholds.usm,
        // No masks here: each mask's Sharpen is the editor's, live.
        local_contrast: [0.0; MAX_LOCAL],
        px_scale: 1.0,
        mask_view: MaskView::Off,
        masks: &[],
        region: None,
    };
    let Some(sharpened) = sharpen_gpu::run(context, &job, Target::Fresh) else {
        return Ok(None);
    };
    read_back(context, &sharpened, width, height).map(Some)
}

/// An Rgba16Float texture's RGB, as floats.
fn read_back(
    context: &GpuContext,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> Result<Rgb32FImage> {
    let row = (width * BYTES_PER_TEXEL).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Capture before enlarging, read back"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        view.texture().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    context.queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    context
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(120)),
        })
        .map_err(|error| anyhow!("The graphics card did not finish: {error:?}"))?;
    receiver
        .recv()
        .context("The read-back was abandoned")?
        .map_err(|error| anyhow!("Could not read the sharpened photo back: {error:?}"))?;
    let bytes = slice.get_mapped_range();
    let mut out = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height as usize {
        let start = y * row as usize;
        let line = &bytes[start..start + (width * BYTES_PER_TEXEL) as usize];
        // Four little-endian halves a texel; alpha is dropped.
        for texel in line.as_chunks::<8>().0 {
            for channel in &texel.as_chunks::<2>().0[..3] {
                out.push(half::f16::from_le_bytes(*channel).to_f32());
            }
        }
    }
    drop(bytes);
    buffer.unmap();
    Rgb32FImage::from_raw(width, height, out).context("Read back the wrong number of pixels")
}

/// A graphics device for tests, or None on a machine without one.
#[cfg(test)]
pub(crate) fn test_device() -> Option<GpuContext> {
    tests::device()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    use serde_json::json;

    pub(super) fn device() -> Option<GpuContext> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .ok()?;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("capture bake test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        Some(GpuContext {
            device: std::sync::Arc::new(device),
            queue: std::sync::Arc::new(queue),
            limits,
            display: std::sync::Arc::new(std::sync::Mutex::new(None)),
            pipelines: Default::default(),
        })
    }

    /// A soft vertical edge, sRGB-encoded, the same down every column.
    fn soft_edge(w: u32, h: u32) -> Rgb32FImage {
        Rgb32FImage::from_fn(w, h, |x, _| {
            let t = 1.0 / (1.0 + (-(x as f32 - w as f32 / 2.0) / 1.2).exp());
            let v = 0.2 + 0.5 * t;
            Rgb([v, v, v])
        })
    }

    fn steepest(image: &Rgb32FImage) -> f32 {
        let y = image.height() / 2;
        (1..image.width())
            .map(|x| image.get_pixel(x, y)[1] - image.get_pixel(x - 1, y)[1])
            .fold(0.0, f32::max)
    }

    #[test]
    fn capture_steepens_an_edge_and_off_means_untouched() {
        let Some(context) = device() else {
            eprintln!("no graphics adapter; skipped");
            return;
        };
        let edge = soft_edge(256, 64);
        let manual = json!({ "agSharpen": { "autoRadius": false, "radius": 1.0, "autoContrast": false, "contrast": 0 } });
        let sharpened = capture(&context, &edge, &manual, "edge.CR3")
            .expect("ran")
            .expect("capture is on by default for a RAW");
        assert_eq!(sharpened.dimensions(), edge.dimensions());
        assert!(
            steepest(&sharpened) > steepest(&edge) * 1.2,
            "{} vs {}",
            steepest(&sharpened),
            steepest(&edge)
        );
        // Far from the edge nothing moves.
        let far = (sharpened.get_pixel(8, 32)[1] - edge.get_pixel(8, 32)[1]).abs();
        assert!(far < 0.01, "{far}");

        let off = json!({ "agSharpen": { "capture": false } });
        assert!(
            capture(&context, &edge, &off, "edge.CR3")
                .unwrap()
                .is_none()
        );
    }

    /// The manual Sharpen is output sharpening: it stays on the enlarged file,
    /// live, and is not applied here a second time.
    #[test]
    fn only_capture_is_applied_here() {
        let Some(context) = device() else {
            eprintln!("no graphics adapter; skipped");
            return;
        };
        let edge = soft_edge(256, 64);
        let capture_only = json!({ "agSharpen": { "autoRadius": false, "radius": 1.0, "autoContrast": false, "contrast": 0 } });
        let with_sharpen = json!({ "agSharpen": { "autoRadius": false, "radius": 1.0, "autoContrast": false, "contrast": 0, "amount": 150 } });
        let a = capture(&context, &edge, &capture_only, "edge.CR3")
            .unwrap()
            .unwrap();
        let b = capture(&context, &edge, &with_sharpen, "edge.CR3")
            .unwrap()
            .unwrap();
        assert_eq!(a.as_raw(), b.as_raw());
    }
}
