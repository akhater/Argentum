//! The pixel pass on the engine's GPU: the input kept as half floats with its
//! guided filter, one dispatch per render, the result read back.

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use half::f16;
use rayon::prelude::*;
use wgpu::util::DeviceExt;

use super::Shader;
use crate::contract::RenderTicket;
use crate::frame::{Frame, Layout};
use crate::{Error, Gpu, Result, Rgb};

/// The app's guided filter, used in place: its passes are the app's own
/// file, not a copy.
const GUIDED: &str = include_str!("../../../src/shaders/guided.wgsl");

// The sizes and strengths `build_guided_coeffs` gives the filter,
// src-tauri/src/gpu_processing.rs @ c786c146: the tonal scale and the coarse
// one the tone sliders read.
const GF_SHORT_EDGE_PX: u32 = 1080;
const GF_COARSE_DIV: u32 = 4;
const GF_STRUCTURE_RADIUS_PX: f32 = 40.0;
const GF_STRUCTURE_EPS: f32 = 0.5;
const GF_TONAL_RADIUS_PX: f32 = 10.0;
const GF_TONAL_EPS: f32 = 0.25;

const WORKGROUP: u32 = 8;

fn failed(what: &str, e: impl Display) -> Error {
    Error::Tool {
        tool: "pixels",
        message: format!("{what}: {e}"),
    }
}

/// One run of per-pixel tools, compiled, with the picture it last ran on.
pub(super) struct Run {
    gpu: Arc<Gpu>,
    pub shader: Shader,
    frame: wgpu::ComputePipeline,
    linear: wgpu::ComputePipeline,
    frame_layout: wgpu::BindGroupLayout,
    linear_layout: wgpu::BindGroupLayout,
    resident: Option<Resident>,
}

/// The input picture on the GPU, and its guided filter.
struct Resident {
    key: u64,
    size: (u32, u32),
    input: wgpu::TextureView,
    tonal: wgpu::TextureView,
    coarse: wgpu::TextureView,
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn written_entry(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    entry: &str,
) -> wgpu::ComputePipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&pipeline_layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn texture(
    gpu: &Gpu,
    label: &str,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Rows per band for a row of `row_bytes`, so a band is about 64 MB.
fn band_rows(row_bytes: usize) -> u32 {
    ((64 << 20) / row_bytes.max(1)).max(1) as u32
}

impl Run {
    /// Compile `shader`. A tool's WGSL that does not compile is refused here,
    /// with what the compiler said.
    pub fn new(gpu: &Arc<Gpu>, shader: Shader) -> Result<Run> {
        let device = &gpu.device;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pixel pass"),
            source: wgpu::ShaderSource::Wgsl(shader.wgsl.as_str().into()),
        });
        let common = [
            texture_entry(0),
            storage_entry(1),
            texture_entry(2),
            texture_entry(3),
        ];
        let layout = |extra: &[wgpu::BindGroupLayoutEntry]| {
            let entries: Vec<_> = common.iter().chain(extra).copied().collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("pixel pass"),
                entries: &entries,
            })
        };
        let frame_layout = layout(&[
            written_entry(4, wgpu::TextureFormat::Rgba8Unorm),
            storage_entry(6),
            storage_entry(7),
        ]);
        let linear_layout = layout(&[written_entry(5, wgpu::TextureFormat::Rgba32Float)]);
        let frame = pipeline(device, &module, &frame_layout, "ag_frame_main");
        let linear = pipeline(device, &module, &linear_layout, "ag_linear_main");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(failed("the pixel pass does not compile", e));
        }
        Ok(Run {
            gpu: gpu.clone(),
            shader,
            frame,
            linear,
            frame_layout,
            linear_layout,
            resident: None,
        })
    }

    /// Let go of the picture kept.
    pub fn release(&mut self) {
        self.resident = None;
    }

    /// `input` through the run, the whole picture, linear light.
    pub fn linear(
        &mut self,
        guided: Option<&Guided>,
        params: &[[f32; 4]],
        input: &Rgb,
        input_key: u64,
    ) -> Result<Rgb> {
        let size = (input.width as u32, input.height as u32);
        self.keep(guided, input, input_key);
        let gpu = self.gpu.clone();
        let out = texture(
            &gpu,
            "pixel pass linear",
            size,
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let out_view = out.create_view(&Default::default());
        let params = self.buffer(bytemuck::cast_slice(params));
        let r = self.resident()?;
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixel pass linear"),
            layout: &self.linear_layout,
            entries: &[
                bind_view(0, &r.input),
                bind_buffer(1, &params),
                bind_view(2, &r.tonal),
                bind_view(3, &r.coarse),
                bind_view(5, &out_view),
            ],
        });
        self.dispatch(&self.linear, &bind_group, size);
        let texels: Vec<[f32; 4]> = read(&gpu, &out, size)?;
        Ok(input.with_pixels(texels.par_iter().map(|t| [t[0], t[1], t[2]]).collect()))
    }

    /// `input` through the run, as `layout`'s frame of a view: each frame
    /// pixel from the render pixel `layout` picks for it, encoded as sRGB.
    pub fn frame(
        &mut self,
        guided: Option<&Guided>,
        params: &[[f32; 4]],
        input: &Rgb,
        input_key: u64,
        layout: &Layout,
        ticket: RenderTicket,
    ) -> Result<Frame> {
        self.keep(guided, input, input_key);
        let gpu = self.gpu.clone();
        let columns = layout.columns(input.width);
        let rows = layout.rows(input.height);
        let size = (columns.len() as u32, rows.len() as u32);
        let out = texture(
            &gpu,
            "pixel pass frame",
            size,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let out_view = out.create_view(&Default::default());
        let params = self.buffer(bytemuck::cast_slice(params));
        let columns = self.buffer(bytemuck::cast_slice(&columns));
        let rows = self.buffer(bytemuck::cast_slice(&rows));
        let r = self.resident()?;
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixel pass frame"),
            layout: &self.frame_layout,
            entries: &[
                bind_view(0, &r.input),
                bind_buffer(1, &params),
                bind_view(2, &r.tonal),
                bind_view(3, &r.coarse),
                bind_view(4, &out_view),
                bind_buffer(6, &columns),
                bind_buffer(7, &rows),
            ],
        });
        self.dispatch(&self.frame, &bind_group, size);
        Ok(Frame {
            ticket,
            placement: layout.placement,
            rgba: read(&gpu, &out, size)?,
        })
    }

    /// Keep `input` on the GPU, with its guided filter when the run reads
    /// it: sent once per picture, as the app keeps its input texture.
    fn keep(&mut self, guided: Option<&Guided>, input: &Rgb, input_key: u64) {
        let size = (input.width as u32, input.height as u32);
        if self
            .resident
            .as_ref()
            .is_some_and(|r| r.key == input_key && r.size == size)
        {
            return;
        }
        // The one before goes first: at full size it is hundreds of MB.
        self.resident = None;
        let gpu = &self.gpu;
        let input_texture = upload(gpu, input);
        let input_view = input_texture.create_view(&Default::default());
        let (tonal, coarse) = match guided {
            Some(guided) => guided.coefficients(gpu, &input_view, size),
            None => {
                // Bound but never read.
                let empty = |label| {
                    texture(
                        gpu,
                        label,
                        (1, 1),
                        wgpu::TextureFormat::Rgba32Float,
                        wgpu::TextureUsages::TEXTURE_BINDING,
                    )
                    .create_view(&Default::default())
                };
                (empty("no tonal filter"), empty("no coarse filter"))
            }
        };
        self.resident = Some(Resident {
            key: input_key,
            size,
            input: input_view,
            tonal,
            coarse,
        });
    }

    fn resident(&self) -> Result<&Resident> {
        self.resident
            .as_ref()
            .ok_or_else(|| failed("the pixel pass", "its input is not on the GPU"))
    }

    fn buffer(&self, contents: &[u8]) -> wgpu::Buffer {
        self.gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pixel pass"),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
    }

    fn dispatch(
        &self,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        (w, h): (u32, u32),
    ) {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("pixel pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
    }
}

fn bind_view(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn bind_buffer(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// `rgb` as an rgba16float texture, the app's input format, a band of rows at
/// a time.
fn upload(gpu: &Gpu, rgb: &Rgb) -> wgpu::Texture {
    let (w, h) = (rgb.width as u32, rgb.height as u32);
    let input = texture(
        gpu,
        "pixel pass input",
        (w, h),
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let rows = band_rows(w as usize * 8);
    for y0 in (0..h).step_by(rows as usize) {
        let n = rows.min(h - y0);
        let start = y0 as usize * rgb.width;
        let band: Vec<f16> = rgb.pixels[start..start + n as usize * rgb.width]
            .par_iter()
            .flat_map_iter(|p| {
                [
                    f16::from_f32(p[0]),
                    f16::from_f32(p[1]),
                    f16::from_f32(p[2]),
                    f16::ONE,
                ]
            })
            .collect();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&band),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 8),
                rows_per_image: Some(n),
            },
            wgpu::Extent3d {
                width: w,
                height: n,
                depth_or_array_layers: 1,
            },
        );
    }
    input
}

/// A texture's texels back from the GPU as `T`s, rows packed tight, a band
/// of rows at a time. Each mapped row starts on the copy alignment, so it can
/// be read as `T`s where it lies.
fn read<T: bytemuck::Pod>(
    gpu: &Gpu,
    texture: &wgpu::Texture,
    (w, h): (u32, u32),
) -> Result<Vec<T>> {
    let device = &gpu.device;
    let texel_bytes = texture
        .format()
        .block_copy_size(None)
        .ok_or_else(|| failed("reading the pixel pass back", "a texture with no copy size"))?;
    let tight = (w * texel_bytes) as usize;
    let row = (w * texel_bytes).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let rows = band_rows(row as usize).min(h);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pixel pass readback"),
        size: (row * rows) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut out = Vec::with_capacity(tight * h as usize / std::mem::size_of::<T>());
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
        gpu.queue.submit(Some(encoder.finish()));
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
            .map_err(|e| failed("reading the pixel pass back", e))?;
        rx.recv()
            .map_err(|e| failed("reading the pixel pass back", e))?
            .map_err(|e| failed("reading the pixel pass back", e))?;
        {
            let bytes = slice.get_mapped_range();
            for r in 0..n as usize {
                let line = &bytes[r * row as usize..r * row as usize + tight];
                out.extend_from_slice(bytemuck::cast_slice::<u8, T>(line));
            }
        }
        buffer.unmap();
    }
    Ok(out)
}

/// The app's guided filter passes (`guided.wgsl`), compiled once.
pub(super) struct Guided {
    layout: wgpu::BindGroupLayout,
    downsample: wgpu::ComputePipeline,
    box_down: wgpu::ComputePipeline,
    blur_h: wgpu::ComputePipeline,
    blur_v: wgpu::ComputePipeline,
    coeffs: wgpu::ComputePipeline,
}

/// `GfParams` in `guided.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GfParams {
    radius: u32,
    is_raw: u32,
    eps: f32,
    _pad: f32,
}

impl Guided {
    pub fn new(gpu: &Gpu) -> Guided {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("guided filter (the app's guided.wgsl)"),
            source: wgpu::ShaderSource::Wgsl(GUIDED.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("guided filter"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                written_entry(2, wgpu::TextureFormat::Rgba32Float),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let make = |entry| pipeline(device, &module, &layout, entry);
        Guided {
            downsample: make("gf_downsample"),
            box_down: make("gf_box_down"),
            blur_h: make("gf_blur_h"),
            blur_v: make("gf_blur_v"),
            coeffs: make("gf_coeffs"),
            layout,
        }
    }

    /// The guided filter of `input`, at the tonal scale and at the coarse
    /// one: each texel's fitted lines, luminance in `xy`, darkest channel in
    /// `zw`.
    // Copied from src-tauri/src/gpu_processing.rs @ c786c146
    // (`build_guided_coeffs`), less the clarity scale and the packing the
    // app's shader reads them through.
    fn coefficients(
        &self,
        gpu: &Gpu,
        input: &wgpu::TextureView,
        (width, height): (u32, u32),
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        let device = &gpu.device;
        let short = width.min(height).max(1);
        let k = (GF_SHORT_EDGE_PX as f32 / short as f32).min(1.0);
        let lw = ((width as f32 * k).round() as u32).max(1);
        let lh = ((height as f32 * k).round() as u32).max(1);
        let low_short = lw.min(lh) as f32;
        let cw = lw.div_ceil(GF_COARSE_DIV).max(1);
        let ch = lh.div_ceil(GF_COARSE_DIV).max(1);

        let make_view = |label: &'static str, w: u32, h: u32| {
            texture(
                gpu,
                label,
                (w, h),
                wgpu::TextureFormat::Rgba32Float,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            )
            .create_view(&Default::default())
        };
        let make_params = |base_radius: f32, eps: f32, div: f32| {
            let radius = (base_radius * low_short / 1080.0 / div).ceil().max(1.0) as u32;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("guided filter params"),
                contents: bytemuck::bytes_of(&GfParams {
                    radius,
                    is_raw: 1,
                    eps,
                    _pad: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };

        let moments = make_view("GF Moments", lw, lh);
        let tmp = make_view("GF Ping Pong", lw, lh);
        let blurred = make_view("GF Blurred", lw, lh);
        let ab = make_view("GF Coeffs", lw, lh);
        let c_moments = make_view("GF Coarse Moments", cw, ch);
        let c_tmp = make_view("GF Coarse Ping Pong", cw, ch);
        let c_blurred = make_view("GF Coarse Blurred", cw, ch);
        let c_ab = make_view("GF Coarse Coeffs", cw, ch);

        let params_tonal = make_params(GF_TONAL_RADIUS_PX, GF_TONAL_EPS, 1.0);
        let params_coarse = make_params(
            GF_STRUCTURE_RADIUS_PX,
            GF_STRUCTURE_EPS,
            GF_COARSE_DIV as f32,
        );

        let mut encoder = device.create_command_encoder(&Default::default());
        let mut pass = |pipeline: &wgpu::ComputePipeline,
                        src: &wgpu::TextureView,
                        dst: &wgpu::TextureView,
                        params: &wgpu::Buffer,
                        (w, h): (u32, u32)| {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("guided filter"),
                layout: &self.layout,
                entries: &[
                    bind_view(0, src),
                    bind_view(1, src),
                    bind_view(2, dst),
                    bind_buffer(3, params),
                ],
            });
            let mut cpass = encoder.begin_compute_pass(&Default::default());
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1);
        };
        let fine = (lw, lh);
        let coarse = (cw, ch);

        pass(&self.downsample, input, &moments, &params_tonal, fine);

        pass(&self.box_down, &moments, &c_moments, &params_coarse, coarse);
        pass(&self.blur_h, &c_moments, &c_tmp, &params_coarse, coarse);
        pass(&self.blur_v, &c_tmp, &c_blurred, &params_coarse, coarse);
        pass(&self.coeffs, &c_blurred, &c_ab, &params_coarse, coarse);
        pass(&self.blur_h, &c_ab, &c_tmp, &params_coarse, coarse);
        pass(&self.blur_v, &c_tmp, &c_moments, &params_coarse, coarse);

        pass(&self.blur_h, &moments, &tmp, &params_tonal, fine);
        pass(&self.blur_v, &tmp, &blurred, &params_tonal, fine);
        pass(&self.coeffs, &blurred, &ab, &params_tonal, fine);
        pass(&self.blur_h, &ab, &tmp, &params_tonal, fine);
        pass(&self.blur_v, &tmp, &blurred, &params_tonal, fine);

        gpu.queue.submit(Some(encoder.finish()));
        (blurred, c_moments)
    }
}
