//! The app's own shader functions, run on the GPU, for the parity tests.
//!
//! The app compiles `shaders/modules.wgsl` followed by `shaders/shader.wgsl`
//! as one module (`SHADER` in `mods/export_precision.rs`). This does the
//! same with the files as they are on disk, adds an entry point that calls
//! their functions once per element, and reads back what they return. A
//! change to one of their functions shows up here the next time the tests
//! run, with nothing to keep in step.
//!
//! The bind group layout is written out rather than derived, so a body that
//! uses only some of these still binds. What the entry point can hand the
//! call, besides its element:
//! - `ag_test_params`, a list of `vec4<f32>`;
//! - their `adjustments` uniform, zeroed but for the fields a test sets;
//! - their `input_texture`, of which their functions read only the size;
//! - `ag_test_tex` and `ag_test_tex2`, rgba32float textures of the test's own.

use std::path::PathBuf;
use std::sync::Arc;

use argentum_engine::Gpu;
use wgpu::util::DeviceExt;

/// `src-tauri/src/shaders/`, read from disk at run time.
pub fn shaders_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/shaders")
}

pub fn read(name: &str) -> String {
    let path = shaders_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One run: the call, once for each of `inputs`.
#[derive(Default)]
pub struct Job<'a> {
    pub inputs: &'a [[f32; 4]],
    pub params: &'a [[f32; 4]],
    /// Fields of their `adjustments`, by path from it, and their bytes.
    pub adjustments: &'a [(&'a [&'a str], Vec<u8>)],
    /// The size of their `input_texture`.
    pub input_size: (u32, u32),
    /// `ag_test_tex`: width, height, texels.
    pub texture: Option<(u32, u32, &'a [[f32; 4]])>,
    /// `ag_test_tex2`, the same.
    pub texture2: Option<(u32, u32, &'a [[f32; 4]])>,
}

pub struct AppShader {
    gpu: Arc<Gpu>,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    module: naga::Module,
}

const ENTRY: &str = r#"
@group(0) @binding(20) var<storage, read> ag_test_in: array<vec4<f32>>;
@group(0) @binding(21) var<storage, read> ag_test_params: array<vec4<f32>>;
@group(0) @binding(22) var<storage, read_write> ag_test_out: array<vec4<f32>>;
@group(0) @binding(23) var ag_test_tex: texture_2d<f32>;
@group(0) @binding(24) var ag_test_tex2: texture_2d<f32>;

fn ag_test_call(v: vec4<f32>, i: u32) -> vec4<f32> {
    BODY
}

@compute @workgroup_size(64)
fn ag_test_main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let i = gid.y * groups.x * 64u + gid.x;
    if (i >= arrayLength(&ag_test_out)) { return; }
    ag_test_out[i] = ag_test_call(ag_test_in[i], i);
}
"#;

impl AppShader {
    /// Their shader with `body` as the WGSL body of
    /// `fn ag_test_call(v: vec4<f32>, i: u32) -> vec4<f32>`: `v` is the
    /// element, `i` its index.
    pub fn new(gpu: Arc<Gpu>, body: &str) -> AppShader {
        let source = format!(
            "{}{}{}",
            read("modules.wgsl"),
            read("shader.wgsl"),
            ENTRY.replace("BODY", body)
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| {
            panic!(
                "their shader with the test's call: {}",
                e.emit_to_string(&source)
            )
        });
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("the app's shader, under test"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                texture(0),
                storage(2, true),
                storage(20, true),
                storage(21, true),
                storage(22, false),
                texture(23),
                texture(24),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("ag_test_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        AppShader {
            gpu,
            pipeline,
            layout,
            module,
        }
    }

    /// Where a field of their `adjustments` starts, and the whole struct's
    /// size: read off their shader by naga, so the test never restates
    /// their layout.
    fn adjustments_layout(&self, path: &[&str]) -> (u32, u32) {
        let m = &self.module;
        let var = m
            .global_variables
            .iter()
            .find(|(_, v)| v.name.as_deref() == Some("adjustments"))
            .expect("their shader declares `adjustments`")
            .1;
        let naga::TypeInner::Struct { span, .. } = m.types[var.ty].inner else {
            panic!("`adjustments` is not a struct")
        };
        let mut ty = var.ty;
        let mut offset = 0;
        for name in path {
            let naga::TypeInner::Struct { members, .. } = &m.types[ty].inner else {
                panic!("{name}: not in a struct")
            };
            let member = members
                .iter()
                .find(|f| f.name.as_deref() == Some(*name))
                .unwrap_or_else(|| panic!("their `adjustments` has no {name}"));
            offset += member.offset;
            ty = member.ty;
        }
        (offset, span)
    }

    pub fn run(&self, job: &Job) -> Vec<[f32; 4]> {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let buffer = |data: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage,
            })
        };
        let (_, span) = self.adjustments_layout(&[]);
        let mut adjustments = vec![0u8; span as usize];
        for (path, bytes) in job.adjustments {
            let (at, _) = self.adjustments_layout(path);
            adjustments[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
        }
        let n = job.inputs.len();
        let pad = |v: &[[f32; 4]]| -> Vec<[f32; 4]> {
            if v.is_empty() {
                vec![[0.0; 4]]
            } else {
                v.to_vec()
            }
        };
        let storage = wgpu::BufferUsages::STORAGE;
        let adjustments = buffer(&adjustments, storage);
        let inputs = buffer(bytemuck::cast_slice(&pad(job.inputs)), storage);
        let params = buffer(bytemuck::cast_slice(&pad(job.params)), storage);
        let out_bytes = (n.max(1) * 16) as u64;
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out_bytes,
            usage: storage | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = |(w, h): (u32, u32), texels: &[[f32; 4]]| {
            let data = if texels.is_empty() {
                vec![[0.0f32; 4]; (w * h) as usize]
            } else {
                texels.to_vec()
            };
            device
                .create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba32Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    bytemuck::cast_slice(&data),
                )
                .create_view(&Default::default())
        };
        let input_size = (job.input_size.0.max(1), job.input_size.1.max(1));
        // Their input texture: only its size is read, so it is left empty.
        let input_texture = texture(input_size, &[]);
        let test_texture = match job.texture {
            Some((w, h, texels)) => texture((w, h), texels),
            None => texture((1, 1), &[]),
        };
        let test_texture2 = match job.texture2 {
            Some((w, h, texels)) => texture((w, h), texels),
            None => texture((1, 1), &[]),
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: adjustments.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 20,
                    resource: inputs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 21,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 22,
                    resource: out.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 23,
                    resource: wgpu::BindingResource::TextureView(&test_texture),
                },
                wgpu::BindGroupEntry {
                    binding: 24,
                    resource: wgpu::BindingResource::TextureView(&test_texture2),
                },
            ],
        });
        let groups = (n.max(1) as u32).div_ceil(64);
        let (gx, gy) = (groups.min(65535), groups.div_ceil(65535));
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        encoder.copy_buffer_to_buffer(&out, 0, &readback, 0, out_bytes);
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("mapped"));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(120)),
            })
            .expect("the GPU finished");
        let values: Vec<[f32; 4]> = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
        values[..n].to_vec()
    }
}

/// A `mat3x3<f32>` as WGSL lays one out: three columns of 16 bytes.
pub fn mat3_bytes(rows: [[f32; 3]; 3]) -> Vec<u8> {
    (0..3)
        .flat_map(|c| [rows[0][c], rows[1][c], rows[2][c], 0.0])
        .flat_map(f32::to_le_bytes)
        .collect()
}
