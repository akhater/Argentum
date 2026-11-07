//! The editor's render on real photos: that what it keeps on the GPU between
//! renders changes no pixel, and what one slider tick costs. Ours.
//!
//! They read the RAWs in `AG_RAWS` (paths separated by ';'), decoded as the
//! app opens them, and render through RapidRAW's processor the way the
//! preview does (`GpuProcessor::run`), after the input stage
//! (`input_stage.rs`: Noise Reduction and sharpening):
//!
//! ```text
//! AG_RAWS="a.CR3;b.CR2" cargo test --lib mods::render_check:: -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `same_pixels` prints a hash per render, in a sequence that moves sliders
//! the way an edit does: run it before and after a change to the render path,
//! and the lines must match. `slider_tick` prints how long a tick takes at the
//! editor's preview size, each waited for on the GPU.
//! `fit_after_zoom_is_a_fresh_fit` holds the fit render after a zoom to 100%
//! to the fresh one, byte for byte. `noise_reduction_order` measures noise
//! and edges with Noise Reduction before sharpening and after it.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use image::{DynamicImage, GenericImageView, ImageBuffer, Luma};
use serde_json::{Value, json};

use super::input_stage;
use super::sharpen;
use super::sharpen_gpu::Target;
use crate::app_settings::AppSettings;
use crate::gpu_processing::{GpuProcessor, RenderRequest, Roi};
use crate::image_processing::GpuContext;
use crate::lut_processing::Lut;

type MaskBitmap = ImageBuffer<Luma<u8>, Vec<u8>>;

/// The editor's preview long side when nothing asks for another
/// (`editor_preview_resolution` in lib.rs), and a larger one that takes two
/// of the processor's 2048 px tiles across.
const SIZES: [u32; 2] = [1920, 3200];

/// A device as the app opens one, without a window: on Windows the primary
/// backends unless `WGPU_BACKEND` names others, the high-performance adapter,
/// every limit it has.
fn device() -> GpuContext {
    #[allow(unused_mut)]
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    #[cfg(target_os = "windows")]
    if std::env::var("WGPU_BACKEND").is_err() {
        desc.backends = wgpu::Backends::PRIMARY;
    }
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("a GPU adapter");
    let info = adapter.get_info();
    eprintln!(
        "adapter: {} ({:?}, {})",
        info.name, info.backend, info.driver_info
    );
    let mut required_features = wgpu::Features::empty();
    if adapter
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }
    let limits = adapter.limits();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("render check"),
        required_features,
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("a device");
    GpuContext {
        device: Arc::new(device),
        queue: Arc::new(queue),
        limits,
        display: Arc::new(Mutex::new(None)),
    }
}

fn raws() -> Vec<String> {
    std::env::var("AG_RAWS")
        .expect("set AG_RAWS")
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// A RAW decoded as the app opens it.
struct Photo {
    path: String,
    shot: crate::as_shot::AsShot,
    full: DynamicImage,
}

fn open(path: &str) -> Photo {
    let bytes = std::fs::read(path).expect("read raw");
    let shot = crate::raw_processing::read_as_shot(&bytes)
        .unwrap_or_else(crate::as_shot::AsShot::reference);
    let full = crate::image_loader::load_base_image_from_bytes(
        &bytes,
        path,
        false,
        &AppSettings::default(),
        None,
    )
    .expect("decodes");
    Photo {
        path: path.to_string(),
        shot,
        full,
    }
}

fn film_lut(name: &str) -> Arc<Lut> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources/film_luts")
        .join(name);
    Arc::new(crate::lut_processing::parse_lut_file(&path.to_string_lossy()).expect("a LUT"))
}

/// A soft round mask centred at (cx, cy), fractions of the frame.
fn round_mask(w: u32, h: u32, cx: f32, cy: f32) -> MaskBitmap {
    let r = w.min(h) as f32 * 0.35;
    MaskBitmap::from_fn(w, h, |x, y| {
        let d = ((x as f32 - cx * w as f32).hypot(y as f32 - cy * h as f32) / r).min(1.0);
        Luma([((1.0 - d * d) * 255.0) as u8])
    })
}

/// Glow, Halation, RapidRAW's sharpness, a LUT and one mask with an exposure
/// and a glow of its own; sharpening off. `keys` go on top.
fn edit(keys: Value) -> Value {
    let mut js = json!({
        "glowAmount": 40,
        "halationAmount": 40,
        "sharpness": 30,
        "lutPath": "film",
        "lutIntensity": 80,
        "agSharpen": { "capture": false, "amount": 0 },
        "masks": [{
            "id": "m1",
            "name": "Mask",
            "visible": true,
            "invert": false,
            "adjustments": { "exposure": 0.5, "glowAmount": 20 },
            "subMasks": [{
                "id": "s1",
                "type": "radial",
                "visible": true,
                "mode": "additive",
                "parameters": {},
            }],
        }],
    });
    if let (Some(js), Some(keys)) = (js.as_object_mut(), keys.as_object()) {
        for (k, v) in keys {
            js.insert(k.clone(), v.clone());
        }
    }
    js
}

/// One photo at one size on a processor of its own, as the preview keeps it.
struct Bench<'a> {
    ctx: &'a GpuContext,
    photo: &'a Photo,
    image: DynamicImage,
    processor: GpuProcessor,
    input: wgpu::TextureView,
    gf: wgpu::TextureView,
    dehaze: wgpu::TextureView,
    key: u64,
}

impl<'a> Bench<'a> {
    fn new(ctx: &'a GpuContext, photo: &'a Photo, long: u32, key: u64) -> Self {
        let image = crate::image_processing::downscale_f32_image(&photo.full, long, long);
        let (w, h) = image.dimensions();
        let processor =
            GpuProcessor::new(ctx.clone(), (w + 255) & !255, (h + 255) & !255).expect("processor");
        let input = Self::upload(ctx, &image);
        let (gf, dehaze) = processor.build_guided_coeffs(&input, w, h, 1);
        Self {
            ctx,
            photo,
            image,
            processor,
            input,
            gf,
            dehaze,
            key,
        }
    }

    /// `image` as the preview's input texture, as gpu_processing.rs makes it.
    fn upload(ctx: &GpuContext, image: &DynamicImage) -> wgpu::TextureView {
        let (w, h) = image.dimensions();
        let texels = crate::gpu_processing::to_rgba_f16(image);
        use wgpu::util::DeviceExt;
        ctx.device
            .create_texture_with_data(
                &ctx.queue,
                &wgpu::TextureDescriptor {
                    label: Some("Input Texture"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::MipMajor,
                bytemuck::cast_slice(&texels),
            )
            .create_view(&Default::default())
    }

    fn request<'m>(
        &self,
        js: &Value,
        masks: &'m [MaskBitmap],
        lut: &Arc<Lut>,
        roi: Option<Roi>,
    ) -> RenderRequest<'m> {
        RenderRequest {
            adjustments: crate::image_processing::get_all_adjustments_from_json(
                js,
                true,
                self.photo.shot,
                crate::image_processing::resolve_render_options(&AppSettings::default(), true),
                Some(&self.photo.path),
            ),
            mask_bitmaps: masks,
            lut: js["lutPath"].is_string().then(|| lut.clone()),
            roi,
        }
    }

    /// One render, read back unless `display`, as the editor makes it: the
    /// input stage, its results kept between renders, then theirs.
    fn render(&self, request: RenderRequest, display: bool) -> Vec<u8> {
        let (w, h) = self.image.dimensions();
        let at = input_stage::At {
            px_scale: 1.0,
            measure_on: &self.image,
            measure_key: self.key,
            source: self.key,
            family: self.key,
        };
        let (staged, _) =
            input_stage::develop(self.ctx, &self.input, w, h, request, &at, Target::Kept);
        let (pixels, ..) = self
            .processor
            .run(
                staged.view.as_ref().unwrap_or(&self.input),
                &self.gf,
                &self.dehaze,
                w,
                h,
                staged.request,
                display,
            )
            .expect("renders");
        pixels
    }
}

fn wait(ctx: &GpuContext) {
    let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..16].to_string()
}

/// The histogram's and the waveform's hashes for RGBA8 `pixels`, laid out
/// `row_stride` bytes per row as the analytics worker is given them.
fn analytics(pixels: &[u8], w: u32, h: u32, row_stride: usize) -> (String, String) {
    use crate::image_processing::{AnalyticsFrame, AnalyticsPixels};
    let mut data = vec![0u8; row_stride * h as usize];
    for (row, out) in pixels
        .chunks_exact(w as usize * 4)
        .zip(data.chunks_exact_mut(row_stride))
    {
        out[..row.len()].copy_from_slice(row);
    }
    let frame = AnalyticsFrame {
        data: AnalyticsPixels::Owned(data),
        width: w,
        height: h,
        row_stride,
    };
    let histogram = crate::image_processing::calculate_histogram(&frame).expect("histogram");
    let waveform = crate::image_processing::calculate_waveform(&frame, None).expect("waveform");
    (
        hash(serde_json::to_string(&histogram).unwrap().as_bytes()),
        hash(serde_json::to_string(&waveform).unwrap().as_bytes()),
    )
}

/// Every render of the sequence below, hashed, for each RAW at each size, and
/// the histogram and waveform of the first. A step that moves only an
/// exposure is a frame whose blurs, masks and LUT are the last one's.
#[test]
#[ignore = "reads RAW files and needs a GPU; run by hand"]
fn same_pixels() {
    let ctx = device();
    let luts = [film_lut("K-Portra 400.cube"), film_lut("F-Pro 400H.cube")];
    let steps: Vec<(&str, Value, usize, usize, bool)> = vec![
        // name, keys on the edit, which mask, which LUT, a region of interest
        ("base", json!({}), 0, 0, false),
        ("exposure +0.4", json!({ "exposure": 0.4 }), 0, 0, false),
        ("exposure +0.8", json!({ "exposure": 0.8 }), 0, 0, false),
        (
            "glow 60",
            json!({ "exposure": 0.8, "glowAmount": 60 }),
            0,
            0,
            false,
        ),
        (
            "mask moved",
            json!({ "exposure": 0.8, "glowAmount": 60 }),
            1,
            0,
            false,
        ),
        (
            "other LUT",
            json!({ "exposure": 0.8, "glowAmount": 60 }),
            1,
            1,
            false,
        ),
        (
            "sharpen 50",
            json!({ "agSharpen": { "amount": 50 } }),
            1,
            1,
            false,
        ),
        (
            "sharpen 120",
            json!({ "agSharpen": { "amount": 120 } }),
            1,
            1,
            false,
        ),
        (
            "sharpen 120, exposure -0.3",
            json!({ "agSharpen": { "amount": 120 }, "exposure": -0.3 }),
            1,
            1,
            false,
        ),
        ("sharpen off", json!({ "exposure": -0.3 }), 1, 1, false),
        ("region", json!({ "exposure": -0.3 }), 1, 1, true),
        ("region, exposure 0", json!({}), 1, 1, true),
        (
            "no mask, no LUT",
            json!({ "masks": [], "lutPath": null }),
            2,
            0,
            false,
        ),
        ("back to base", json!({}), 0, 0, false),
    ];
    for (n, path) in raws().iter().enumerate() {
        let photo = open(path);
        for (s, long) in SIZES.into_iter().enumerate() {
            let bench = Bench::new(&ctx, &photo, long, (n * 10 + s + 1) as u64);
            let (w, h) = bench.image.dimensions();
            let masks = [
                vec![round_mask(w, h, 0.4, 0.45)],
                vec![round_mask(w, h, 0.6, 0.55)],
                Vec::new(),
            ];
            let roi = Roi {
                x: w / 4,
                y: h / 5,
                width: w / 2,
                height: h / 2,
            };
            for (name, keys, mask, lut, region) in &steps {
                let js = edit(keys.clone());
                let request = bench.request(&js, &masks[*mask], &luts[*lut], region.then_some(roi));
                let pixels = bench.render(request, false);
                println!("photo {} {w}x{h} {name}: {}", n + 1, hash(&pixels));
                if *name == "base" {
                    // As the CPU readback hands the frame on, and with rows padded the way
                    // the GPU's readback buffer pads them: the same histogram and waveform.
                    let tight = analytics(&pixels, w, h, w as usize * 4);
                    let padded = analytics(&pixels, w, h, (w as usize * 4).next_multiple_of(256));
                    assert_eq!(tight, padded, "padded rows change the analytics");
                    println!(
                        "photo {} {w}x{h} histogram: {}, waveform: {}",
                        n + 1,
                        tight.0,
                        tight.1
                    );
                }
            }
            // The export, which renders on a processor of its own.
            let js = edit(json!({}));
            let exported = crate::mods::export_precision::render_high_precision(
                &ctx,
                &bench.image,
                bench.request(&js, &masks[0], &luts[0], None),
            )
            .expect("exports");
            println!(
                "photo {} {w}x{h} export: {}",
                n + 1,
                hash(exported.as_bytes())
            );
            input_stage::reset();
        }
    }
}

/// A slider tick at the editor's preview size: Exposure moved on an edit
/// with Glow, Halation, a mask and a LUT, drawn to the screen as the editor
/// draws it (no readback), each tick waited for on the GPU. Then the same
/// with sharpening on, with Noise Reduction on as well (kept, as Exposure
/// moves), with Noise Reduction moving (its pass runs every tick), and a plain
/// edit with none of those. The median of twelve ticks per photo, after two to
/// warm up.
#[test]
#[ignore = "reads RAW files and needs a GPU; timing, run by hand"]
fn slider_tick() {
    let ctx = device();
    let lut = film_lut("K-Portra 400.cube");
    // Name, keys on the edit, the slider that moves and its step.
    let cases: [(&str, Value, &str, f64); 5] = [
        ("effects, mask, LUT", json!({}), "exposure", 0.05),
        (
            "same, sharpening on",
            json!({ "agSharpen": { "amount": 50 } }),
            "exposure",
            0.05,
        ),
        (
            "same, noise reduction 30",
            json!({ "agSharpen": { "amount": 50 }, "lumaNoiseReduction": 30 }),
            "exposure",
            0.05,
        ),
        (
            "same, noise reduction moving",
            json!({ "agSharpen": { "amount": 50 } }),
            "lumaNoiseReduction",
            5.0,
        ),
        (
            "plain edit",
            json!({ "glowAmount": 0, "halationAmount": 0, "sharpness": 0, "masks": [], "lutPath": null }),
            "exposure",
            0.05,
        ),
    ];
    let mut medians: Vec<Vec<f32>> = vec![Vec::new(); SIZES.len() * cases.len()];
    for (n, path) in raws().iter().enumerate() {
        let photo = open(path);
        for (s, long) in SIZES.into_iter().enumerate() {
            let bench = Bench::new(&ctx, &photo, long, (n * 10 + s + 1) as u64);
            let (w, h) = bench.image.dimensions();
            let masks = vec![round_mask(w, h, 0.4, 0.45)];
            for (c, (name, keys, slider, step)) in cases.iter().enumerate() {
                let mut ticks = Vec::new();
                for i in 0..14 {
                    let mut keys = keys.clone();
                    keys[*slider] = json!(step * (i % 7 + 1) as f64);
                    let js = edit(keys);
                    let started = Instant::now();
                    bench.render(bench.request(&js, &masks, &lut, None), true);
                    wait(&ctx);
                    if i >= 2 {
                        ticks.push(started.elapsed().as_secs_f32() * 1000.0);
                    }
                }
                ticks.sort_by(f32::total_cmp);
                let median = ticks[ticks.len() / 2];
                eprintln!(
                    "photo {} {w}x{h} {name}: median {median:.1} ms, fastest {:.1} ms",
                    n + 1,
                    ticks[0]
                );
                medians[s * cases.len() + c].push(median);
            }
            input_stage::reset();
        }
    }
    for (s, long) in SIZES.into_iter().enumerate() {
        for (c, (name, ..)) in cases.iter().enumerate() {
            let m = &medians[s * cases.len() + c];
            eprintln!(
                "{long} px long side, {name}: {:.1} ms (mean of the photos' medians)",
                m.iter().sum::<f32>() / m.len() as f32
            );
        }
    }
}

/// One editor render of `photo` at `long` px on `processor`, staged as
/// `input_stage.rs` stages it: sized by render pixels per full-resolution
/// pixel, kept between renders, sharpening measured on the full picture.
fn render_at(
    ctx: &GpuContext,
    processor: &GpuProcessor,
    photo: &Photo,
    js: &Value,
    long: u32,
) -> Vec<u8> {
    // At full size the preview is the full picture itself (lib.rs compute_transformed_preview).
    let image = if long >= photo.full.width().max(photo.full.height()) {
        photo.full.clone()
    } else {
        crate::image_processing::downscale_f32_image(&photo.full, long, long)
    };
    let (w, h) = image.dimensions();
    let input = Bench::upload(ctx, &image);
    let (gf, dehaze) = processor.build_guided_coeffs(&input, w, h, 1);
    let adjustments = crate::image_processing::get_all_adjustments_from_json(
        js,
        true,
        photo.shot,
        crate::image_processing::resolve_render_options(&AppSettings::default(), true),
        Some(&photo.path),
    );
    let at = input_stage::At {
        px_scale: (w as f32 / photo.full.width() as f32).min(1.0),
        measure_on: &photo.full,
        measure_key: 0,
        source: sharpen::picture_key(&image, 0),
        family: sharpen::picture_key(&photo.full, 0),
    };
    let request = RenderRequest {
        adjustments,
        mask_bitmaps: &[],
        lut: None,
        roi: None,
    };
    let (staged, _) = input_stage::develop(ctx, &input, w, h, request, &at, Target::Kept);
    let (pixels, ..) = processor
        .run(
            staged.view.as_ref().unwrap_or(&input),
            &gf,
            &dehaze,
            w,
            h,
            staged.request,
            false,
        )
        .expect("renders");
    pixels
}

/// Zoomed to 100% and 50% and back, the fit render is the fresh one, byte for
/// byte: the processor grown to full size and the sharpening kept from the
/// zoomed renders change nothing. With the RAW's own edit when it has a
/// sidecar, its masks left out. (Fit-to-window looking noisier after a zoom
/// was the interface keeping the 100% frame, src/argentum/zoomResolution.ts.)
#[test]
#[ignore = "reads RAW files and needs a GPU; run by hand"]
fn fit_after_zoom_is_a_fresh_fit() {
    const FIT: u32 = 2048;
    let ctx = device();
    for path in raws() {
        let photo = open(&path);
        let mut js = std::fs::read_to_string(format!("{path}.agdata"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map_or_else(|| json!({}), |sidecar| sidecar["adjustments"].clone());
        js["masks"] = json!([]);
        let (fw, fh) = photo.full.dimensions();
        let full_long = fw.max(fh);

        let fresh = {
            let small = GpuProcessor::new(ctx.clone(), (FIT + 255) & !255, (FIT + 255) & !255)
                .expect("processor");
            render_at(&ctx, &small, &photo, &js, FIT)
        };
        input_stage::reset();
        let grown = GpuProcessor::new(ctx.clone(), (fw + 255) & !255, (fh + 255) & !255)
            .expect("processor");
        render_at(&ctx, &grown, &photo, &js, full_long);
        render_at(&ctx, &grown, &photo, &js, full_long / 2);
        let back = render_at(&ctx, &grown, &photo, &js, FIT);
        let max = fresh
            .iter()
            .zip(&back)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        println!("{path}: fit after zoom vs fresh fit, max difference {max}");
        assert_eq!(fresh.len(), back.len());
        assert_eq!(max, 0, "{path}: the fit render after a zoom differs");
        input_stage::reset();
    }
}

/// Where Noise Reduction runs: in RapidRAW's shader after sharpening, as it did
/// until 2026-11-07, or in the input stage before it.
#[derive(Clone, Copy, PartialEq)]
enum NrOrder {
    AfterSharpening,
    BeforeSharpening,
}

/// One render of `photo` at `long` px, read back as RGBA8, with Noise
/// Reduction in `order`. `AfterSharpening` stages the edit with its Noise
/// Reduction taken out and hands RapidRAW's shader the edit as it is: the
/// render as it was made before the input stage ran Noise Reduction.
fn render_nr(
    ctx: &GpuContext,
    processor: &GpuProcessor,
    photo: &Photo,
    image: &DynamicImage,
    js: &Value,
    order: NrOrder,
) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let input = Bench::upload(ctx, image);
    let (gf, dehaze) = processor.build_guided_coeffs(&input, w, h, 1);
    let adjustments = crate::image_processing::get_all_adjustments_from_json(
        js,
        true,
        photo.shot,
        crate::image_processing::resolve_render_options(&AppSettings::default(), true),
        Some(&photo.path),
    );
    let request = |adjustments| RenderRequest {
        adjustments,
        mask_bitmaps: &[],
        lut: None,
        roi: None,
    };
    let at = input_stage::At {
        px_scale: (w as f32 / photo.full.width() as f32).min(1.0),
        measure_on: &photo.full,
        measure_key: 0,
        source: 0,
        family: 0,
    };
    let (staged, main) = match order {
        NrOrder::BeforeSharpening => {
            let (staged, _) =
                input_stage::develop(ctx, &input, w, h, request(adjustments), &at, Target::Fresh);
            (staged.view, staged.request)
        }
        NrOrder::AfterSharpening => {
            let mut without = adjustments;
            without.global.luma_noise_reduction = 0.0;
            without.global.color_noise_reduction = 0.0;
            let (staged, _) =
                input_stage::develop(ctx, &input, w, h, request(without), &at, Target::Fresh);
            (staged.view, request(adjustments))
        }
    };
    let (pixels, ..) = processor
        .run(
            staged.as_ref().unwrap_or(&input),
            &gf,
            &dehaze,
            w,
            h,
            main,
            false,
        )
        .expect("renders");
    pixels
}

/// Rec. 709 luma of RGBA8 pixels, 0 to 255.
fn luma8(pixels: &[u8]) -> Vec<f32> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect()
}

/// `luma` (`w` wide) averaged down to `tw` x `th`, each output pixel the mean
/// of the input pixels whose centres fall in it: the full render as it would
/// look at fit-to-screen.
fn shrink(luma: &[f32], w: u32, h: u32, tw: u32, th: u32) -> Vec<f32> {
    let mut sum = vec![0.0f64; (tw * th) as usize];
    let mut count = vec![0u32; (tw * th) as usize];
    for y in 0..h {
        let ty = ((y as f64 + 0.5) * th as f64 / h as f64) as u32;
        for x in 0..w {
            let tx = ((x as f64 + 0.5) * tw as f64 / w as f64) as u32;
            let i = (ty.min(th - 1) * tw + tx.min(tw - 1)) as usize;
            sum[i] += luma[(y * w + x) as usize] as f64;
            count[i] += 1;
        }
    }
    sum.iter()
        .zip(&count)
        .map(|(s, &c)| (s / c.max(1) as f64) as f32)
        .collect()
}

/// A square of `size` px at (x, y).
#[derive(Clone, Copy)]
struct Patch {
    x: u32,
    y: u32,
    size: u32,
}

impl Patch {
    fn values<'l>(&self, luma: &'l [f32], w: u32) -> impl Iterator<Item = f32> + 'l {
        let (x0, y0, s) = (self.x, self.y, self.size);
        (y0..y0 + s).flat_map(move |y| (x0..x0 + s).map(move |x| luma[(y * w + x) as usize]))
    }

    fn std(&self, luma: &[f32], w: u32) -> f32 {
        let n = (self.size * self.size) as f32;
        let mean = self.values(luma, w).sum::<f32>() / n;
        (self
            .values(luma, w)
            .map(|v| (v - mean).powi(2))
            .sum::<f32>()
            / n)
            .sqrt()
    }

    /// The steepest steps across edges: the 98th percentile of the larger of
    /// the horizontal and vertical neighbour differences.
    fn steepness(&self, luma: &[f32], w: u32) -> f32 {
        let mut steps = Vec::new();
        for y in self.y..self.y + self.size - 1 {
            for x in self.x..self.x + self.size - 1 {
                let i = (y * w + x) as usize;
                let dx = (luma[i + 1] - luma[i]).abs();
                let dy = (luma[i + w as usize] - luma[i]).abs();
                steps.push(dx.max(dy));
            }
        }
        steps.sort_by(f32::total_cmp);
        steps[steps.len() * 98 / 100]
    }

    /// How much picture there is, as against noise: the spread of its 4 x 4
    /// px block means.
    fn structure(&self, luma: &[f32], w: u32) -> f32 {
        let mut means = Vec::new();
        for by in (self.y..self.y + self.size).step_by(4) {
            for bx in (self.x..self.x + self.size).step_by(4) {
                let block = Patch {
                    x: bx,
                    y: by,
                    size: 4,
                };
                means.push(block.values(luma, w).sum::<f32>() / 16.0);
            }
        }
        let mean = means.iter().sum::<f32>() / means.len() as f32;
        (means.iter().map(|m| (m - mean).powi(2)).sum::<f32>() / means.len() as f32).sqrt()
    }
}

/// The flattest and the most detailed patches of `luma`: where noise is
/// measured, and where edges are.
fn patches(luma: &[f32], w: u32, h: u32, size: u32) -> (Vec<Patch>, Vec<Patch>) {
    let mut all: Vec<(f32, Patch)> = Vec::new();
    for y in (0..h - size).step_by(size as usize) {
        for x in (0..w - size).step_by(size as usize) {
            let p = Patch { x, y, size };
            let mean = p.values(luma, w).sum::<f32>() / (size * size) as f32;
            // Away from black and white, where clipping hides noise.
            if (40.0..=215.0).contains(&mean) {
                all.push((p.structure(luma, w), p));
            }
        }
    }
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    const N: usize = 24;
    let flat = all.iter().take(N).map(|(_, p)| *p).collect();
    let edges = all.iter().rev().take(N).map(|(_, p)| *p).collect();
    (flat, edges)
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let v: Vec<f32> = values.collect();
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

/// Noise Reduction before sharpening against after it, on high-ISO RAWs: the
/// noise on the flattest patches (luma std dev, 8-bit) and the steepness of
/// the most detailed ones, at fit-to-screen and at 100%, with each photo's
/// default edit and Luma Noise Reduction at 0, 30 and 60. At fit, also the
/// noise of the 100% render shrunk to fit: what the fit view should show.
/// At 0 the two orders must be the same render, byte for byte.
#[test]
#[ignore = "reads RAW files and needs a GPU; run by hand"]
fn noise_reduction_order() {
    const FIT: u32 = 1920;
    let ctx = device();
    for path in raws() {
        let photo = open(&path);
        let (fw, fh) = photo.full.dimensions();
        let fit_image = crate::image_processing::downscale_f32_image(&photo.full, FIT, FIT);
        let (tw, th) = fit_image.dimensions();
        let processor = GpuProcessor::new(ctx.clone(), (fw + 255) & !255, (fh + 255) & !255)
            .expect("processor");
        let mut fit_cells = None;
        let mut full_cells = None;
        for nr in [0, 30, 60] {
            let js = json!({ "lumaNoiseReduction": nr });
            let mut fulls = Vec::new();
            for order in [NrOrder::AfterSharpening, NrOrder::BeforeSharpening] {
                let fit = luma8(&render_nr(&ctx, &processor, &photo, &fit_image, &js, order));
                let full = luma8(&render_nr(
                    &ctx,
                    &processor,
                    &photo,
                    &photo.full,
                    &js,
                    order,
                ));
                let (fit_flat, fit_edges) = fit_cells
                    .get_or_insert_with(|| patches(&fit, tw, th, 16))
                    .clone();
                let (full_flat, full_edges) = full_cells
                    .get_or_insert_with(|| patches(&full, fw, fh, 32))
                    .clone();
                let shrunk = shrink(&full, fw, fh, tw, th);
                println!(
                    "{path} NR {nr:>2} {:<7} fit: noise {:.2} (100% shrunk {:.2}), steep {:.1} | 100%: noise {:.2}, steep {:.1}",
                    if order == NrOrder::AfterSharpening {
                        "after"
                    } else {
                        "before"
                    },
                    mean(fit_flat.iter().map(|p| p.std(&fit, tw))),
                    mean(fit_flat.iter().map(|p| p.std(&shrunk, tw))),
                    mean(fit_edges.iter().map(|p| p.steepness(&fit, tw))),
                    mean(full_flat.iter().map(|p| p.std(&full, fw))),
                    mean(full_edges.iter().map(|p| p.steepness(&full, fw))),
                );
                fulls.push((fit, full));
            }
            if nr == 0 {
                assert!(
                    fulls[0] == fulls[1],
                    "{path}: with Noise Reduction at 0 the order changed the render"
                );
            }
        }
        // What the pass costs at fit and at 100%, luma and colour on, their
        // amounts moving so it runs every time, each waited for on the GPU.
        for image in [&fit_image, &photo.full] {
            let (w, h) = image.dimensions();
            let input = Bench::upload(&ctx, image);
            let mut ticks = Vec::new();
            for i in 0..10 {
                let amount = 0.3 + 0.01 * i as f32;
                let job = super::denoise_gpu::Job {
                    src: &input,
                    source: 1,
                    family: 1,
                    width: w,
                    height: h,
                    is_raw: true,
                    luma: amount,
                    color: amount,
                    px_scale: (w as f32 / fw as f32).min(1.0),
                };
                wait(&ctx);
                let started = Instant::now();
                let _ = super::denoise_gpu::run(&ctx, &job, Target::Kept);
                wait(&ctx);
                if i >= 2 {
                    ticks.push(started.elapsed().as_secs_f32() * 1000.0);
                }
            }
            ticks.sort_by(f32::total_cmp);
            println!(
                "{path} {w}x{h}: the pass takes {:.1} ms (median), fastest {:.1} ms",
                ticks[ticks.len() / 2],
                ticks[0]
            );
        }
        input_stage::reset();
    }
}
