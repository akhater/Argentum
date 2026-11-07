//! Noise Reduction before sharpening. Ours, on RapidRAW's maths.
//!
//! RapidRAW runs Details > Noise Reduction inside its main shader, on the
//! texture that shader reads - which in Argentum is the sharpened one
//! (`input_stage.rs`). So its sliders smoothed a noise that capture
//! sharpening and Sharpen had already made stronger. darktable denoises right
//! after demosaic, long before its sharpen (src/common/iop_order.c: denoiseprofile
//! at 9.0, sharpen at 35.0); this pass puts the global amounts there too: the
//! same maths (`noise_reduction.wgsl`), once over the whole input, kept, and
//! handed to sharpening. The main shader then skips the global amounts. A
//! mask's own Noise Reduction still runs there, after sharpening, on what the
//! mask adds.
//!
//! The neighbourhood is the full-resolution picture's, in render pixels
//! (`px_scale`): the same at every zoom and in export, as sharpening's radii
//! are. RapidRAW sized it from the texture, so the fit-to-screen view used a
//! neighbourhood several times wider than the export's.
//!
//! WHEN IT RUNS
//!
//! Only when an amount is above 0, and only when its inputs change: the result
//! is kept with what went into it, so dragging Exposure costs nothing here.
//! Two are kept, for the editor's two pictures while zoomed in (the drag
//! preview and the still one, see `sharpen_gpu.rs`).
//!
//! While its own slider is dragged at 100% it does not run at all
//! (`input_stage::develop`): new input means sharpening again, and capture
//! sharpening's iterations made each tick eight times slower than main.
//!
//! MEMORY
//!
//! A kept result is a texture the size of its render, 8 bytes a pixel
//! (`Rgba16Float`, the smallest format with three float channels a shader can
//! write), and it cannot be one that is already kept: it is what sharpening
//! reads to sharpen again or to sharpen a strip panned into view, and their
//! input texture, which it is made from, cannot be written. At most `KEEP`,
//! only while an amount is above 0: 390 MB at 100% on a 32 MP photo (the photo
//! and its drag preview), 20 MB at a 1920 px fit view. `kept_bytes` counts it
//! and a test holds it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};

use super::sharpen_gpu::Target;
use crate::image_processing::GpuContext;

/// The pass, around RapidRAW's maths: shared helpers, the maths, the entry.
pub const SHADER: &str = concat!(
    include_str!("../shaders/shared.wgsl"),
    include_str!("../shaders/noise_reduction.wgsl"),
    include_str!("../shaders/denoise.wgsl"),
);

/// `denoise.wgsl`'s workgroup, each side.
const WORKGROUP: u32 = 8;

/// The drag preview and the still picture.
const KEEP: usize = 2;

/// `DenoiseParams` in denoise.wgsl, field for field.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug, PartialEq)]
struct Uniform {
    luma: f32,
    color: f32,
    px_scale: f32,
    is_raw: u32,
}

/// One request to denoise a texture.
pub struct Job<'a> {
    pub src: &'a wgpu::TextureView,
    /// What the pixels in `src` are, as `sharpen_gpu::Job::source`: 0 is
    /// unknown, and is never reused.
    pub source: u64,
    /// Which photo, as `sharpen_gpu::Job::family`: another photo's results
    /// are let go.
    pub family: u64,
    pub width: u32,
    pub height: u32,
    pub is_raw: bool,
    /// RapidRAW's amounts, 0 to 1: `luma_noise_reduction`,
    /// `color_noise_reduction`.
    pub luma: f32,
    pub color: f32,
    /// Render pixels per full-resolution pixel.
    pub px_scale: f32,
}

impl Job<'_> {
    /// Whether there is anything to do. Below 0 the shader clamps to 0.
    pub fn runs(&self) -> bool {
        self.luma > 0.0 || self.color > 0.0
    }

    fn uniform(&self) -> Uniform {
        Uniform {
            luma: self.luma,
            color: self.color,
            px_scale: self.px_scale,
            is_raw: self.is_raw as u32,
        }
    }

    /// What the result is, as a `source` for whatever reads it next: this
    /// picture, denoised with these settings. 0 stays unknown.
    pub fn result_source(&self) -> u64 {
        if self.source == 0 {
            return 0;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.source.hash(&mut h);
        bytemuck::bytes_of(&self.uniform()).hash(&mut h);
        h.finish().max(1)
    }
}

/// A kept result, and what it was made from.
struct Kept {
    family: u64,
    source: u64,
    size: (u32, u32),
    settings: Uniform,
    view: wgpu::TextureView,
}

/// Everything the pass needs, made once per device.
struct Engine {
    device: Arc<wgpu::Device>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    uniform: wgpu::Buffer,
    /// Oldest first, at most `KEEP`.
    kept: Vec<Kept>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// Every write into a kept result, counted, as `sharpen_gpu::writes`.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// How many writes into a kept result there have been, ever.
pub fn writes() -> u64 {
    WRITES.load(Ordering::Acquire)
}

/// Whether a result for this photo with these amounts is kept, at any size:
/// the amounts were settled, not moving.
pub fn settled(job: &Job) -> bool {
    let slot = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_ref().is_some_and(|engine| {
        engine.kept.iter().any(|k| {
            k.family == job.family
                && (k.settings.luma, k.settings.color, k.settings.is_raw)
                    == (job.luma, job.color, job.is_raw as u32)
        })
    })
}

/// The bytes the kept results take on the GPU, `Rgba16Float` at 8 a pixel:
/// what the memory test holds.
#[cfg(test)]
pub fn kept_bytes() -> u64 {
    const BYTES_PER_PIXEL: u64 = 8;
    let slot = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_ref().map_or(0, |engine| {
        engine
            .kept
            .iter()
            .map(|k| k.size.0 as u64 * k.size.1 as u64 * BYTES_PER_PIXEL)
            .sum()
    })
}

/// Forget everything kept, as `sharpen_gpu::reset`.
pub fn reset() {
    *ENGINE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Denoise `job.src`, or say there is nothing to do.
pub fn run(context: &GpuContext, job: &Job, target: Target) -> Option<wgpu::TextureView> {
    let mut slot = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    if !job.runs() {
        // The editor's photo is at 0: let go of what was kept, full-size
        // textures. An export's answer says nothing about the editor's.
        if let (Target::Kept, Some(engine)) = (target, slot.as_mut()) {
            engine.kept.clear();
        }
        return None;
    }
    let device = &context.device;
    if slot
        .as_ref()
        .is_none_or(|e| !Arc::ptr_eq(&e.device, device))
    {
        *slot = Some(Engine::new(device));
    }
    let engine = slot.as_mut()?;
    Some(match target {
        Target::Kept => engine.keep(&context.queue, job),
        Target::Fresh => {
            let view = engine.output(job.width, job.height);
            engine.encode(&context.queue, job, &view);
            view
        }
    })
}

impl Engine {
    fn new(device: &Arc<wgpu::Device>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Argentum Denoise Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Argentum Denoise BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Argentum Denoise Layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("denoise"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("denoise"),
            compilation_options: Default::default(),
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Argentum Denoise Params"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Engine {
            device: Arc::clone(device),
            layout,
            pipeline,
            uniform,
            kept: Vec::new(),
        }
    }

    fn output(&self, width: u32, height: u32) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Argentum Denoised Input"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                // COPY_SRC so the tests can read it back.
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// The kept result for `job`, made or remade only when what goes into it
    /// changed.
    fn keep(&mut self, queue: &wgpu::Queue, job: &Job) -> wgpu::TextureView {
        let size = (job.width, job.height);
        let settings = job.uniform();
        // Another photo's results go now, not when they would be evicted.
        self.kept.retain(|k| k.family == job.family);
        if let Some(i) = self.kept.iter().position(|k| {
            job.source != 0 && k.source == job.source && k.size == size && k.settings == settings
        }) {
            let kept = self.kept.remove(i);
            let view = kept.view.clone();
            self.kept.push(kept);
            return view;
        }
        // An older version of this picture at this size is written over;
        // otherwise the oldest makes room.
        let reuse = match self.kept.iter().position(|k| k.size == size) {
            Some(i) => Some(self.kept.remove(i).view),
            None => {
                if self.kept.len() >= KEEP {
                    self.kept.remove(0);
                }
                None
            }
        };
        let view = reuse.unwrap_or_else(|| self.output(job.width, job.height));
        self.encode(queue, job, &view);
        WRITES.fetch_add(1, Ordering::AcqRel);
        self.kept.push(Kept {
            family: job.family,
            source: job.source,
            size,
            settings,
            view: view.clone(),
        });
        view
    }

    fn encode(&self, queue: &wgpu::Queue, job: &Job, dst: &wgpu::TextureView) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&job.uniform()));
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Argentum Denoise BG"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(job.src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(dst),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Argentum Denoise"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Argentum Denoise"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                job.width.div_ceil(WORKGROUP),
                job.height.div_ceil(WORKGROUP),
                1,
            );
        }
        queue.submit(Some(encoder.finish()));
    }
}
