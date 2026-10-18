//! The passes of capture sharpening on the GPU, a tile at a time: the
//! contrast mask, the Richardson-Lucy iterations, and the factor each pixel's
//! luminance moved by. Copied from src-tauri/src/mods/sharpen_gpu.rs
//! @ 2bcb953d, capture sharpening only; the shader is `passes.wgsl`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};

use super::{MAX_RADIUS_PX, Resolved, RtCaptureSharpen};
use crate::{Error, Gpu, Result, Rgb, Tool};
const TILE: u32 = 1024;
const BORDER: u32 = 64;
const EXT: u32 = TILE + 2 * BORDER;
const STOP_BLOCK: u32 = 32;
const WORKGROUP: u32 = 16;

/// Below this sigma, in render pixels, a gaussian is a delta function to the
/// eye: capture sharpening at fit-to-screen. Skipped rather than computed.
const MIN_SIGMA: f32 = 0.25;

/// RawTherapee's clip mask level, 0.95 of white.
const CLIP_LEVEL: f32 = 0.95;

// Flags the shader reads. Match the constants at the top of the .wgsl, and
// the app's values for the same flags.
const FLAG_CAPTURE: u32 = 1;
const FLAG_ITER_CHECK: u32 = 8;
const FLAG_MASK: u32 = 16;
const FLAG_CLIP: u32 = 64;

const SHADER: &str = include_str!("passes.wgsl");

/// `Params` in the shader, field for field.
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
    contrast: f32,
    mask_sigma: f32,
    cap_sigma: f32,
    cap_slope: f32,
    cap_amount: f32,
    centre_x: f32,
    centre_y: f32,
    clip: f32,
    _pad0: u32,
}

/// What is going to run, worked out before anything touches the GPU.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Plan {
    flags: u32,
    iterations: u32,
    contrast: f32,
    mask_sigma: f32,
    cap_sigma: f32,
    cap_slope: f32,
    cap_amount: f32,
}

// Copied from src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d (`plan_for`), the
// capture sharpening half.
pub(super) fn plan(p: &Resolved, width: usize, height: usize, px_scale: f32) -> Option<Plan> {
    let s = px_scale.clamp(1.0e-3, 1.0);
    let (w, h) = (width as f32, height as f32);

    // RawTherapee's corner boost: the radius at the corners is
    // min(2, radius + boost), reached linearly from the centre.
    let cap_sigma = p.radius * s;
    let corner_sigma = (p.radius + p.corner).min(MAX_RADIUS_PX) * s;
    let corner_distance = ((w * 0.5).powi(2) + (h * 0.5).powi(2)).sqrt().max(1.0);
    let cap_slope = (corner_sigma - cap_sigma) / corner_distance;
    let capture = p.amount > 0.0 && p.iterations > 0 && cap_sigma.max(corner_sigma) >= MIN_SIGMA;
    if !capture {
        return None;
    }

    let mut flags = FLAG_CAPTURE;
    if p.iter_check {
        flags |= FLAG_ITER_CHECK;
    }
    // The mask, with its clip guard, unless the threshold is 0.
    if p.contrast > 0.0 {
        flags |= FLAG_MASK | FLAG_CLIP;
    }
    Some(Plan {
        flags,
        iterations: p.iterations,
        contrast: p.contrast,
        // RawTherapee blurs its mask with sigma 2, at full resolution.
        mask_sigma: (2.0 * s).max(0.3),
        cap_sigma,
        cap_slope,
        cap_amount: p.amount,
    })
}

struct Pipelines {
    prep: wgpu::ComputePipeline,
    mask_h: wgpu::ComputePipeline,
    mask_v: wgpu::ComputePipeline,
    tick: wgpu::ComputePipeline,
    rl_blur_est_h: wgpu::ComputePipeline,
    rl_ratio_v: wgpu::ComputePipeline,
    rl_blur_ratio_h: wgpu::ComputePipeline,
    rl_update_v: wgpu::ComputePipeline,
    capture_mix: wgpu::ComputePipeline,
    compose: wgpu::ComputePipeline,
}

/// Everything the passes need, made once per device.
pub(super) struct Passes {
    pub(super) gpu: Arc<Gpu>,
    /// OpenGL: small submissions, waited for without holding its context.
    gl: bool,
    layout: wgpu::BindGroupLayout,
    pipelines: Pipelines,
    uniform: wgpu::Buffer,
    scratch: [wgpu::Buffer; 5],
    stopped: wgpu::Buffer,
    counter: wgpu::Buffer,
}

impl Passes {
    // Copied from src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d (`Engine::new`),
    // with the bindings capture sharpening uses: the result is an r32float
    // texture of factors, and there is no mask array.
    pub(super) fn new(gpu: &Arc<Gpu>) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt_capture_sharpen"),
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
            label: Some("rt_capture_sharpen"),
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
                        format: wgpu::TextureFormat::R32Float,
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt_capture_sharpen"),
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
            mask_h: pipeline("mask_h"),
            mask_v: pipeline("mask_v"),
            tick: pipeline("tick"),
            rl_blur_est_h: pipeline("rl_blur_est_h"),
            rl_ratio_v: pipeline("rl_ratio_v"),
            rl_blur_ratio_h: pipeline("rl_blur_ratio_h"),
            rl_update_v: pipeline("rl_update_v"),
            capture_mix: pipeline("capture_mix"),
            compose: pipeline("compose"),
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

        Passes {
            gpu: Arc::clone(gpu),
            gl: gpu.adapter.backend == wgpu::Backend::Gl,
            layout,
            pipelines,
            uniform: buffer(
                "rt_capture_sharpen params",
                std::mem::size_of::<Uniform>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            scratch: [
                buffer("rt_capture_sharpen y0", plane, rw),
                buffer("rt_capture_sharpen estimate", plane, rw),
                buffer("rt_capture_sharpen tmp", plane, rw),
                buffer("rt_capture_sharpen aux", plane, rw),
                buffer("rt_capture_sharpen blend", plane, rw),
            ],
            stopped: buffer("rt_capture_sharpen stopped", blocks, rw),
            counter: buffer("rt_capture_sharpen counter", 16, rw),
        }
    }

    /// The factor each pixel of `rgb` is to be multiplied by.
    pub(super) fn ratios(&self, rgb: &Rgb, plan: &Plan) -> Result<Vec<f32>> {
        let device = &self.gpu.device;
        let (w, h) = (rgb.width as u32, rgb.height as u32);
        let limit = device.limits().max_texture_dimension_2d;
        if w > limit || h > limit {
            return Err(Error::Tool {
                tool: RtCaptureSharpen::ID,
                message: format!("{w}x{h} is past this GPU's texture limit of {limit}"),
            });
        }

        let src = self.upload(rgb);
        let ratio = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rt_capture_sharpen ratio"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let src_view = src.create_view(&Default::default());
        let ratio_view = ratio.create_view(&Default::default());

        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&src_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&ratio_view),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt_capture_sharpen"),
            layout: &self.layout,
            entries: &entries,
        });

        let base = Uniform {
            img_w: w,
            img_h: h,
            flags: plan.flags,
            contrast: plan.contrast,
            mask_sigma: plan.mask_sigma,
            cap_sigma: plan.cap_sigma,
            cap_slope: plan.cap_slope,
            cap_amount: plan.cap_amount,
            centre_x: w as f32 * 0.5,
            centre_y: h as f32 * 0.5,
            clip: CLIP_LEVEL,
            ..Default::default()
        };

        // Copied from src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d (`encode`):
        // tiles of TILE with a BORDER, over the whole picture.
        let mut ty = 0;
        while ty < h {
            let th = TILE.min(h - ty);
            let mut tx = 0;
            while tx < w {
                let tw = TILE.min(w - tx);
                let ex0 = tx.saturating_sub(BORDER);
                let ey0 = ty.saturating_sub(BORDER);
                let ex1 = (tx + tw + BORDER).min(w);
                let ey1 = (ty + th + BORDER).min(h);
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
                self.tile(&bind_group, &uniform, plan);
                tx += TILE;
            }
            ty += TILE;
        }

        self.read(&ratio, w, h)
    }

    /// `rgb` as an rgba32float texture, a band of rows at a time so the
    /// staging copy stays small.
    fn upload(&self, rgb: &Rgb) -> wgpu::Texture {
        let (w, h) = (rgb.width as u32, rgb.height as u32);
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rt_capture_sharpen input"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let rows = band_rows(w as usize * 16);
        for y0 in (0..h).step_by(rows) {
            let n = rows.min((h - y0) as usize);
            let start = y0 as usize * rgb.width;
            let band: Vec<f32> = rgb.pixels[start..start + n * rgb.width]
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 1.0])
                .collect();
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&band),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 16),
                    rows_per_image: Some(n as u32),
                },
                wgpu::Extent3d {
                    width: w,
                    height: n as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        texture
    }

    /// The factors back from the GPU, a band of rows at a time.
    fn read(&self, texture: &wgpu::Texture, w: u32, h: u32) -> Result<Vec<f32>> {
        let device = &self.gpu.device;
        let row = (w * 4).div_ceil(256) * 256;
        let rows = band_rows(row as usize) as u32;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt_capture_sharpen readback"),
            size: (row * rows.min(h)) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut out = Vec::with_capacity((w * h) as usize);
        for y0 in (0..h).step_by(rows as usize) {
            let n = rows.min(h - y0);
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(n),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: n,
                    depth_or_array_layers: 1,
                },
            );
            self.gpu.queue.submit(Some(encoder.finish()));
            let slice = buffer.slice(..(row * n) as u64);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(120)),
                })
                .map_err(|e| self.failed(e))?;
            rx.recv()
                .map_err(|e| self.failed(e))?
                .map_err(|e| self.failed(e))?;
            {
                let bytes = slice.get_mapped_range();
                for r in 0..n as usize {
                    let line = &bytes[r * row as usize..r * row as usize + w as usize * 4];
                    out.extend_from_slice(bytemuck::cast_slice::<u8, f32>(line));
                }
            }
            buffer.unmap();
        }
        Ok(out)
    }

    fn failed(&self, e: impl std::fmt::Display) -> Error {
        Error::Tool {
            tool: RtCaptureSharpen::ID,
            message: format!("reading the result back: {e}"),
        }
    }

    // Copied from src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d (`Engine::tile`),
    // the capture sharpening steps and the last pass.
    fn tile(&self, bind_group: &wgpu::BindGroup, u: &Uniform, plan: &Plan) {
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
        for _ in 0..plan.iterations {
            steps.push(vec![
                (&p.tick, one),
                (&p.rl_blur_est_h, whole),
                (&p.rl_ratio_v, whole),
                (&p.rl_blur_ratio_h, whole),
                (&p.rl_update_v, whole),
            ]);
        }
        steps.push(vec![(&p.capture_mix, whole), (&p.compose, whole)]);

        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(u));
        // On GL a few steps at a time: twelve iterations of a tile is tens of
        // milliseconds, well inside the second GL allows another thread.
        let chunks: Vec<Vec<_>> = if self.gl {
            steps
                .chunks(12)
                .map(|group| group.iter().flatten().copied().collect())
                .collect()
        } else {
            vec![steps.iter().flatten().copied().collect()]
        };
        let mut cleared = false;
        for chunk in &chunks {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rt_capture_sharpen tile"),
            });
            if !cleared {
                encoder.clear_buffer(&self.stopped, 0, None);
                encoder.clear_buffer(&self.counter, 0, None);
                cleared = true;
            }
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("rt_capture_sharpen"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, bind_group, &[]);
                for (pipeline, groups) in chunk {
                    pass.set_pipeline(pipeline);
                    pass.dispatch_workgroups(groups.0, groups.1, 1);
                }
            }
            let index = queue.submit(Some(encoder.finish()));
            if self.gl {
                wait_briefly(device, index);
            }
        }
    }
}

/// Rows per band for a row of `row_bytes`, so a band is about 64 MB.
fn band_rows(row_bytes: usize) -> usize {
    ((64 << 20) / row_bytes.max(1)).max(1)
}

// Copied from src-tauri/src/mods/sharpen_gpu.rs @ 2bcb953d.
/// Wait for one submission on OpenGL without holding its context long: a
/// blocking poll on GL keeps the one context lock until the GPU is done, and
/// every other thread that wants it panics after a second.
fn wait_briefly(device: &wgpu::Device, index: wgpu::SubmissionIndex) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        match device.poll(wgpu::PollType::Wait {
            submission_index: Some(index.clone()),
            timeout: Some(Duration::from_millis(200)),
        }) {
            Err(wgpu::PollError::Timeout) => continue,
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DEFAULT_ITERATIONS, DEFAULT_RADIUS};
    use super::*;

    fn resolved() -> Resolved {
        Resolved {
            amount: 1.0,
            radius: DEFAULT_RADIUS,
            corner: 0.0,
            iterations: DEFAULT_ITERATIONS,
            iter_check: true,
            contrast: 0.1,
        }
    }

    #[test]
    fn the_uniform_matches_the_shader_struct() {
        // 20 four-byte fields in the shader's Params.
        assert_eq!(std::mem::size_of::<Uniform>(), 80);
    }

    #[test]
    fn the_shader_compiles() {
        let module = match wgpu::naga::front::wgsl::parse_str(SHADER) {
            Ok(module) => module,
            Err(e) => panic!("does not parse:\n{}", e.emit_to_string(SHADER)),
        };
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&module) {
            panic!("does not validate:\n{}", e.emit_to_string(SHADER));
        }
    }

    // The app's own checks on `plan_for`, for the half that came across.

    #[test]
    fn capture_sharpening_is_skipped_at_fit_to_screen_and_runs_at_100_percent() {
        // 1920 px of a 6000 px photo: 0.75 * 0.32 = 0.24 px of blur.
        assert!(plan(&resolved(), 6000, 4000, 0.32).is_none());
        let full = plan(&resolved(), 6000, 4000, 1.0).expect("runs at 1:1");
        assert_eq!(full.flags & FLAG_CAPTURE, FLAG_CAPTURE);
        assert_eq!(full.flags & FLAG_ITER_CHECK, FLAG_ITER_CHECK);
        assert_eq!(full.iterations, 20);
        assert_eq!(full.cap_sigma, 0.75);
        assert_eq!(full.mask_sigma, 2.0);
    }

    #[test]
    fn corner_boost_reaches_its_radius_at_the_corner() {
        let corner = (3000.0f32.powi(2) + 2000.0f32.powi(2)).sqrt();
        let p = plan(
            &Resolved {
                corner: 0.4,
                ..resolved()
            },
            6000,
            4000,
            1.0,
        )
        .unwrap();
        assert!((p.cap_sigma + p.cap_slope * corner - 1.15).abs() < 1e-4);
        let p = plan(
            &Resolved {
                radius: 1.9,
                corner: 0.5,
                ..resolved()
            },
            6000,
            4000,
            1.0,
        )
        .unwrap();
        assert!((p.cap_sigma + p.cap_slope * corner - 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_threshold_of_zero_has_no_mask() {
        let p = plan(
            &Resolved {
                contrast: 0.0,
                ..resolved()
            },
            600,
            400,
            1.0,
        )
        .unwrap();
        assert_eq!(p.flags & (FLAG_MASK | FLAG_CLIP), 0);
        assert!(
            plan(
                &Resolved {
                    amount: 0.0,
                    ..resolved()
                },
                600,
                400,
                1.0
            )
            .is_none()
        );
    }
}
