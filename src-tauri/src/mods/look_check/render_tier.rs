//! The render tier: a sample as the app develops and renders it, in the
//! states the check keeps. Every step is the app's own call: the decode with
//! `image_loader::load_base_image_from_bytes`, the as-shot white balance and
//! exposure with `raw_processing::read_as_shot`, the edit through
//! `image_processing::get_all_adjustments_from_json`, made small with
//! `downscale_f32_image`, rendered as `export_precision` renders an export,
//! all with `AppSettings::default()`. The calls #171's A/B measurements made.

use std::path::Path;
use std::sync::Arc;

use image::{DynamicImage, Rgba32FImage};
use serde_json::{Value, json};

use crate::app_settings::AppSettings;
use crate::gpu_processing::RenderRequest;
use crate::image_processing::GpuContext;
use crate::lut_processing::Lut;
use crate::mods::export_precision::HighPrecisionRenderer;
use crate::mods::highlights;

/// The long side every sample is rendered at.
pub const RENDER_LONG: u32 = 1600;

/// The built-in film look the fourth state puts on.
pub const FILM_LOOK: &str = "K-Portra 400.cube";

/// The extra state an X-Trans sample gets: Exposure -2 with recovery off.
pub const NO_RECOVERY: &str = "no-recovery-exposure-2";

/// The states every sample is rendered in, in order: the name, and the keys
/// each adds to the edit the photo opens with.
pub fn states(film_look: &Path) -> Vec<(&'static str, Value)> {
    vec![
        // DNG Standard, as a photo nobody has edited opens.
        ("defaults", json!({})),
        // What recovery rebuilt above the sensor's white.
        ("exposure-2", json!({ "exposure": -2.0 })),
        // RapidRAW 1.6.7's Highlights and Whites (#171).
        (
            "highlights-whites",
            json!({ "highlights": -100.0, "whites": 50.0 }),
        ),
        // The film look's cap and its V-Log input (#146, #150), as the Looks
        // tab puts it on (src/argentum/looks/filmLookInput.ts).
        (
            "portra-400",
            json!({
                "lutPath": film_look.to_string_lossy(),
                "lutName": FILM_LOOK,
                "lutIntensity": 100,
                "lutIsSceneReferred": true,
                crate::mods::film_lut::KEY: true,
            }),
        ),
    ]
}

/// The film look's file, among the app's own.
pub fn film_look() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources/film_luts")
        .join(FILM_LOOK)
}

/// A device as the app opens one (`gpu_processing::get_or_init_gpu_context`),
/// without a window: the backends `WGPU_BACKEND` allows (on Windows the
/// primary ones otherwise), the adapter `WGPU_ADAPTER_NAME` names or the
/// high-performance one, every limit the adapter has.
///
/// With the adapter as references record it: a software adapter (lavapipe,
/// WARP) by name, backend and driver, which are the same on every machine; a
/// graphics card by backend and kind only, so a reference names no one's
/// hardware. The full name is printed.
pub fn device() -> Result<(GpuContext, String), String> {
    #[allow(unused_mut)]
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    #[cfg(target_os = "windows")]
    if std::env::var("WGPU_BACKEND").is_err() {
        desc.backends = wgpu::Backends::PRIMARY;
    }
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env(&instance, None))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            }))
        })
        .map_err(|e| format!("no GPU adapter: {e}"))?;
    let info = adapter.get_info();
    let full = format!(
        "{} ({:?}, {} {})",
        info.name, info.backend, info.driver, info.driver_info
    )
    .replace(['\t', '\n'], " ");
    eprintln!("look check: adapter {full}");
    let name = if info.device_type == wgpu::DeviceType::Cpu {
        full
    } else {
        format!("{:?} ({:?})", info.device_type, info.backend)
    };
    let mut required_features = wgpu::Features::empty();
    if adapter
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }
    let limits = adapter.limits();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("look check"),
        required_features,
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| e.to_string())?;
    Ok((
        GpuContext {
            device: Arc::new(device),
            queue: Arc::new(queue),
            limits,
            display: Arc::new(std::sync::Mutex::new(None)),
        },
        name,
    ))
}

/// The sample decoded as the app opens it, made `RENDER_LONG` on its long
/// side.
pub fn develop(bytes: &[u8], path: &str, settings: &AppSettings) -> Result<DynamicImage, String> {
    let full = crate::image_loader::load_base_image_from_bytes(bytes, path, false, settings, None)
        .map_err(|e| e.to_string())?;
    Ok(crate::image_processing::downscale_f32_image(
        &full,
        RENDER_LONG,
        RENDER_LONG,
    ))
}

/// The same, with highlight recovery switched off for the decode.
pub fn develop_without_recovery(
    bytes: &[u8],
    path: &str,
    settings: &AppSettings,
) -> Result<DynamicImage, String> {
    /// Recovery switched back on when dropped, even if the decode panics.
    struct On;
    impl Drop for On {
        fn drop(&mut self) {
            highlights::set_enabled(true);
        }
    }
    highlights::set_enabled(false);
    let _on = On;
    develop(bytes, path, settings)
}

/// The edit the app starts a photo with when it opens: the camera's built-in
/// lens correction where it starts on (src/argentum/lensBuiltin.ts).
pub fn opening_edit(bytes: &[u8]) -> Value {
    match argentum_engine::rm_lens_builtin::read(bytes) {
        Some(c) if c.default_on => json!({
            crate::mods::lens_builtin::KEYS[0]: true,
            crate::mods::lens_builtin::KEYS[1]: c,
        }),
        _ => json!({}),
    }
}

/// `base` with `keys` set on it.
pub fn merged(base: &Value, keys: &Value) -> Value {
    let mut out = base.clone();
    if let (Some(out), Some(keys)) = (out.as_object_mut(), keys.as_object()) {
        for (k, v) in keys {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// What the app asks the GPU for, for this photo with this edit: the edit as
/// the app reads it, against the photo's as-shot white balance and exposure.
pub fn request(
    edit: &Value,
    bytes: &[u8],
    path: &str,
    settings: &AppSettings,
    lut: &Arc<Lut>,
) -> RenderRequest<'static> {
    let shot = crate::raw_processing::read_as_shot(bytes)
        .unwrap_or_else(crate::as_shot::AsShot::reference);
    let adjustments = crate::image_processing::get_all_adjustments_from_json(
        edit,
        true,
        shot,
        crate::image_processing::resolve_render_options(settings, true),
        Some(path),
    );
    RenderRequest {
        adjustments,
        mask_bitmaps: &[],
        lut: edit["lutPath"].is_string().then(|| lut.clone()),
        roi: None,
    }
}

pub fn render(
    renderer: &HighPrecisionRenderer,
    image: &DynamicImage,
    request: RenderRequest,
) -> Result<Rgba32FImage, String> {
    renderer.render(image, request).map(|out| out.to_rgba32f())
}

/// Every value in the highlights one 8-bit level brighter: the smallest
/// change the check exists for, put on a real render to show it is seen.
pub fn one_level_brighter_highlights(image: &Rgba32FImage) -> Rgba32FImage {
    use argentum_engine::colour::{luma, srgb_decode};
    let mut out = image.clone();
    for p in out.pixels_mut() {
        if luma([p[0], p[1], p[2]].map(srgb_decode)) > 0.5 {
            for c in 0..3 {
                p[c] = (p[c] + 1.0 / 255.0).min(1.0);
            }
        }
    }
    out
}
