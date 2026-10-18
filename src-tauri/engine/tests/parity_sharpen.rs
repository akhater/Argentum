//! Pass test 3b: `rt_capture_sharpen` against the app's capture sharpening.
//!
//! The app's side is its own files - `mods/sharpen.rs`, `mods/sharpen_gpu.rs`
//! and `shaders/sharpen.wgsl` - compiled unchanged by the reference crate.
//!
//! - The automatic radius and contrast threshold are CPU code copied as it
//!   was: bit-identical.
//! - The GPU passes are the same arithmetic. The app reads and writes half
//!   floats; the engine keeps f32. Given the same input - values a half float
//!   holds exactly - every channel of every pixel has to agree to within one
//!   half-float step of the app's output. That is the stated tolerance.

mod common;

use std::sync::Arc;

use argentum_engine::tools::ag_dng_standard::{self, AgDngStandard};
use argentum_engine::tools::rt_capture_sharpen::{self, RtCaptureSharpen};
use argentum_engine::{Context, Gpu, GpuSlot, Image, Pipeline, Rgb, Size, Source, Tool};
use argentum_engine_reference::image_processing::GpuContext;
use argentum_engine_reference::sharpen as today;
use argentum_engine_reference::sharpen_gpu as today_gpu;
use half::f16;

/// The developed picture, before any sharpening, as the tool receives it:
/// the tools after it at the photo's own defaults, which change nothing, and
/// DNG Standard off.
fn developed(source: &Source) -> Rgb {
    let mut p = Pipeline::standard().unwrap();
    p.set_edit(&p.registry().defaults_for(source)).unwrap();
    p.set::<RtCaptureSharpen>(rt_capture_sharpen::Settings {
        capture: false,
        ..Default::default()
    })
    .unwrap();
    p.set::<AgDngStandard>(ag_dng_standard::Settings {
        enabled: false,
        ..Default::default()
    })
    .unwrap();
    p.render(source, Size::Full)
        .unwrap()
        .as_rgb()
        .unwrap()
        .clone()
}

fn as_image(rgb: &Rgb) -> image::DynamicImage {
    let flat: Vec<f32> = rgb.pixels.iter().flatten().copied().collect();
    image::DynamicImage::ImageRgb32F(
        image::ImageBuffer::from_raw(rgb.width as u32, rgb.height as u32, flat).unwrap(),
    )
}

#[test]
fn the_automatic_radius_is_the_apps() {
    let mut cases = vec![("synthetic Bayer".to_string(), common::source(640, 480))];
    for path in common::raws("the_automatic_radius_is_the_apps") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        cases.push((name, common::open(&path)));
    }
    eprintln!();
    for (name, source) in cases {
        let key = format!("parity://{name}");
        today::record_raw(&source.raw, Some(&key));
        let theirs = today::auto_radius(&key);
        let ours = rt_capture_sharpen::measure_radius(&source.raw);
        eprintln!("  {name:<52} radius ours {ours:?} app {theirs:?}");
        assert_eq!(
            ours.map(f32::to_bits),
            theirs.map(f32::to_bits),
            "{name}: ours {ours:?}, the app's {theirs:?}"
        );
    }
}

#[test]
fn the_automatic_contrast_is_the_apps() {
    let mut cases = vec![(
        "synthetic Bayer".to_string(),
        developed(&common::source(640, 480)),
    )];
    for path in common::raws("the_automatic_contrast_is_the_apps") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        cases.push((name, developed(&common::open(&path))));
    }
    let params = today::from_json(&serde_json::json!({}), true, None);
    assert!(params.auto_contrast(), "the app's default is automatic");
    eprintln!();
    for (i, (name, rgb)) in cases.iter().enumerate() {
        let theirs = today::thresholds(&params, &as_image(rgb), i as u64, true)
            .measured
            .expect("measured");
        let ours = rt_capture_sharpen::measure_contrast(rgb);
        eprintln!("  {name:<52} threshold ours {ours} app {theirs}");
        assert_eq!(ours.to_bits(), theirs.to_bits(), "{name}");
    }
}

/// One way of sharpening, as both sides are told it.
#[derive(Clone, Copy, Debug)]
struct Case {
    radius: f32,
    corner: f32,
    iterations: u32,
    iter_check: bool,
    /// 0..1. 0 sharpens everywhere.
    contrast: f32,
    amount: f32,
}

impl Case {
    fn ours(&self) -> rt_capture_sharpen::Settings {
        rt_capture_sharpen::Settings {
            capture: true,
            capture_amount: self.amount * 100.0,
            auto_radius: false,
            radius: self.radius,
            corner_boost: self.corner,
            iterations: self.iterations,
            iter_check: self.iter_check,
            auto_contrast: false,
            contrast: self.contrast * 100.0,
        }
    }

    fn theirs(&self) -> today::Params {
        today::Params {
            capture_amount: self.amount,
            capture_radius: self.radius,
            capture_corner: self.corner,
            capture_iterations: self.iterations,
            flags: if self.iter_check {
                today::FLAG_ITER_CHECK
            } else {
                0
            },
            ..Default::default()
        }
    }
}

/// The input rounded to what a half float holds, so both sides start from
/// the same values.
fn halved(rgb: &Rgb) -> Rgb {
    rgb.with_pixels(
        rgb.pixels
            .iter()
            .map(|p| p.map(|v| f16::from_f32(v).to_f32()))
            .collect(),
    )
}

fn ours(gpu: &Arc<Gpu>, source: &Source, rgb: &Rgb, case: &Case) -> Vec<[f32; 3]> {
    let slot = GpuSlot::with(gpu.clone());
    let cx = Context::new::<RtCaptureSharpen>(source, 7, &slot);
    match RtCaptureSharpen::default()
        .process(&case.ours(), &Image::Rgb(rgb.clone()), &cx)
        .unwrap()
    {
        Some(Image::Rgb(out)) => out.pixels,
        _ => panic!("nothing sharpened"),
    }
}

fn theirs(ctx: &GpuContext, rgb: &Rgb, case: &Case) -> Vec<[f32; 3]> {
    let (w, h) = (rgb.width as u32, rgb.height as u32);
    let src = upload(ctx, rgb);
    let job = today_gpu::Job {
        src: &src,
        source: 0,
        family: 0,
        width: w,
        height: h,
        is_raw: true,
        params: case.theirs(),
        contrast: case.contrast,
        usm_contrast: case.contrast,
        local_contrast: [0.0; today::MAX_LOCAL],
        px_scale: 1.0,
        mask_view: today_gpu::MaskView::Off,
        masks: &[],
        region: None,
    };
    let out = today_gpu::run(ctx, &job, today_gpu::Target::Fresh).expect("work to do");
    read(ctx, &out, w, h)
}

/// Half-float steps between `a` and `b` at `b`'s size: 0 is the same half
/// float, 1 is the next one.
fn steps(a: f32, b: f32) -> f32 {
    let step = if b.abs() < 6.1e-5 {
        2f32.powi(-24)
    } else {
        2f32.powi(b.abs().log2().floor() as i32 - 10)
    };
    (a - b).abs() / step
}

/// The two half floats either side of `v` (the same one twice when `v` is
/// one). Every value here is zero or positive.
fn bracket(v: f32) -> (f32, f32) {
    let h = f16::from_f32(v);
    let next = |h: f16, up: bool| {
        f16::from_bits(if up { h.to_bits() + 1 } else { h.to_bits() - 1 }).to_f32()
    };
    if h.to_f32() == v {
        (v, v)
    } else if h.to_f32() > v {
        (next(h, false), h.to_f32())
    } else {
        (h.to_f32(), next(h, true))
    }
}

/// How the two agree: the largest difference in half-float steps, and the
/// share of values where the app's is one of the two half floats either side
/// of ours - that is, ours stored as a half float, rounded one way or the
/// other.
fn agreement(a: &[[f32; 3]], b: &[[f32; 3]]) -> (f32, f64) {
    let mut worst = 0.0f32;
    let mut stored = 0usize;
    for (p, q) in a.iter().zip(b) {
        for c in 0..3 {
            worst = worst.max(steps(p[c], q[c]));
            let (lo, hi) = bracket(p[c]);
            if q[c] == lo || q[c] == hi {
                stored += 1;
            }
        }
    }
    (worst, stored as f64 / (a.len() * 3) as f64)
}

/// Pixels the sharpening moved, so a run that did nothing cannot pass.
fn moved(before: &Rgb, after: &[[f32; 3]]) -> usize {
    before
        .pixels
        .iter()
        .zip(after)
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
fn the_gpu_passes_agree_with_the_apps_within_a_half_float_step() {
    let Some(gpu) = common::gpu("the_gpu_passes_agree_with_the_apps_within_a_half_float_step")
    else {
        return;
    };
    let ctx = GpuContext {
        device: Arc::new(gpu.device.clone()),
        queue: Arc::new(gpu.queue.clone()),
    };

    let defaults = Case {
        radius: rt_capture_sharpen::DEFAULT_RADIUS,
        corner: 0.0,
        iterations: rt_capture_sharpen::DEFAULT_ITERATIONS,
        iter_check: true,
        contrast: 0.1,
        amount: 1.0,
    };
    let mut inputs: Vec<(String, Source, Vec<Case>)> = vec![(
        "synthetic Bayer".into(),
        common::source(640, 480),
        vec![
            defaults,
            Case {
                radius: 1.2,
                contrast: 0.0,
                iter_check: false,
                ..defaults
            },
            Case {
                corner: 0.4,
                amount: 0.6,
                iterations: 35,
                ..defaults
            },
        ],
    )];
    for path in common::raws("the_gpu_passes_agree_with_the_apps_within_a_half_float_step") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = common::open(&path);
        // As the app resolves its defaults for this photo.
        let key = format!("parity-gpu://{name}");
        today::record_raw(&source.raw, Some(&key));
        let radius = today::auto_radius(&key).unwrap_or(rt_capture_sharpen::DEFAULT_RADIUS);
        let rgb = developed(&source);
        let params = today::from_json(&serde_json::json!({}), true, None);
        let contrast = today::thresholds(&params, &as_image(&rgb), 99, true).capture;
        inputs.push((
            name,
            source,
            vec![Case {
                radius,
                contrast,
                ..defaults
            }],
        ));
    }

    eprintln!();
    for (name, source, cases) in inputs {
        let rgb = halved(&developed(&source));
        for case in cases {
            let a = ours(&gpu, &source, &rgb, &case);
            let b = theirs(&ctx, &rgb, &case);
            let (worst, stored) = agreement(&a, &b);
            let n = moved(&rgb, &a);
            eprintln!(
                "  {name:<46} radius {:.3} contrast {:.3} corner {} iter {}: \
                 {n} of {} pixels sharpened; worst {worst:.4} half-float steps; \
                 the app's value is ours rounded to a half float, one way or the \
                 other, for {:.4}% of values",
                case.radius,
                case.contrast,
                case.corner,
                case.iterations,
                a.len(),
                stored * 100.0
            );
            assert!(n > 0, "{name} {case:?}: nothing was sharpened");
            assert!(
                worst <= 1.0,
                "{name} {case:?}: {worst} half-float steps from the app"
            );
        }
    }
}

// The app's own test helpers for its sharpening, from
// src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d (`upload`, `read`).

fn upload(ctx: &GpuContext, rgb: &Rgb) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let data: Vec<f16> = rgb
        .pixels
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 1.0])
        .map(f16::from_f32)
        .collect();
    let texture = ctx.device.create_texture_with_data(
        &ctx.queue,
        &wgpu::TextureDescriptor {
            label: Some("parity input"),
            size: wgpu::Extent3d {
                width: rgb.width as u32,
                height: rgb.height as u32,
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
        bytemuck::cast_slice(&data),
    );
    texture.create_view(&Default::default())
}

fn read(ctx: &GpuContext, view: &wgpu::TextureView, w: u32, h: u32) -> Vec<[f32; 3]> {
    let row = (w * 8).div_ceil(256) * 256;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("parity readback"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        view.texture().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    ctx.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(120)),
        })
        .expect("poll");
    let bytes = slice.get_mapped_range().to_vec();
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        let line: &[f16] = bytemuck::cast_slice(&bytes[start..start + w as usize * 8]);
        for p in line.chunks(4) {
            out.push([p[0].to_f32(), p[1].to_f32(), p[2].to_f32()]);
        }
    }
    out
}
