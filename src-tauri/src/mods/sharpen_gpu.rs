//! The GPU passes behind `sharpen.rs`. Ours.
//!
//! Takes the texture RapidRAW's shader is about to read, and returns a
//! sharpened copy of it - or `None` when there is nothing to do, in which case
//! their texture is used exactly as it was.
//!
//! TILES
//!
//! The image is walked in 1024 px tiles with a 64 px border, and every scratch
//! buffer is one bordered tile. RawTherapee does the same with 32 px tiles and
//! a border of 5 to 8, because Richardson-Lucy converges locally: twenty
//! iterations of a sigma-1 kernel do not carry an edge's influence anywhere
//! near 64 px. Tiling keeps the scratch at 32 MB whether the photo is 2 MP or
//! 100 MP; only the output is full size, and that is the texture their shader
//! would otherwise have read.
//!
//! WHEN IT RUNS
//!
//! Only when its inputs change. The result is kept with everything that went
//! into it - the source texture itself, the settings, the scale, the region -
//! and handed back as is while those match, so dragging Exposure costs nothing
//! here. Dragging a sharpening slider recomputes, and while their render has a
//! region of interest (zoomed in, mid-drag) only that region and a margin is
//! sharpened; the rest is copied across untouched.
//!
//! ON OPENGL
//!
//! RapidRAW offers OpenGL as a processing backend, and falls back to it on
//! its own after a crash during start-up. Two things differ there, both found
//! on AK's machine on 2026-10-10:
//!
//! - A texture with one layer cannot be viewed as an array, so every layered
//!   texture here has at least two (their mask array does the same).
//! - The GL context is one lock for the whole device, and wgpu panics in any
//!   thread that waits more than a second for it - "Could not lock adapter
//!   context. This is most-likely a deadlock." A blocking poll holds it until
//!   the GPU is done, so seconds of deconvolution queued at once meant the next
//!   thread to want the context crashed, and with it their preview worker.
//!   On GL the work goes in small submissions, each waited for without the
//!   lock held (`wait_politely`), so nobody waits behind more than one.
//!
//! WHICH TEXTURE IT WRITES
//!
//! `Target::Kept` reuses one texture between calls. That is only safe where
//! calls cannot overlap - inside `process_and_get_dynamic_image`, which holds
//! RapidRAW's processor lock for the whole render. Anything outside that lock
//! (the 16-bit export) asks for `Target::Fresh`, and gets a texture of its own
//! that dies with the render.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};

use super::sharpen::{FLAG_ITER_CHECK, Params};
use crate::image_processing::GpuContext;

const TILE: u32 = 1024;
const BORDER: u32 = 64;
const EXT: u32 = TILE + 2 * BORDER;
const STOP_BLOCK: u32 = 32;
const WORKGROUP: u32 = 16;

/// Their render reads up to 128 px past its region of interest (their
/// TILE_OVERLAP). Sharpening that far, plus a little, keeps every texel they
/// read a sharpened one.
const REGION_MARGIN: u32 = 160;

/// Below this sigma, in render pixels, a gaussian is a delta function to the
/// eye: capture sharpening at fit-to-screen. Skipped rather than computed.
const MIN_SIGMA: f32 = 0.25;

/// RawTherapee's clip mask level, 0.95 of white.
const CLIP_LEVEL: f32 = 0.95;

// Flags the shader reads. Matches the constants at the top of sharpen.wgsl.
const FLAG_CAPTURE: u32 = 1;
const FLAG_USM: u32 = 2;
const FLAG_MASK_VIEW: u32 = 4;
const FLAG_ITER: u32 = 8;
const FLAG_MASK: u32 = 16;
const FLAG_USM_MASK: u32 = 32;
const FLAG_CLIP: u32 = 64;

/// `Params` in sharpen.wgsl, field for field.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug, PartialEq)]
struct Uniform {
    img_w: u32,
    img_h: u32,
    origin_x: i32,
    origin_y: i32,
    ext_w: u32,
    ext_h: u32,
    inner_x0: u32,
    inner_y0: u32,
    inner_w: u32,
    inner_h: u32,
    flags: u32,
    is_raw: u32,
    contrast: f32,
    mask_sigma: f32,
    cap_sigma: f32,
    cap_slope: f32,
    cap_amount: f32,
    usm_sigma: f32,
    usm_amount: f32,
    usm_threshold: f32,
    centre_x: f32,
    centre_y: f32,
    clip: f32,
    usm_contrast: f32,
    local_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    local_amount: [[f32; 4]; 8],
}

/// One of RapidRAW's mask bitmaps, as a render carries them.
pub type MaskBitmap = image::ImageBuffer<image::Luma<u8>, Vec<u8>>;

/// One request to sharpen a texture.
pub struct Job<'a> {
    pub src: &'a wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub is_raw: bool,
    pub params: Params,
    /// Capture's contrast threshold, 0..1, already resolved from auto. 0
    /// sharpens everywhere.
    pub contrast: f32,
    /// The manual sharpen's: the same number unless it has its own mask.
    pub usm_contrast: f32,
    /// Render pixels per full-resolution pixel.
    pub px_scale: f32,
    /// Show a mask instead of sharpening.
    pub mask_view: MaskView,
    /// RapidRAW's mask bitmaps for this render, indexed as `params.local` is.
    pub masks: &'a [MaskBitmap],
    /// Their region of interest, in render pixels: x, y, width, height.
    pub region: Option<[u32; 4]>,
}

pub enum Target {
    Kept,
    Fresh,
}

/// Which mask the eye is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskView {
    Off,
    Capture,
    Sharpen,
}

/// What is going to run, worked out before anything touches the GPU.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Plan {
    flags: u32,
    iterations: u32,
    /// The first mask's threshold: capture's, or the manual sharpen's when
    /// capture is not running.
    contrast: f32,
    usm_contrast: f32,
    mask_sigma: f32,
    cap_sigma: f32,
    cap_slope: f32,
    cap_amount: f32,
    usm_sigma: f32,
    usm_amount: f32,
    usm_threshold: f32,
    /// The masks with a Sharpen of their own and their pixels, hashed: a
    /// brush stroke changes the result without changing any setting.
    local_key: u64,
    /// x0, y0, x1, y1 of the area to sharpen, render pixels.
    area: [u32; 4],
}

/// Whether anything would run - asked before the contrast threshold is
/// measured, since that is a pass over the whole photo.
pub fn needs_work(
    params: &Params,
    width: u32,
    height: u32,
    px_scale: f32,
    mask_view: MaskView,
) -> bool {
    plan_for(params, width, height, px_scale, mask_view, 0.0, 0.0, None).is_some()
}

fn plan(job: &Job) -> Option<Plan> {
    plan_for(
        &job.params,
        job.width,
        job.height,
        job.px_scale,
        job.mask_view,
        job.contrast,
        job.usm_contrast,
        job.region,
    )
    .map(|mut plan| {
        if plan.flags & FLAG_USM != 0 {
            plan.local_key = local_key(&local_layers(job));
        }
        plan
    })
}

/// The masks whose own Sharpen is not zero, with their bitmap - skipping any
/// whose bitmap is missing or not this render's size, which would mean the
/// indices no longer line up with theirs.
fn local_layers<'a>(job: &'a Job) -> Vec<(&'a MaskBitmap, f32)> {
    job.params
        .local
        .iter()
        .enumerate()
        .filter(|(_, a)| **a != 0.0)
        .filter_map(|(i, a)| {
            let bitmap = job.masks.get(i)?;
            (bitmap.width() == job.width && bitmap.height() == job.height).then_some((bitmap, *a))
        })
        .collect()
}

fn local_key(layers: &[(&MaskBitmap, f32)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (bitmap, amount) in layers {
        amount.to_bits().hash(&mut h);
        bitmap.as_raw().hash(&mut h);
    }
    h.finish()
}

#[allow(clippy::too_many_arguments)]
fn plan_for(
    p: &Params,
    width: u32,
    height: u32,
    px_scale: f32,
    mask_view: MaskView,
    contrast: f32,
    usm_contrast: f32,
    region: Option<[u32; 4]>,
) -> Option<Plan> {
    let showing = mask_view != MaskView::Off;
    let s = px_scale.clamp(1.0e-3, 1.0);
    let (w, h) = (width as f32, height as f32);

    // RawTherapee's corner boost: the radius at the corners is
    // min(2, radius + boost), reached linearly from the centre.
    let cap_sigma = p.capture_radius * s;
    let corner_sigma = (p.capture_radius + p.capture_corner).min(2.0) * s;
    let corner_distance = ((w * 0.5).powi(2) + (h * 0.5).powi(2)).sqrt().max(1.0);
    let cap_slope = (corner_sigma - cap_sigma) / corner_distance;
    let capture = !showing && p.capture_on() && cap_sigma.max(corner_sigma) >= MIN_SIGMA;

    let usm_sigma = p.usm_radius * s;
    // A mask's own Sharpen runs the same unsharp mask, at Sharpen's radius,
    // even with the global Amount at 0.
    let usm =
        !showing && (p.usm_on() || p.local_on()) && p.usm_radius > 0.0 && usm_sigma >= MIN_SIGMA;

    if !capture && !usm && !showing {
        return None;
    }

    // The mask the first passes build: capture's, with its clip guard, unless
    // the manual sharpen is all that runs or its mask is the one on show.
    let (first, clip) = match mask_view {
        MaskView::Capture => (contrast, true),
        MaskView::Sharpen => (usm_contrast, false),
        MaskView::Off if capture => (contrast, true),
        MaskView::Off => (usm_contrast, false),
    };

    let mut flags = 0;
    if capture {
        flags |= FLAG_CAPTURE;
        if p.flags & FLAG_ITER_CHECK != 0 {
            flags |= FLAG_ITER;
        }
    }
    if usm {
        flags |= FLAG_USM;
    }
    if showing {
        flags |= FLAG_MASK_VIEW;
    }
    if first > 0.0 {
        flags |= FLAG_MASK;
        if clip {
            flags |= FLAG_CLIP;
        }
    }
    // Both run and the manual sharpen has a different mask: build it once
    // capture is done with capture's.
    if capture && usm && usm_contrast != contrast {
        flags |= FLAG_USM_MASK;
    }

    let area = match region {
        Some([x, y, rw, rh]) => [
            x.saturating_sub(REGION_MARGIN),
            y.saturating_sub(REGION_MARGIN),
            (x + rw + REGION_MARGIN).min(width),
            (y + rh + REGION_MARGIN).min(height),
        ],
        None => [0, 0, width, height],
    };
    if area[2] <= area[0] || area[3] <= area[1] {
        return None;
    }

    Some(Plan {
        flags,
        iterations: if capture {
            p.capture_iterations.clamp(1, 100)
        } else {
            0
        },
        contrast: first,
        usm_contrast,
        // RawTherapee blurs its mask with sigma 2, at full resolution.
        mask_sigma: (2.0 * s).max(0.3),
        cap_sigma,
        cap_slope,
        cap_amount: p.capture_amount,
        usm_sigma,
        usm_amount: p.usm_amount,
        usm_threshold: p.usm_threshold,
        local_key: 0,
        area,
    })
}

struct Pipelines {
    prep: wgpu::ComputePipeline,
    usm_mask_prep: wgpu::ComputePipeline,
    mask_h: wgpu::ComputePipeline,
    mask_v: wgpu::ComputePipeline,
    tick: wgpu::ComputePipeline,
    rl_blur_est_h: wgpu::ComputePipeline,
    rl_ratio_v: wgpu::ComputePipeline,
    rl_blur_ratio_h: wgpu::ComputePipeline,
    rl_update_v: wgpu::ComputePipeline,
    capture_mix: wgpu::ComputePipeline,
    usm_prep: wgpu::ComputePipeline,
    usm_h: wgpu::ComputePipeline,
    usm_v: wgpu::ComputePipeline,
    compose: wgpu::ComputePipeline,
    passthrough: wgpu::ComputePipeline,
}

/// Everything the passes need, made once per device.
struct Engine {
    device: Arc<wgpu::Device>,
    /// OpenGL: small submissions, waited for without holding its context.
    gl: bool,
    layout: wgpu::BindGroupLayout,
    pipelines: Pipelines,
    uniform: wgpu::Buffer,
    scratch: [wgpu::Buffer; 5],
    stopped: wgpu::Buffer,
    counter: wgpu::Buffer,
    /// Bound when no mask has a Sharpen of its own.
    no_masks: wgpu::TextureView,
    kept: Option<Kept>,
}

/// The last result, and what it was made from.
struct Kept {
    src: wgpu::TextureView,
    size: (u32, u32),
    is_raw: bool,
    plan: Plan,
    view: wgpu::TextureView,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

pub const SHADER: &str = include_str!("../shaders/sharpen.wgsl");

impl Engine {
    fn new(device: &Arc<wgpu::Device>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Argentum Sharpen Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Argentum Sharpen BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Uniform>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                storage(3),
                storage(4),
                storage(5),
                storage(6),
                storage(7),
                storage(8),
                storage(9),
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Argentum Sharpen Layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let pipelines = Pipelines {
            prep: pipeline("prep"),
            usm_mask_prep: pipeline("usm_mask_prep"),
            mask_h: pipeline("mask_h"),
            mask_v: pipeline("mask_v"),
            tick: pipeline("tick"),
            rl_blur_est_h: pipeline("rl_blur_est_h"),
            rl_ratio_v: pipeline("rl_ratio_v"),
            rl_blur_ratio_h: pipeline("rl_blur_ratio_h"),
            rl_update_v: pipeline("rl_update_v"),
            capture_mix: pipeline("capture_mix"),
            usm_prep: pipeline("usm_prep"),
            usm_h: pipeline("usm_h"),
            usm_v: pipeline("usm_v"),
            compose: pipeline("compose"),
            passthrough: pipeline("passthrough"),
        };

        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let plane = (EXT as u64) * (EXT as u64) * 4;
        let rw = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let blocks = (EXT.div_ceil(STOP_BLOCK) as u64).pow(2) * 4;

        Engine {
            device: Arc::clone(device),
            gl: device.adapter_info().backend == wgpu::Backend::Gl,
            layout,
            pipelines,
            uniform: buffer(
                "Argentum Sharpen Params",
                std::mem::size_of::<Uniform>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            scratch: [
                buffer("Argentum Sharpen Y0", plane, rw),
                buffer("Argentum Sharpen Estimate", plane, rw),
                buffer("Argentum Sharpen Tmp", plane, rw),
                buffer("Argentum Sharpen Aux", plane, rw),
                buffer("Argentum Sharpen Blend", plane, rw),
            ],
            stopped: buffer("Argentum Sharpen Stopped", blocks, rw),
            counter: buffer("Argentum Sharpen Counter", 16, rw),
            no_masks: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Argentum Sharpen No Masks"),
                    // Two: on OpenGL a one-layer texture is not an array.
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 2,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                }),
            kept: None,
        }
    }

    /// The masks with a Sharpen of their own, as one layered texture, and
    /// each one's amount in the order of its layer.
    fn local_masks(
        &self,
        queue: &wgpu::Queue,
        job: &Job,
        plan: &Plan,
    ) -> (Option<wgpu::TextureView>, u32, [[f32; 4]; 8]) {
        let mut amounts = [[0.0f32; 4]; 8];
        if plan.flags & FLAG_USM == 0 {
            return (None, 0, amounts);
        }
        let layers = local_layers(job);
        if layers.is_empty() {
            return (None, 0, amounts);
        }
        let mut data = Vec::with_capacity(layers.len() * (job.width * job.height) as usize);
        for (k, (bitmap, amount)) in layers.iter().enumerate() {
            data.extend_from_slice(bitmap.as_raw());
            amounts[k / 4][k % 4] = *amount;
        }
        // Never one layer: on OpenGL that is a plain texture, not an array.
        // The extra layer is blank and local_count never reaches it.
        let depth = layers.len().max(2);
        data.resize(depth * (job.width * job.height) as usize, 0);
        use wgpu::util::DeviceExt;
        let texture = self.device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Argentum Sharpen Masks"),
                size: wgpu::Extent3d {
                    width: job.width,
                    height: job.height,
                    depth_or_array_layers: depth as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        (Some(view), layers.len() as u32, amounts)
    }

    fn output(&self, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Argentum Sharpened Input"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            // COPY_SRC so a test can read it back; nothing else copies it.
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        (texture, view)
    }

    fn encode(&self, queue: &wgpu::Queue, job: &Job, plan: &Plan, dst: &wgpu::TextureView) {
        let (masks, local_count, local_amount) = self.local_masks(queue, job, plan);
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(job.src),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(dst),
            },
        ];
        for (i, buffer) in self.scratch.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 3 + i as u32,
                resource: buffer.as_entire_binding(),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: self.stopped.as_entire_binding(),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 9,
            resource: self.counter.as_entire_binding(),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 10,
            resource: wgpu::BindingResource::TextureView(masks.as_ref().unwrap_or(&self.no_masks)),
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Argentum Sharpen BG"),
            layout: &self.layout,
            entries: &entries,
        });

        let base = Uniform {
            img_w: job.width,
            img_h: job.height,
            flags: plan.flags,
            is_raw: u32::from(job.is_raw),
            contrast: plan.contrast,
            usm_contrast: plan.usm_contrast,
            mask_sigma: plan.mask_sigma,
            cap_sigma: plan.cap_sigma,
            cap_slope: plan.cap_slope,
            cap_amount: plan.cap_amount,
            usm_sigma: plan.usm_sigma,
            usm_amount: plan.usm_amount,
            usm_threshold: plan.usm_threshold,
            centre_x: job.width as f32 * 0.5,
            centre_y: job.height as f32 * 0.5,
            clip: if job.is_raw { CLIP_LEVEL } else { 1.0e30 },
            local_count,
            local_amount,
            ..Default::default()
        };

        let [ax0, ay0, ax1, ay1] = plan.area;
        let whole = plan.area == [0, 0, job.width, job.height];
        if !whole {
            // Only part of the picture is sharpened: the rest has to be there,
            // unchanged, for their passes to read.
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&base));
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, &bind_group, &[]);
                pass.set_pipeline(&self.pipelines.passthrough);
                pass.dispatch_workgroups(
                    job.width.div_ceil(WORKGROUP),
                    job.height.div_ceil(WORKGROUP),
                    1,
                );
            }
            queue.submit(Some(encoder.finish()));
            if self.gl {
                wait_politely(&self.device, queue);
            }
        }

        let mut ty = ay0;
        while ty < ay1 {
            let th = TILE.min(ay1 - ty);
            let mut tx = ax0;
            while tx < ax1 {
                let tw = TILE.min(ax1 - tx);
                let ex0 = tx.saturating_sub(BORDER);
                let ey0 = ty.saturating_sub(BORDER);
                let ex1 = (tx + tw + BORDER).min(job.width);
                let ey1 = (ty + th + BORDER).min(job.height);
                let uniform = Uniform {
                    origin_x: ex0 as i32,
                    origin_y: ey0 as i32,
                    ext_w: ex1 - ex0,
                    ext_h: ey1 - ey0,
                    inner_x0: tx - ex0,
                    inner_y0: ty - ey0,
                    inner_w: tw,
                    inner_h: th,
                    ..base
                };
                self.tile(queue, &bind_group, &uniform, plan);
                tx += TILE;
            }
            ty += TILE;
        }
    }

    fn tile(&self, queue: &wgpu::Queue, bind_group: &wgpu::BindGroup, u: &Uniform, plan: &Plan) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(u));
        let whole = (u.ext_w.div_ceil(WORKGROUP), u.ext_h.div_ceil(WORKGROUP));
        let one = (1, 1);
        let p = &self.pipelines;

        // The tile's work as steps, each a few dispatches. On Vulkan and DX12
        // they all go in one submission; on OpenGL each step is its own.
        let mut steps: Vec<Vec<(&wgpu::ComputePipeline, (u32, u32))>> = Vec::new();
        let mut first = vec![(&p.prep, whole)];
        if plan.flags & FLAG_MASK != 0 {
            first.push((&p.mask_h, whole));
            first.push((&p.mask_v, whole));
        }
        steps.push(first);
        if plan.flags & FLAG_CAPTURE != 0 {
            for _ in 0..plan.iterations {
                steps.push(vec![
                    (&p.tick, one),
                    (&p.rl_blur_est_h, whole),
                    (&p.rl_ratio_v, whole),
                    (&p.rl_blur_ratio_h, whole),
                    (&p.rl_update_v, whole),
                ]);
            }
            steps.push(vec![(&p.capture_mix, whole)]);
        }
        if plan.flags & FLAG_USM_MASK != 0 {
            // Capture is done with its mask; the manual sharpen's replaces it.
            steps.push(vec![
                (&p.usm_mask_prep, whole),
                (&p.mask_h, whole),
                (&p.mask_v, whole),
            ]);
        }
        if plan.flags & FLAG_USM != 0 {
            steps.push(vec![
                (&p.usm_prep, whole),
                (&p.usm_h, whole),
                (&p.usm_v, whole),
            ]);
        }
        steps.push(vec![(&p.compose, whole)]);

        let chunks: Vec<Vec<_>> = if self.gl {
            steps
        } else {
            vec![steps.into_iter().flatten().collect()]
        };
        for (n, chunk) in chunks.iter().enumerate() {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Argentum Sharpen Tile"),
                });
            if n == 0 {
                encoder.clear_buffer(&self.stopped, 0, None);
                encoder.clear_buffer(&self.counter, 0, None);
            }
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("Argentum Sharpen"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, bind_group, &[]);
                for (pipeline, groups) in chunk {
                    pass.set_pipeline(pipeline);
                    pass.dispatch_workgroups(groups.0, groups.1, 1);
                }
            }
            queue.submit(Some(encoder.finish()));
            if self.gl {
                wait_politely(&self.device, queue);
            }
        }
    }
}

/// Wait for everything submitted so far, without holding OpenGL's context.
///
/// A blocking poll on GL keeps the one context lock for as long as the GPU
/// takes, and every other thread that wants it panics after a second. Asking
/// for a callback and polling without blocking takes the lock only for a
/// moment at a time. Gives up after thirty seconds rather than hang a render;
/// the work is still queued, and their own wait picks it up.
fn wait_politely(device: &wgpu::Device, queue: &wgpu::Queue) {
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    queue.on_submitted_work_done(move || flag.store(true, Ordering::Release));
    let started = Instant::now();
    while !done.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(30) {
        let _ = device.poll(wgpu::PollType::Poll);
        if !done.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

/// Forget everything kept, so the next run builds the engine afresh - after a
/// failure that may have left it half way through a job.
pub fn reset() {
    *ENGINE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Sharpen `job.src`, or say there is nothing to do.
pub fn run(context: &GpuContext, job: &Job, target: Target) -> Option<wgpu::TextureView> {
    let mut slot = ENGINE.lock().unwrap_or_else(|e| e.into_inner());

    let Some(plan) = plan(job) else {
        // Nothing to sharpen: let go of the last result, which is a
        // full-size texture, and of the input it was holding on to.
        if let Some(engine) = slot.as_mut() {
            engine.kept = None;
        }
        return None;
    };

    let device = &context.device;
    if slot
        .as_ref()
        .is_none_or(|e| !Arc::ptr_eq(&e.device, device))
    {
        *slot = Some(Engine::new(device));
    }
    let engine = slot.as_mut()?;

    if matches!(target, Target::Kept)
        && let Some(kept) = &engine.kept
        && kept.src == *job.src
        && kept.size == (job.width, job.height)
        && kept.is_raw == job.is_raw
        && kept.plan == plan
    {
        return Some(kept.view.clone());
    }

    let (_texture, view) = match target {
        // Reuse the kept texture when the size still fits; it is never being
        // read while this runs, because the caller holds the processor lock.
        Target::Kept => match engine.kept.take() {
            Some(kept) if kept.size == (job.width, job.height) => (None, kept.view),
            _ => {
                let (t, v) = engine.output(job.width, job.height);
                (Some(t), v)
            }
        },
        Target::Fresh => {
            let (t, v) = engine.output(job.width, job.height);
            (Some(t), v)
        }
    };

    let started = std::time::Instant::now();
    engine.encode(&context.queue, job, &plan, &view);
    log::debug!(
        "sharpen: {}x{} area {:?} flags {:#x} iterations {} encoded in {:?}",
        job.width,
        job.height,
        plan.area,
        plan.flags,
        plan.iterations,
        started.elapsed()
    );

    if matches!(target, Target::Kept) {
        engine.kept = Some(Kept {
            src: job.src.clone(),
            size: (job.width, job.height),
            is_raw: job.is_raw,
            plan,
            view: view.clone(),
        });
    }
    Some(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Params {
        super::super::sharpen::from_json(&serde_json::json!({}), true, None)
    }

    fn plan_at(params: &Params, scale: f32, mask_view: MaskView) -> Option<Plan> {
        plan_for(params, 6000, 4000, scale, mask_view, 0.1, 0.1, None)
    }

    #[test]
    fn the_uniform_matches_the_shader_struct() {
        // 28 four-byte fields in sharpen.wgsl's Params, then 8 vec4s.
        assert_eq!(std::mem::size_of::<Uniform>(), 112 + 128);
    }

    #[test]
    fn capture_sharpening_is_skipped_at_fit_to_screen_and_runs_at_100_percent() {
        let p = defaults();
        // 1920 px of a 6000 px photo: 0.75 * 0.32 = 0.24 px of blur, below
        // anything a screen pixel can show.
        assert!(plan_at(&p, 0.32, MaskView::Off).is_none());
        let full = plan_at(&p, 1.0, MaskView::Off).expect("runs at 1:1");
        assert_eq!(full.flags & FLAG_CAPTURE, FLAG_CAPTURE);
        assert_eq!(
            full.flags & FLAG_ITER,
            FLAG_ITER,
            "with RawTherapee's iteration check"
        );
        assert_eq!(full.iterations, 20);
        assert_eq!(full.cap_sigma, 0.75);
    }

    #[test]
    fn radii_are_scaled_to_the_render() {
        let mut p = defaults();
        p.usm_amount = 0.5;
        p.usm_radius = 2.0;
        let plan = plan_at(&p, 0.5, MaskView::Off).expect("unsharp mask at half size");
        assert_eq!(plan.usm_sigma, 1.0);
        assert_eq!(plan.mask_sigma, 1.0, "RawTherapee's mask blur of 2, halved");
    }

    #[test]
    fn corner_boost_reaches_its_radius_at_the_corner() {
        let mut p = defaults();
        p.capture_corner = 0.4;
        let plan = plan_at(&p, 1.0, MaskView::Off).unwrap();
        let corner = (3000.0f32.powi(2) + 2000.0f32.powi(2)).sqrt();
        assert!((plan.cap_sigma + plan.cap_slope * corner - 1.15).abs() < 1e-4);
        // And never past RawTherapee's ceiling of 2.
        p.capture_radius = 1.9;
        p.capture_corner = 0.5;
        let plan = plan_at(&p, 1.0, MaskView::Off).unwrap();
        assert!((plan.cap_sigma + plan.cap_slope * corner - 2.0).abs() < 1e-4);
    }

    #[test]
    fn the_mask_view_always_runs_and_sharpens_nothing() {
        let p = defaults();
        let plan = plan_at(&p, 0.32, MaskView::Capture).expect("the mask is shown at any zoom");
        assert_eq!(plan.flags & FLAG_MASK_VIEW, FLAG_MASK_VIEW);
        assert_eq!(plan.flags & (FLAG_CAPTURE | FLAG_USM), 0);
    }

    #[test]
    fn nothing_on_means_no_work() {
        assert!(plan_at(&Params::default(), 1.0, MaskView::Off).is_none());
    }

    #[test]
    fn a_region_is_widened_past_their_overlap_and_clamped() {
        let p = defaults();
        let plan = plan_for(
            &p,
            6000,
            4000,
            1.0,
            MaskView::Off,
            0.1,
            0.1,
            Some([100, 3900, 500, 100]),
        )
        .unwrap();
        assert_eq!(plan.area, [0, 3740, 760, 4000]);
    }

    #[test]
    fn the_manual_sharpen_gets_its_own_mask_only_when_it_differs() {
        let mut p = defaults();
        p.usm_amount = 0.5;
        let same = plan_for(&p, 6000, 4000, 1.0, MaskView::Off, 0.1, 0.1, None).unwrap();
        assert_eq!(same.flags & FLAG_USM_MASK, 0);
        let own = plan_for(&p, 6000, 4000, 1.0, MaskView::Off, 0.1, 0.4, None).unwrap();
        assert_eq!(own.flags & FLAG_USM_MASK, FLAG_USM_MASK);
        assert_eq!((own.contrast, own.usm_contrast), (0.1, 0.4));
        // Capture off: the manual sharpen's mask is the only one, built first,
        // and without capture's clip guard.
        p.capture_amount = 0.0;
        let alone = plan_for(&p, 6000, 4000, 1.0, MaskView::Off, 0.1, 0.4, None).unwrap();
        assert_eq!(alone.contrast, 0.4);
        assert_eq!(alone.flags & (FLAG_USM_MASK | FLAG_CLIP), 0);
    }

    #[test]
    fn a_masks_sharpen_runs_with_the_global_amount_at_zero() {
        let mut p = defaults();
        p.capture_amount = 0.0;
        assert!(plan_at(&p, 1.0, MaskView::Off).is_none(), "nothing on");
        p.local[3] = -0.5;
        let plan = plan_at(&p, 1.0, MaskView::Off).expect("a mask's own Sharpen runs");
        assert_eq!(plan.flags & FLAG_USM, FLAG_USM);
    }

    #[test]
    fn each_eye_shows_its_own_mask() {
        let p = defaults();
        let capture = plan_for(&p, 6000, 4000, 0.3, MaskView::Capture, 0.1, 0.4, None).unwrap();
        assert_eq!(capture.contrast, 0.1);
        assert_eq!(capture.flags & FLAG_CLIP, FLAG_CLIP);
        let sharpen = plan_for(&p, 6000, 4000, 0.3, MaskView::Sharpen, 0.1, 0.4, None).unwrap();
        assert_eq!(sharpen.contrast, 0.4);
        assert_eq!(sharpen.flags & FLAG_CLIP, 0);
    }

    #[test]
    fn the_shader_compiles() {
        let module = match wgpu::naga::front::wgsl::parse_str(SHADER) {
            Ok(module) => module,
            Err(e) => panic!(
                "sharpen.wgsl does not parse:
{}",
                e.emit_to_string(SHADER)
            ),
        };
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&module) {
            panic!(
                "sharpen.wgsl does not validate:
{}",
                e.emit_to_string(SHADER)
            );
        }
    }

    // ------------------------------------------------------------------
    // On a real device: cargo test --lib sharpen_gpu -- --ignored
    // ------------------------------------------------------------------

    fn device(fallback: bool) -> Option<GpuContext> {
        device_on(wgpu::Instance::default(), fallback)
    }

    /// The OpenGL backend, as RapidRAW selects it: WGPU_BACKEND=gl.
    fn gl_device() -> Option<GpuContext> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::GL;
        let ctx = device_on(wgpu::Instance::new(desc), false)?;
        assert_eq!(ctx.device.adapter_info().backend, wgpu::Backend::Gl);
        Some(ctx)
    }

    fn device_on(instance: wgpu::Instance, fallback: bool) -> Option<GpuContext> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: None,
        }))
        .ok()?;
        eprintln!("adapter: {:?}", adapter.get_info());
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sharpen test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        Some(GpuContext {
            device: Arc::new(device),
            queue: Arc::new(queue),
            limits,
            display: Arc::new(Mutex::new(None)),
        })
    }

    fn upload(ctx: &GpuContext, w: u32, h: u32, px: &[[f32; 4]]) -> wgpu::TextureView {
        use wgpu::util::DeviceExt;
        let data: Vec<half::f16> = px
            .iter()
            .flatten()
            .map(|&v| half::f16::from_f32(v))
            .collect();
        let texture = ctx.device.create_texture_with_data(
            &ctx.queue,
            &wgpu::TextureDescriptor {
                label: Some("sharpen test input"),
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
            bytemuck::cast_slice(&data),
        );
        texture.create_view(&Default::default())
    }

    fn read(ctx: &GpuContext, view: &wgpu::TextureView, w: u32, h: u32) -> Vec<[f32; 4]> {
        let row = (w * 8).div_ceil(256) * 256;
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sharpen test readback"),
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
            let line: &[half::f16] = bytemuck::cast_slice(&bytes[start..start + w as usize * 8]);
            for p in line.chunks(4) {
                out.push([p[0].to_f32(), p[1].to_f32(), p[2].to_f32(), p[3].to_f32()]);
            }
        }
        out
    }

    /// A grey vertical step from 0.05 to 0.4 at x = w / 2, blurred by a
    /// gaussian of `sigma`. Constant down every column.
    fn edge(w: u32, h: u32, sigma: f32) -> Vec<[f32; 4]> {
        let mut row = Vec::with_capacity(w as usize);
        for x in 0..w {
            let mut acc = 0.0f32;
            let mut wsum = 0.0f32;
            for k in -40..=40 {
                let d = k as f32 * 0.25;
                let weight = (-(d * d) / (2.0 * sigma * sigma)).exp();
                acc += weight
                    * if x as f32 + d >= (w / 2) as f32 {
                        0.4
                    } else {
                        0.05
                    };
                wsum += weight;
            }
            let v = acc / wsum;
            row.push([v, v, v, 1.0]);
        }
        (0..h).flat_map(|_| row.clone()).collect()
    }

    fn steepest(row: &[[f32; 4]]) -> f32 {
        row.windows(2)
            .map(|p| (p[1][1] - p[0][1]).abs())
            .fold(0.0, f32::max)
    }

    fn capture_job(src: &wgpu::TextureView, w: u32, h: u32, contrast: f32) -> Job<'_> {
        let mut params = defaults();
        params.capture_radius = 1.2;
        params.flags &= !FLAG_ITER_CHECK;
        Job {
            src,
            width: w,
            height: h,
            is_raw: true,
            params,
            contrast,
            usm_contrast: contrast,
            px_scale: 1.0,
            mask_view: MaskView::Off,
            masks: &[],
            region: None,
        }
    }

    fn deconvolution_steepens_the_edge(ctx: &GpuContext) {
        let (w, h) = (256, 64);
        let px = edge(w, h, 1.2);
        let src = upload(ctx, w, h, &px);
        let out = run(ctx, &capture_job(&src, w, h, 0.0), Target::Fresh).expect("work to do");
        let got = read(ctx, &out, w, h);
        let mid = (h / 2 * w) as usize;
        let before = steepest(&px[mid..mid + w as usize]);
        let after = steepest(&got[mid..mid + w as usize]);
        assert!(
            after > before * 1.3,
            "steepest step {before} before, {after} after"
        );
        // Far from the edge the picture is flat, and stays where it was.
        for x in [10usize, 60, 200, 245] {
            assert!(
                (got[mid + x][1] - px[mid + x][1]).abs() < 2e-3,
                "x = {x} moved"
            );
        }
        // Grey in, grey out: only the luminance moves.
        let c = got[mid + w as usize / 2];
        assert!((c[0] - c[1]).abs() < 1e-3 && (c[2] - c[1]).abs() < 1e-3);
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn capture_sharpening_steepens_a_blurred_edge() {
        let ctx = device(false).expect("no wgpu adapter");
        deconvolution_steepens_the_edge(&ctx);
    }

    /// The same on the software adapter Windows gives a machine with no
    /// graphics card (WARP). Every slider in the app already runs that way on
    /// such a machine; this is the proof that sharpening does too.
    #[test]
    #[ignore = "needs a GPU driver stack; run with --ignored"]
    fn capture_sharpening_runs_on_the_software_adapter() {
        let ctx =
            device(true).expect("no software adapter on this machine - nothing proven either way");
        deconvolution_steepens_the_edge(&ctx);
    }

    /// darktable's sharpen worked out here on the CPU from its own formula,
    /// and compared with the shader along a row.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_unsharp_mask_is_darktables() {
        let ctx = device(false).expect("no wgpu adapter");
        let (w, h) = (128u32, 16u32);
        let px = edge(w, h, 1.5);
        let src = upload(&ctx, w, h, &px);
        let params = Params {
            usm_amount: 0.8,
            usm_radius: 2.0,
            usm_threshold: 0.5,
            ..Params::default()
        };
        let job = Job {
            src: &src,
            width: w,
            height: h,
            is_raw: true,
            params,
            contrast: 0.0,
            usm_contrast: 0.0,
            px_scale: 1.0,
            mask_view: MaskView::Off,
            masks: &[],
            region: None,
        };
        let got = read(&ctx, &run(&ctx, &job, Target::Fresh).unwrap(), w, h);

        // L* of each pixel, a gaussian of sigma 2 reaching 2.5 sigma
        // (darktable's kernel), the thresholded detail, back to Y. The input
        // as the GPU saw it, through f16.
        let lstar = |y: f32| {
            let f = if y > 216.0 / 24389.0 {
                y.cbrt()
            } else {
                (24389.0 / 27.0 * y + 16.0) / 116.0
            };
            116.0 * f - 16.0
        };
        let y_of = |l: f32| {
            let f = (l + 16.0) / 116.0;
            if f.powi(3) > 216.0 / 24389.0 {
                f.powi(3)
            } else {
                l / (24389.0 / 27.0)
            }
        };
        let row: Vec<f32> = px[..w as usize]
            .iter()
            .map(|p| half::f16::from_f32(p[1]).to_f32())
            .collect();
        let luma = |v: f32| v * (0.212671 + 0.715160 + 0.072169);
        let l: Vec<f32> = row.iter().map(|&v| lstar(luma(v))).collect();
        let r = 5;
        let mid = (h / 2 * w) as usize;
        for x in 0..w as i32 {
            let (mut sum, mut wsum) = (0.0f32, 0.0f32);
            for k in -r..=r {
                let i = (x + k).clamp(0, w as i32 - 1) as usize;
                let weight = (-((k * k) as f32) / 8.0).exp();
                sum += l[i] * weight;
                wsum += weight;
            }
            let lx = l[x as usize];
            let delta = lx - sum / wsum;
            let detail = delta.signum() * (delta.abs() - 0.5).max(0.0);
            let expect = row[x as usize] * y_of(lx + 0.8 * detail) / luma(row[x as usize]);
            let actual = got[mid + x as usize][1];
            assert!(
                (actual - expect).abs() < 2e-3 + expect * 2e-3,
                "x = {x}: darktable gives {expect}, the shader {actual}"
            );
        }
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_mask_view_shows_edges_white_and_flat_black() {
        let ctx = device(false).expect("no wgpu adapter");
        let (w, h) = (256, 32);
        let px = edge(w, h, 1.0);
        let src = upload(&ctx, w, h, &px);
        let mut job = capture_job(&src, w, h, 0.1);
        job.mask_view = MaskView::Capture;
        job.px_scale = 0.3; // at any zoom
        let got = read(&ctx, &run(&ctx, &job, Target::Fresh).unwrap(), w, h);
        let mid = (h / 2 * w) as usize;
        assert!(
            got[mid + 20][0] < 0.02,
            "flat area should be black, was {}",
            got[mid + 20][0]
        );
        let at_edge = got[mid + w as usize / 2][0];
        assert!(at_edge > 0.9, "the edge should be white, was {at_edge}");
    }

    // Through RapidRAW's whole render, as an export makes it.

    fn render(ctx: &GpuContext, js: serde_json::Value, show: u32) -> Vec<f32> {
        let (w, h) = (256u32, 32u32);
        let px = edge(w, h, 1.2);
        let buf = image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_raw(
            w,
            h,
            px.iter().flatten().copied().collect(),
        )
        .unwrap();
        let mut all = crate::image_processing::get_all_adjustments_from_json(
            &js,
            true,
            crate::white_balance::WhiteBalance::reference(),
            None,
            None,
        );
        all.global.show_clipping = show;
        let out = crate::mods::export_precision::render_high_precision(
            ctx,
            &image::DynamicImage::ImageRgba32F(buf),
            crate::gpu_processing::RenderRequest {
                adjustments: all,
                mask_bitmaps: &[],
                lut: None,
                roi: None,
            },
        )
        .expect("renders");
        let img = out.as_rgba32f().expect("float").clone();
        (0..w).map(|x| img.get_pixel(x, h / 2)[1]).collect()
    }

    /// Their main shader reads the sharpened texture, and their GPU struct
    /// carries the settings to the stage, in the real pipeline.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_whole_render_comes_out_sharper() {
        let ctx = device(false).expect("no wgpu adapter");
        let off = render(
            &ctx,
            serde_json::json!({ "agSharpen": { "capture": false } }),
            0,
        );
        let on = render(
            &ctx,
            serde_json::json!({ "agSharpen": { "autoRadius": false, "radius": 1.2, "autoContrast": false, "contrast": 0 } }),
            0,
        );
        let steepest = |row: &[f32]| {
            row.windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0, f32::max)
        };
        assert!(
            steepest(&on) > steepest(&off) * 1.15,
            "after the tone curve: {} without, {} with",
            steepest(&off),
            steepest(&on)
        );
    }

    /// Mode 7 reaches the screen as the mask itself, past the tone curve.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_mask_view_survives_the_whole_render() {
        let ctx = device(false).expect("no wgpu adapter");
        let row = render(
            &ctx,
            serde_json::json!({ "agSharpen": { "autoContrast": false, "contrast": 10 } }),
            crate::mods::clipping::SHARPEN_MASK,
        );
        assert!(row[20] < 0.02, "flat should show black, showed {}", row[20]);
        assert!(
            row[128] > 0.9,
            "the edge should show white, showed {}",
            row[128]
        );
    }

    /// Every backend RapidRAW's Settings offers that this machine has -
    /// Vulkan, DirectX 12 (hardware, and WARP, which is what DirectX gives a
    /// machine with no graphics card) and OpenGL - through the same checks:
    /// deconvolution, a single mask's own Sharpen, the mask view through the
    /// whole render, and a second thread on the GPU meanwhile. Metal is the
    /// fifth option and exists only on a Mac.
    #[test]
    #[ignore = "needs GPUs; run with --ignored"]
    fn every_backend_we_offer() {
        let on = |backends: wgpu::Backends, fallback: bool| {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            desc.backends = backends;
            device_on(wgpu::Instance::new(desc), fallback)
        };
        let candidates = [
            ("Vulkan", on(wgpu::Backends::VULKAN, false)),
            ("DirectX 12", on(wgpu::Backends::DX12, false)),
            ("DirectX 12 WARP", on(wgpu::Backends::DX12, true)),
            ("OpenGL", on(wgpu::Backends::GL, false)),
        ];
        let mut ran = Vec::new();
        for (name, ctx) in candidates {
            let Some(ctx) = ctx else {
                eprintln!("{name}: not on this machine");
                continue;
            };
            let info = ctx.device.adapter_info();
            eprintln!("{name}: {} ({:?})", info.name, info.backend);
            all_checks(&ctx);
            ran.push(name);
        }
        eprintln!("passed on: {}", ran.join(", "));
        assert!(!ran.is_empty());
    }

    fn all_checks(ctx: &GpuContext) {
        deconvolution_steepens_the_edge(ctx);

        let (w, h) = (128u32, 32u32);
        let px = edge(w, h, 1.5);
        let src = upload(ctx, w, h, &px);
        let mask = MaskBitmap::from_pixel(w, h, image::Luma([255]));
        let mut local = [0.0; super::super::sharpen::MAX_LOCAL];
        local[0] = 0.8;
        let job = Job {
            src: &src,
            width: w,
            height: h,
            is_raw: true,
            params: Params {
                usm_radius: 2.0,
                usm_threshold: 0.5,
                local,
                ..Params::default()
            },
            contrast: 0.0,
            usm_contrast: 0.0,
            px_scale: 1.0,
            mask_view: MaskView::Off,
            masks: std::slice::from_ref(&mask),
            region: None,
        };
        let got = read(ctx, &run(ctx, &job, Target::Fresh).unwrap(), w, h);
        let mid = (h / 2 * w) as usize;
        assert!(
            steepest(&got[mid..mid + w as usize]) > steepest(&px[mid..mid + w as usize]) * 1.1,
            "a mask's own Sharpen"
        );

        let off = render(
            ctx,
            serde_json::json!({ "agSharpen": { "capture": false } }),
            0,
        );
        let on = render(
            ctx,
            serde_json::json!({ "agSharpen": { "autoRadius": false, "radius": 1.2, "autoContrast": false, "contrast": 0 } }),
            0,
        );
        let steep = |row: &[f32]| {
            row.windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0, f32::max)
        };
        assert!(
            steep(&on) > steep(&off) * 1.15,
            "the whole render comes out sharper"
        );
        let row = render(
            ctx,
            serde_json::json!({ "agSharpen": { "autoContrast": false, "contrast": 10 } }),
            crate::mods::clipping::SHARPEN_MASK,
        );
        assert!(
            row[20] < 0.02 && row[128] > 0.9,
            "the mask view through the whole render"
        );

        // Another thread on the GPU while a photo-sized job runs.
        let (w, h) = (3000u32, 2000u32);
        let px = edge(w, h, 1.2);
        let src = upload(ctx, w, h, &px);
        let stop = Arc::new(AtomicBool::new(false));
        let other = {
            let ctx = ctx.clone();
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    ctx.queue.submit(None);
                    let _ = ctx.device.poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(60)),
                    });
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        let job = capture_job(&src, w, h, 0.0);
        let _ = read(ctx, &run(ctx, &job, Target::Fresh).unwrap(), 16, 16);
        stop.store(true, Ordering::Release);
        other.join().expect("the other thread must not panic");
    }

    /// Everything the engine does, on OpenGL: deconvolution, a mask's own
    /// Sharpen with a single mask (one layer, which GL cannot view as an
    /// array unless padded), the mask view, and the whole render.
    #[test]
    #[ignore = "needs a GPU with OpenGL; run with --ignored"]
    fn everything_runs_on_opengl() {
        let ctx = gl_device().expect("no OpenGL adapter");
        deconvolution_steepens_the_edge(&ctx);

        let (w, h) = (128u32, 32u32);
        let px = edge(w, h, 1.5);
        let src = upload(&ctx, w, h, &px);
        let mask = MaskBitmap::from_pixel(w, h, image::Luma([255]));
        let mut local = [0.0; super::super::sharpen::MAX_LOCAL];
        local[0] = 0.8;
        let job = Job {
            src: &src,
            width: w,
            height: h,
            is_raw: true,
            params: Params {
                usm_radius: 2.0,
                usm_threshold: 0.5,
                local,
                ..Params::default()
            },
            contrast: 0.0,
            usm_contrast: 0.0,
            px_scale: 1.0,
            mask_view: MaskView::Off,
            masks: std::slice::from_ref(&mask),
            region: None,
        };
        let got = read(&ctx, &run(&ctx, &job, Target::Fresh).unwrap(), w, h);
        let mid = (h / 2 * w) as usize;
        assert!(steepest(&got[mid..mid + w as usize]) > steepest(&px[mid..mid + w as usize]) * 1.1);

        let row = render(
            &ctx,
            serde_json::json!({ "agSharpen": { "autoContrast": false, "contrast": 10 } }),
            crate::mods::clipping::SHARPEN_MASK,
        );
        assert!(row[20] < 0.02 && row[128] > 0.9, "the mask view on GL");
    }

    /// What crashed the app on AK's machine: another thread wanting the GL
    /// context while sharpening ran. Their readback polls and waits, which
    /// holds the context until the GPU is done; behind a whole photo's
    /// deconvolution that was seconds, and wgpu panics after one.
    #[test]
    #[ignore = "needs a GPU with OpenGL; run with --ignored"]
    fn opengl_survives_another_thread_using_the_gpu() {
        let ctx = gl_device().expect("no OpenGL adapter");
        // A 24 MP photo at 100%: on this machine about two seconds of
        // deconvolution, twice what GL lets another thread wait.
        let (w, h) = (6000u32, 4000u32);
        let px = edge(w, h, 1.2);
        let src = upload(&ctx, w, h, &px);

        let stop = Arc::new(AtomicBool::new(false));
        let other = {
            let ctx = ctx.clone();
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("the other thread"),
                    size: 256,
                    usage: wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut rounds = 0;
                while !stop.load(Ordering::Acquire) {
                    ctx.queue.write_buffer(&buffer, 0, &[0u8; 256]);
                    ctx.queue.submit(None);
                    let _ = ctx.device.poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(60)),
                    });
                    rounds += 1;
                    std::thread::sleep(Duration::from_millis(5));
                }
                rounds
            })
        };

        let mut job = capture_job(&src, w, h, 0.0);
        job.params.capture_iterations = 40;
        let started = Instant::now();
        let out = run(&ctx, &job, Target::Fresh).expect("work to do");
        let _ = read(&ctx, &out, 16, 16);
        eprintln!(
            "sharpened {w}x{h}, 40 iterations, on GL in {:?}",
            started.elapsed()
        );

        stop.store(true, Ordering::Release);
        let rounds = other.join().expect("the other thread must not panic");
        assert!(rounds > 0);
    }

    /// Where nothing is sharpened the picture comes back bit for bit.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn a_pass_that_moves_nothing_changes_nothing() {
        let ctx = device(false).expect("no wgpu adapter");
        let (w, h) = (64u32, 8u32);
        let px: Vec<[f32; 4]> = (0..w * h).map(|_| [0.05, 0.05, 0.05, 1.0]).collect();
        let src = upload(&ctx, w, h, &px);
        let params = Params {
            usm_amount: 1.0,
            usm_radius: 2.0,
            usm_threshold: 0.5,
            ..Params::default()
        };
        let job = Job {
            src: &src,
            width: w,
            height: h,
            is_raw: true,
            params,
            contrast: 0.0,
            usm_contrast: 0.0,
            px_scale: 1.0,
            mask_view: MaskView::Off,
            masks: &[],
            region: None,
        };
        let got = read(&ctx, &run(&ctx, &job, Target::Fresh).unwrap(), w, h);
        assert_eq!(
            half::f16::from_f32(got[100][1]),
            half::f16::from_f32(0.05),
            "got {:?}",
            got[100]
        );
    }

    /// A mask's own Sharpen lands inside the mask and nowhere else, and a
    /// negative one softens.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn a_masks_sharpen_stays_inside_the_mask() {
        let ctx = device(false).expect("no wgpu adapter");
        let (w, h) = (256u32, 64u32);
        let px = edge(w, h, 1.5);
        let src = upload(&ctx, w, h, &px);
        // The top half of the picture.
        let top = MaskBitmap::from_fn(w, h, |_, y| image::Luma([if y < h / 2 { 255 } else { 0 }]));
        let masks = [top];
        let run_with = |amount: f32| {
            let mut local = [0.0; super::super::sharpen::MAX_LOCAL];
            local[0] = amount;
            let params = Params {
                usm_radius: 2.0,
                usm_threshold: 0.5,
                local,
                ..Params::default()
            };
            let job = Job {
                src: &src,
                width: w,
                height: h,
                is_raw: true,
                params,
                contrast: 0.0,
                usm_contrast: 0.0,
                px_scale: 1.0,
                mask_view: MaskView::Off,
                masks: &masks,
                region: None,
            };
            read(
                &ctx,
                &run(&ctx, &job, Target::Fresh).expect("work to do"),
                w,
                h,
            )
        };
        let row = |img: &[[f32; 4]], y: u32| img[(y * w) as usize..((y + 1) * w) as usize].to_vec();
        let original = steepest(&row(&px, 8));

        let sharper = run_with(0.8);
        assert!(
            steepest(&row(&sharper, 8)) > original * 1.1,
            "inside the mask: sharper"
        );
        assert_eq!(
            row(&sharper, 56)
                .iter()
                .map(|p| half::f16::from_f32(p[1]))
                .collect::<Vec<_>>(),
            row(&px, 56)
                .iter()
                .map(|p| half::f16::from_f32(p[1]))
                .collect::<Vec<_>>(),
            "outside the mask: untouched"
        );

        let softer = run_with(-0.8);
        assert!(
            steepest(&row(&softer, 8)) < original * 0.9,
            "a negative amount softens"
        );
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn a_region_leaves_the_rest_of_the_picture_alone() {
        let ctx = device(false).expect("no wgpu adapter");
        let (w, h) = (1400, 64);
        let px = edge(w, h, 1.2);
        let src = upload(&ctx, w, h, &px);
        // Only the right end, which is nowhere near the edge at x = 700.
        let mut job = capture_job(&src, w, h, 0.0);
        job.region = Some([1300, 0, 100, 64]);
        let got = read(&ctx, &run(&ctx, &job, Target::Fresh).unwrap(), w, h);
        let mid = (h / 2 * w) as usize;
        for x in 690..710usize {
            assert_eq!(
                half::f16::from_f32(got[mid + x][1]),
                half::f16::from_f32(px[mid + x][1]),
                "x = {x} was sharpened outside the region"
            );
        }
    }
}
