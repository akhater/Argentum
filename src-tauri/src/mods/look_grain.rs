//! Grain for a provider look: its provider's map, and the noise drawn with it
//! right after the LUT. Ours.
//!
//! A provider that can offers grain as an option marked `makesGrain`
//! (mods/lut_provider.rs). It is no LUT option: never sent with `lut`, never in
//! a look file's name, never a reason to make one. The edit holds it next to
//! the look, on or off and how strong ([`LookGrain`], `agProviderLook.grain`).
//!
//! The provider makes one map per look, the same for every photo: two 64-step
//! cubes in a 16-bit PNG, indexed by the colour after the LUT, that say how
//! much of three noise layers A, B and C each channel gets, in levels per unit
//! of noise. [`ready`] keeps it in the app's cache folder and the last few in
//! memory; a render draws grain only for a map already here, so nothing in a
//! render ever runs the provider.
//!
//! The drawing is the provider's reference: each layer normal noise on a
//! lattice of `cells` across the frame's long side, smoothed with
//! `[1, 7, 1] / sqrt(51)`, enlarged with bicubic and scaled so it measures 1
//! after an area shrink back to the lattice ([`fit`]); then, with `k` the amount,
//!
//! ```text
//! R += k a A    G += k (b A + d B)    B += k (e A + f B + g C)
//! ```
//!
//! and a clip (`ag_look_grain` in modules.wgsl). The lattice is hashed from its
//! cells and a seed per photo, so it is tied to the frame, not to pixels: every
//! render of a photo, preview or export, has the same grain. It is smoothed on
//! the GPU once for a seed and size and kept ([`Gpu`]), so a render reads 16 of
//! its nodes a pixel.

use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bytemuck::{Pod, Zeroable};
use half::f16;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wgpu::util::{DeviceExt, TextureDataOrder};

use crate::file_management::parse_virtual_path;
use crate::mods::atomic_write::write_atomic;
use crate::mods::provider_look;

/// Steps of each cube, as for a LUT.
const STEPS: u32 = 64;
/// The map PNG: two cubes, one above the other, each laid out like a LUT PNG.
const MAP_PX: (u32, u32) = (512, 1024);
const MAX_MAP_BYTES: usize = 8 * 1024 * 1024;
/// What a provider may say of its lattice, and of its scale. The lattice's
/// texture must fit any GPU's 8192.
const CELLS: std::ops::RangeInclusive<u32> = 16..=8000;
const MAX_SCALE: f64 = 1.0;
/// Past this an amount is the provider's mistake or a hand edit.
pub const MAX_AMOUNT: f32 = 8.0;
/// Maps kept in memory: one is 4 MB (two cubes of half floats).
const MAPS_KEPT: usize = 4;
/// The cache folder's budget: a map is 1 to 2 MB, a provider about 20 looks.
const MAX_FOLDER_BYTES: u64 = 64 * 1024 * 1024;
/// Lattice units [`axis_gain`] averages over.
const GAIN_SAMPLES: u32 = 512;
/// What a render smaller than the lattice stands for: a full-size export,
/// about 6000 pixels over 2040 cells.
const EXPORT_PX_PER_CELL: u32 = 3;

/// Grain on a provider look, as the edit holds it under `agProviderLook`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LookGrain {
    pub on: bool,
    /// `k`: 1 is the look's own strength.
    pub amount: f32,
}

/// What `grain` answered about the map it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapFacts {
    /// Of a sample, in levels out of 255 per unit of noise, away from 32768.
    pub scale: f64,
    /// The lattice across the long side.
    pub cells: u32,
}

impl MapFacts {
    fn is_sane(&self) -> bool {
        self.scale.is_finite()
            && self.scale > 0.0
            && self.scale <= MAX_SCALE
            && CELLS.contains(&self.cells)
    }
}

/// A look's map, ready for the GPU: both cubes as one 64 x 64 x 128 texture
/// of RGBA half floats, cube 2 above cube 1, in output units (out of 1).
pub struct GrainMap {
    texels: Vec<f16>,
    cells: u32,
    /// Tells this map from any other this session.
    serial: u32,
}

static SERIALS: AtomicU32 = AtomicU32::new(1);

impl GrainMap {
    /// The map in a provider's PNG, or None for a file that is not one.
    pub fn read(bytes: &[u8], facts: MapFacts) -> Option<Self> {
        if bytes.len() > MAX_MAP_BYTES || !facts.is_sane() {
            return None;
        }
        let reader = || ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        if reader().into_dimensions().ok()? != MAP_PX {
            return None;
        }
        let DynamicImage::ImageRgb16(png) = reader().decode().ok()? else {
            return None;
        };
        let n = STEPS as usize;
        let mut texels = vec![f16::ZERO; n * n * 2 * n * 4];
        let level = facts.scale / 255.0;
        for (x, y, pixel) in png.enumerate_pixels() {
            // Row b * 8 + g / 8, column (g % 8) * 64 + r, cube 2 512 rows lower.
            let (cube, row) = (y / MAP_PX.0, y % MAP_PX.0);
            let (r, g, b) = (x % STEPS, (row % 8) * 8 + x / STEPS, row / 8);
            let at = (((cube * STEPS + b) * STEPS + g) * STEPS + r) as usize * 4;
            for c in 0..3 {
                let value = (f64::from(pixel[c]) - 32768.0) * level;
                texels[at + c] = f16::from_f64(value);
            }
        }
        let serial = SERIALS.fetch_add(1, Ordering::Relaxed);
        Some(Self {
            texels,
            cells: facts.cells,
            serial,
        })
    }
}

// ------------------------------------------------------------------ the maps

/// The maps in memory, newest last, by look.
static KEPT: Mutex<VecDeque<(String, Arc<GrainMap>)>> = Mutex::new(VecDeque::new());

fn kept() -> MutexGuard<'static, VecDeque<(String, Arc<GrainMap>)>> {
    KEPT.lock().unwrap_or_else(|e| e.into_inner())
}

/// The map of `look`, if it is in memory.
pub fn loaded(look: &str) -> Option<Arc<GrainMap>> {
    kept()
        .iter()
        .find(|(id, _)| id == look)
        .map(|(_, map)| Arc::clone(map))
}

fn by_serial(serial: u32) -> Option<Arc<GrainMap>> {
    kept()
        .iter()
        .find(|(_, map)| map.serial == serial)
        .map(|(_, map)| Arc::clone(map))
}

/// Keeps `map` for `look`, in place of any it had, within `MAPS_KEPT`.
pub fn keep(look: &str, map: GrainMap) {
    let mut maps = kept();
    maps.retain(|(id, _)| id != look);
    maps.push_back((look.to_string(), Arc::new(map)));
    while maps.len() > MAPS_KEPT {
        maps.pop_front();
    }
}

/// `look`'s map, in memory once this is true. From the cache folder if it is
/// there, else made by `make`, which writes the provider's PNG where it is told
/// and says what it wrote. `key` names the map in the folder: the provider, its
/// version and the look, so a new provider makes its own.
pub fn ready(
    folder: &Path,
    key: &str,
    look: &str,
    make: impl FnOnce(&Path) -> Option<MapFacts>,
) -> bool {
    static MAKING: Mutex<()> = Mutex::new(());
    if loaded(look).is_some() {
        return true;
    }
    let _one = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    if loaded(look).is_some() {
        return true;
    }
    let (png, facts_file) = files_of(folder, key);
    if let Some(map) = from_folder(&png, &facts_file) {
        keep(look, map);
        return true;
    }
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let scratch = png.with_extension("new.png");
    let made = make(&scratch).and_then(|facts| {
        let bytes = std::fs::read(&scratch).ok()?;
        let map = GrainMap::read(&bytes, facts)?;
        let text = serde_json::to_vec(&facts).ok()?;
        (write_atomic(&png, &bytes).is_ok() && write_atomic(&facts_file, &text).is_ok())
            .then_some(map)
    });
    let _ = std::fs::remove_file(&scratch);
    let Some(map) = made else {
        return false;
    };
    crate::mods::lut_provider::trim_folder(folder, MAX_FOLDER_BYTES);
    keep(look, map);
    true
}

/// The map's PNG and what its provider said of it, in `folder`.
fn files_of(folder: &Path, key: &str) -> (PathBuf, PathBuf) {
    let hash = blake3::hash(key.as_bytes()).to_hex();
    let stem = folder.join(&hash[..32]);
    (
        stem.with_extension("grain.png"),
        stem.with_extension("grain.json"),
    )
}

fn from_folder(png: &Path, facts_file: &Path) -> Option<GrainMap> {
    let facts: MapFacts = serde_json::from_slice(&std::fs::read(facts_file).ok()?).ok()?;
    GrainMap::read(&std::fs::read(png).ok()?, facts)
}

// ------------------------------------------------------------------ a render

/// The grain of a render, as `AgLookGrain` in modules.wgsl reads it. Rides in
/// RapidRAW's `GlobalAdjustments`, as `color_grade::Params` does. 48 bytes.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default, PartialEq)]
pub struct Params {
    pub on: u32,
    pub seed: u32,
    pub amount: f32,
    /// Makes each layer measure 1 at the lattice's size, for this render's frame.
    pub norm: f32,
    /// The frame, in pixels of this render.
    pub frame: [f32; 2],
    /// The lattice across the frame's width and height.
    pub cells: [f32; 2],
    /// The map's serial: the render looks it up once more as it starts.
    pub map: u32,
    _pad: [u32; 3],
}

impl Params {
    /// The grain `js` has on, for a map that is in memory; off otherwise.
    /// The frame is the render's to [`fit`].
    pub fn of(js: &Value, photo: Option<&str>) -> Self {
        let Some(look) = provider_look::look_of(js) else {
            return Self::default();
        };
        let Some(grain) = look.grain.filter(|g| g.on && g.amount.is_finite()) else {
            return Self::default();
        };
        let Some(map) = loaded(&look.look) else {
            return Self::default();
        };
        Self {
            on: 1,
            seed: photo.map_or(0, seed_of),
            amount: grain.amount.clamp(0.0, MAX_AMOUNT),
            map: map.serial,
            ..Self::default()
        }
    }
}

/// The same for every render of `photo`, a virtual copy's as its photo's.
fn seed_of(photo: &str) -> u32 {
    let source = parse_virtual_path(photo).0;
    let hash = blake3::hash(source.as_os_str().as_encoded_bytes());
    u32::from_le_bytes(hash.as_bytes()[..4].try_into().unwrap_or_default())
}

/// The map `params` names, with `params` fitted to a frame of `width` x
/// `height` pixels; None, and `params` off, when it has no grain or its map
/// has gone.
pub fn fit(params: &mut Params, width: u32, height: u32) -> Option<Arc<GrainMap>> {
    if params.on == 0 {
        return None;
    }
    let Some(map) = by_serial(params.map).filter(|_| width > 0 && height > 0) else {
        params.on = 0;
        return None;
    };
    let (long, short) = (width.max(height), width.min(height));
    let short_cells =
        ((f64::from(map.cells) * f64::from(short) / f64::from(long)).round() as u32).max(1);
    let [cells_x, cells_y] = if width >= height {
        [map.cells, short_cells]
    } else {
        [short_cells, map.cells]
    };
    params.frame = [width as f32, height as f32];
    params.cells = [cells_x as f32, cells_y as f32];
    params.norm = (axis_gain(width, cells_x) * axis_gain(height, cells_y)).sqrt() as f32;
    Some(map)
}

/// `[1, 7, 1] / sqrt(51)`: its squares sum to 1, so a smoothed layer still
/// measures 1 on the lattice.
const SMOOTH: [f64; 3] = [
    0.140_028_008_402_8,
    0.980_196_058_819_6,
    0.140_028_008_402_8,
];

/// OpenCV's bicubic, `A` = -0.75, at `t` past a node: the weights of the nodes
/// at -1, 0, 1 and 2. `ag_grain_bicubic` in modules.wgsl is its twin.
fn bicubic(t: f64) -> [f64; 4] {
    const A: f64 = -0.75;
    let near = |x: f64| ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0;
    let far = |x: f64| ((A * x - 5.0 * A) * x + 8.0 * A) * x - 4.0 * A;
    let w = [far(t + 1.0), near(t), near(1.0 - t)];
    [w[0], w[1], w[2], 1.0 - w[0] - w[1] - w[2]]
}

/// What a pixel at `x` on the lattice takes of the raw noise at `floor(x) - 2`
/// to `floor(x) + 3`: the bicubic over smoothed nodes (`ag_grain_noise` in
/// modules.wgsl, over shaders/look_grain.wgsl), as one six-tap filter.
fn taps(x: f64) -> (i64, [f64; 6]) {
    let node = x.floor();
    let c = bicubic(x - node);
    let [h1, h0] = [SMOOTH[0], SMOOTH[1]];
    let taps = [
        c[0] * h1,
        c[0] * h0 + c[1] * h1,
        c[0] * h1 + c[1] * h0 + c[2] * h1,
        c[1] * h1 + c[2] * h0 + c[3] * h1,
        c[2] * h1 + c[3] * h0,
        c[3] * h1,
    ];
    (node as i64 - 2, taps)
}

/// Where pixel `p` of `pixels` sits on a lattice of `cells`, as OpenCV's resize
/// puts it.
fn on_lattice(p: f64, pixels: u32, cells: u32) -> f64 {
    (p + 0.5) * f64::from(cells) / f64::from(pixels) - 0.5
}

/// The parts of `[lo, hi)` each unit square in it covers, for an area shrink.
fn covered(lo: f64, hi: f64) -> impl Iterator<Item = (i64, f64)> {
    (lo.floor() as i64..hi.ceil() as i64).filter_map(move |i| {
        let part = (hi.min(i as f64 + 1.0) - lo.max(i as f64)).max(0.0);
        (part > 0.0).then_some((i, part))
    })
}

/// The sum of squares of weights on independent noise: its variance.
fn variance(weights: &std::collections::HashMap<i64, f64>) -> f64 {
    weights.values().map(|w| w * w).sum()
}

/// The factor one axis gives the variance of the drawn noise, for `pixels` on
/// a lattice of `cells`. The noise is separable, so two of them make the
/// frame's. Enlarged, the reference scales it to measure 1 after an area shrink
/// back to the lattice. Shrunk, a pixel stands for the area average of a
/// full-size export under it; the render samples one point, scaled to measure
/// what that average would.
fn axis_gain(pixels: u32, cells: u32) -> f64 {
    use std::collections::HashMap;
    let s = f64::from(pixels) / f64::from(cells);
    let mut sum = 0.0;
    let mut point = 0.0;
    let count = GAIN_SAMPLES.min(if s >= 1.0 { cells } else { pixels });
    for unit in 0..count {
        let mut weights = HashMap::new();
        if s >= 1.0 {
            // One cell of the shrink, from the pixels it covers.
            let at = f64::from(unit + (cells - count) / 2);
            for (p, part) in covered(at * s, (at + 1.0) * s) {
                let (first, w) = taps(on_lattice(p as f64, pixels, cells));
                for (i, w) in w.iter().enumerate() {
                    *weights.entry(first + i as i64).or_insert(0.0) += part / s * w;
                }
            }
        } else {
            // One pixel, as the area average of the export's pixels it covers.
            let p = f64::from(unit + (pixels - count) / 2);
            let export = f64::from(EXPORT_PX_PER_CELL);
            let under = export / s;
            for (q, part) in covered(p * under, (p + 1.0) * under) {
                let (first, w) = taps((q as f64 + 0.5) / export - 0.5);
                for (i, w) in w.iter().enumerate() {
                    *weights.entry(first + i as i64).or_insert(0.0) += part / under * w;
                }
            }
            point += taps(on_lattice(p, pixels, cells))
                .1
                .iter()
                .map(|w| w * w)
                .sum::<f64>();
        }
        sum += variance(&weights);
    }
    if s >= 1.0 {
        f64::from(count) / sum
    } else {
        sum / point * axis_gain(cells * EXPORT_PX_PER_CELL, cells)
    }
}

// ------------------------------------------------------------------ the GPU

/// Bindings of modules.wgsl, after `ag_grade_curves`.
pub const MAP_BINDING: u32 = 16;
pub const SAMPLER_BINDING: u32 = 17;
pub const LATTICE_BINDING: u32 = 18;
/// Nodes past each edge of the lattice the bicubic reads: texel 0 is node -2.
const LATTICE_MARGIN: u32 = 2;

/// The lattice's own shader.
pub const LATTICE_SOURCE: &str = include_str!("../shaders/look_grain.wgsl");

/// What one render binds.
pub struct Bound {
    map: wgpu::TextureView,
    lattice: wgpu::TextureView,
}

/// A render's grain on the GPU: the map, and the smoothed lattice
/// (shaders/look_grain.wgsl), each kept for the next render while it is the
/// same. One lattice is 22 MB at 2040 cells; none is made without grain.
pub struct Gpu {
    no_map: wgpu::TextureView,
    no_lattice: wgpu::TextureView,
    sampler: wgpu::Sampler,
    smooth: wgpu::ComputePipeline,
    smooth_layout: wgpu::BindGroupLayout,
    map: Mutex<Option<(u32, wgpu::TextureView)>>,
    /// By seed and lattice size.
    lattice: Mutex<Option<([u32; 3], wgpu::TextureView)>>,
}

fn half_float_2d(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Gpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let no_map = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("No Grain Map"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 2,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let binding = wgpu::TextureUsages::TEXTURE_BINDING;
        let no_lattice = half_float_2d(device, "No Grain Lattice", (1, 1), binding);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Grain Map Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let smooth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Grain Lattice Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grain Lattice Shader"),
            source: wgpu::ShaderSource::Wgsl(LATTICE_SOURCE.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grain Lattice Pipeline Layout"),
            bind_group_layouts: &[Some(&smooth_layout)],
            immediate_size: 0,
        });
        let smooth = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Grain Lattice"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("ag_grain_lattice"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            no_map,
            no_lattice,
            sampler,
            smooth,
            smooth_layout,
            map: Mutex::new(None),
            lattice: Mutex::new(None),
        }
    }

    /// Its bindings, for the main layout.
    pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
        let texture = |binding, filterable, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        [
            texture(MAP_BINDING, true, wgpu::TextureViewDimension::D3),
            wgpu::BindGroupLayoutEntry {
                binding: SAMPLER_BINDING,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            texture(LATTICE_BINDING, false, wgpu::TextureViewDimension::D2),
        ]
    }

    /// What a render with `params`, fitted to its frame ([`fit`]), binds: the
    /// empty ones without `map`.
    pub fn bind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: &Params,
        map: Option<&GrainMap>,
    ) -> Bound {
        let Some(map) = map else {
            return Bound {
                map: self.no_map.clone(),
                lattice: self.no_lattice.clone(),
            };
        };
        Bound {
            map: self.map_view(device, queue, map),
            lattice: self.lattice_view(device, queue, params),
        }
    }

    fn map_view(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map: &GrainMap,
    ) -> wgpu::TextureView {
        let mut last = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((serial, view)) = last.as_ref()
            && *serial == map.serial
        {
            return view.clone();
        }
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Grain Map"),
                size: wgpu::Extent3d {
                    width: STEPS,
                    height: STEPS,
                    depth_or_array_layers: 2 * STEPS,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::MipMajor,
            bytemuck::cast_slice(&map.texels),
        );
        let view = texture.create_view(&Default::default());
        *last = Some((map.serial, view.clone()));
        view
    }

    /// The smoothed lattice of `params`' seed and size, made on the GPU the
    /// first time; the one before is let go first.
    fn lattice_view(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: &Params,
    ) -> wgpu::TextureView {
        let [cells_x, cells_y] = params.cells.map(|c| c as u32);
        let key = [params.seed, cells_x, cells_y];
        let mut last = self.lattice.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((made_for, view)) = last.as_ref()
            && *made_for == key
        {
            return view.clone();
        }
        *last = None;
        let size = (cells_x + 2 * LATTICE_MARGIN, cells_y + 2 * LATTICE_MARGIN);
        let usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = half_float_2d(device, "Grain Lattice", size, usage);
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Grain Lattice Params"),
            contents: bytemuck::cast_slice(&[params.seed, size.0, size.1, 0u32]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grain Lattice Bind Group"),
            layout: &self.smooth_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Grain Lattice"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.smooth);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        }
        queue.submit([encoder.finish()]);
        *last = Some((key, view.clone()));
        view
    }

    /// Its bindings for a render, with `bound` from [`Gpu::bind`].
    pub fn bind_entries<'a>(&'a self, bound: &'a Bound) -> [wgpu::BindGroupEntry<'a>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: MAP_BINDING,
                resource: wgpu::BindingResource::TextureView(&bound.map),
            },
            wgpu::BindGroupEntry {
                binding: SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
                binding: LATTICE_BINDING,
                resource: wgpu::BindingResource::TextureView(&bound.lattice),
            },
        ]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::dng_standard::gpu::device;
    use image::{ImageBuffer, Rgb, Rgba};
    use serde_json::json;

    /// Tests that keep maps take turns: [`MAPS_KEPT`] is shared.
    pub(crate) fn one_at_a_time() -> MutexGuard<'static, ()> {
        static TURN: Mutex<()> = Mutex::new(());
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forgets every map in memory.
    pub(crate) fn forget_all() {
        kept().clear();
    }

    fn png_of(image: DynamicImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("png");
        bytes
    }

    /// A map's PNG with `abd` levels in every point of cube 1 and `efg` in
    /// cube 2, at `scale`.
    pub(crate) fn flat_png(abd: [f64; 3], efg: [f64; 3], scale: f64) -> Vec<u8> {
        let sample = |levels: f64| (32768.0 + levels / scale).round() as u16;
        png_of(DynamicImage::ImageRgb16(ImageBuffer::from_fn(
            MAP_PX.0,
            MAP_PX.1,
            |_, y| Rgb(if y < MAP_PX.0 { abd } else { efg }.map(sample)),
        )))
    }

    pub(crate) const SCALE: f64 = 0.001;

    /// Grain at amount 2 from a flat map kept for the speed budgets' row
    /// (sharpen_gpu.rs `where_the_time_goes`).
    pub(crate) fn speed_params() -> Params {
        let png = flat_png([3.0, 3.0, 0.0], [3.0, 0.0, 0.0], SCALE);
        let facts = MapFacts {
            scale: SCALE,
            cells: 2040,
        };
        keep("speed-row", GrainMap::read(&png, facts).expect("a map"));
        let edit = json!({
            "lutPath": provider_look::MARKER,
            "agProviderLook": {
                "camera": "cam1",
                "look": "speed-row",
                "grain": { "on": true, "amount": 2 },
                "before": {},
            },
        });
        Params::of(&edit, Some("speed.CR3"))
    }

    /// The brief's flat map: 3 levels in a, b and e, none in d, f and g.
    fn flat_map(cells: u32) -> GrainMap {
        let png = flat_png([3.0, 3.0, 0.0], [3.0, 0.0, 0.0], SCALE);
        GrainMap::read(
            &png,
            MapFacts {
                scale: SCALE,
                cells,
            },
        )
        .expect("a map")
    }

    #[test]
    fn the_lattice_shader_compiles() {
        let module = wgpu::naga::front::wgsl::parse_str(LATTICE_SOURCE)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(LATTICE_SOURCE)));
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("validates");
    }

    #[test]
    fn each_point_of_the_png_lands_in_its_cube() {
        let scale = 0.01;
        let sample = |levels: f64| (32768.0 + levels / scale).round() as u16;
        // Point (r, g, b) = (5, 20, 40) of cube 2, where the brief puts it.
        let (r, g, b) = (5u32, 20u32, 40u32);
        let png = png_of(DynamicImage::ImageRgb16(ImageBuffer::from_fn(
            MAP_PX.0,
            MAP_PX.1,
            |x, y| {
                let hit = y == MAP_PX.0 + b * 8 + g / 8 && x == (g % 8) * 64 + r;
                Rgb(if hit { [1.0, 2.0, 3.0] } else { [0.0; 3] }.map(sample))
            },
        )));
        let map = GrainMap::read(&png, MapFacts { scale, cells: 2040 }).expect("a map");
        let at = (((STEPS + b) * STEPS + g) * STEPS + r) as usize * 4;
        for (got, want) in map.texels[at..at + 3].iter().zip([1.0, 2.0, 3.0]) {
            assert!((got.to_f32() * 255.0 - want).abs() < 0.01, "{got} {want}");
        }
        let elsewhere: f32 = map.texels.iter().map(|v| v.to_f32().abs()).sum();
        assert!((elsewhere * 255.0 - 6.0).abs() < 0.05, "{elsewhere}");
    }

    #[test]
    fn a_map_of_another_size_depth_or_scale_is_none() {
        let good = flat_png([3.0; 3], [3.0; 3], SCALE);
        let facts = MapFacts {
            scale: SCALE,
            cells: 2040,
        };
        assert!(GrainMap::read(&good, facts).is_some());
        for bad in [
            MapFacts {
                scale: 0.0,
                ..facts
            },
            MapFacts {
                scale: f64::NAN,
                ..facts
            },
            MapFacts { cells: 0, ..facts },
        ] {
            assert!(GrainMap::read(&good, bad).is_none(), "{bad:?}");
        }
        let eight_bit = png_of(DynamicImage::ImageRgb8(ImageBuffer::new(
            MAP_PX.0, MAP_PX.1,
        )));
        let square = png_of(DynamicImage::ImageRgb16(ImageBuffer::new(512, 512)));
        for bytes in [eight_bit, square, b"not a picture".to_vec()] {
            assert!(GrainMap::read(&bytes, facts).is_none());
        }
    }

    #[test]
    fn at_the_lattice_size_the_noise_is_the_smoothed_lattice() {
        assert!((axis_gain(2040, 2040) - 1.0).abs() < 1e-9);
        let (first, taps) = taps(7.0);
        assert_eq!(first, 5);
        let want = [0.0, SMOOTH[0], SMOOTH[1], SMOOTH[2], 0.0, 0.0];
        for (got, want) in taps.iter().zip(want) {
            assert!((got - want).abs() < 1e-12);
        }
    }

    #[test]
    fn an_enlarged_layer_is_scaled_up_and_a_shrunk_one_down() {
        // Bicubic between nodes is smoother than the nodes, so it needs more.
        for pixels in [3060, 4080, 6120, 6960] {
            let gain = axis_gain(pixels, 2040);
            assert!(gain > 1.0 && gain < 2.0, "{pixels}: {gain}");
        }
        // An area average over two nodes or more is weaker than one.
        for pixels in [1020, 680, 256] {
            let gain = axis_gain(pixels, 2040);
            assert!(gain < 1.0 && gain > 0.0, "{pixels}: {gain}");
        }
    }

    fn look_edit(grain: Value) -> Value {
        json!({
            "lutPath": provider_look::MARKER,
            "agProviderLook": { "camera": "cam1", "look": "grain-edit", "grain": grain, "before": {} },
        })
    }

    #[test]
    fn an_edit_has_grain_only_on_a_look_with_its_map_here() {
        let _turn = one_at_a_time();
        let on = look_edit(json!({ "on": true, "amount": 3 }));
        assert_eq!(Params::of(&on, Some("a.CR3")).on, 0, "no map yet");
        keep("grain-edit", flat_map(2040));
        let params = Params::of(&on, Some("a.CR3"));
        assert_eq!((params.on, params.amount), (1, 3.0));
        assert_eq!(params.seed, Params::of(&on, Some("a.CR3")).seed);
        assert_ne!(params.seed, Params::of(&on, Some("b.CR3")).seed);
        let off = look_edit(json!({ "on": false, "amount": 3 }));
        assert_eq!(Params::of(&off, Some("a.CR3")).on, 0);
        let mut taken_off = on.clone();
        taken_off["lutPath"] = json!("C:/other.cube");
        assert_eq!(Params::of(&taken_off, Some("a.CR3")).on, 0);
        let loud = look_edit(json!({ "on": true, "amount": 100 }));
        assert_eq!(Params::of(&loud, Some("a.CR3")).amount, MAX_AMOUNT);
    }

    #[test]
    fn a_virtual_copy_has_its_photos_grain() {
        let (photo, copy) = ("C:/a/b.CR3", "C:/a/b.CR3?vc=1");
        assert_eq!(parse_virtual_path(copy).0, parse_virtual_path(photo).0);
        assert_eq!(seed_of(photo), seed_of(copy));
    }

    #[test]
    fn the_map_is_made_once_and_then_read_from_the_folder() {
        let _turn = one_at_a_time();
        let folder = tempfile::tempdir().unwrap();
        let png = flat_png([3.0; 3], [3.0; 3], SCALE);
        let made = std::cell::Cell::new(0);
        let make = |output: &Path| {
            made.set(made.get() + 1);
            std::fs::write(output, &png).unwrap();
            Some(MapFacts {
                scale: SCALE,
                cells: 2040,
            })
        };
        let look = "folder-look";
        assert!(ready(folder.path(), "program\nv1\nfolder-look", look, make));
        assert!(ready(folder.path(), "program\nv1\nfolder-look", look, make));
        assert_eq!(made.get(), 1, "kept in memory");
        forget_all();
        assert!(ready(folder.path(), "program\nv1\nfolder-look", look, make));
        assert_eq!(made.get(), 1, "read from the folder");
        forget_all();
        assert!(ready(folder.path(), "program\nv2\nfolder-look", look, make));
        assert_eq!(made.get(), 2, "a new version makes its own");
        let left = std::fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(left, 4, "a PNG and its facts for each, no scratch file");
        forget_all();
        let refused = ready(folder.path(), "program\nv3\nfolder-look", look, |_| None);
        assert!(!refused && loaded(look).is_none());
    }

    // ---------------------------------------------------------- on the GPU

    /// A flat mid grey of `w` x `h` with `params`' grain, as an export renders it.
    fn render(
        ctx: &crate::image_processing::GpuContext,
        w: u32,
        h: u32,
        params: Params,
    ) -> Vec<[f32; 3]> {
        let grey = Rgba([0.5, 0.5, 0.5, 1.0]);
        let image = DynamicImage::ImageRgba32F(ImageBuffer::from_pixel(w, h, grey));
        let mut adjustments = crate::image_processing::get_all_adjustments_from_json(
            &json!({}),
            false,
            crate::as_shot::AsShot::reference(),
            Default::default(),
            None,
        );
        adjustments.global.ag_look_grain = params;
        let request = crate::gpu_processing::RenderRequest {
            adjustments,
            mask_bitmaps: &[],
            lut: None,
            roi: None,
        };
        let out = crate::mods::export_precision::render_high_precision(ctx, &image, request)
            .expect("renders")
            .to_rgba32f();
        out.pixels().map(|p| [p[0], p[1], p[2]]).collect()
    }

    /// An area shrink of `image`, `w` x `h`, to `to_w` x `to_h`, as OpenCV's.
    fn shrink(image: &[[f32; 3]], (w, h): (u32, u32), (to_w, to_h): (u32, u32)) -> Vec<[f32; 3]> {
        let along = |n: u32, to: u32| -> Vec<Vec<(usize, f64)>> {
            let s = f64::from(n) / f64::from(to);
            (0..to)
                .map(|i| {
                    let lo = f64::from(i) * s;
                    let parts = covered(lo, lo + s);
                    parts.map(|(p, part)| (p as usize, part / s)).collect()
                })
                .collect()
        };
        let (xs, ys) = (along(w, to_w), along(h, to_h));
        let mut out = Vec::with_capacity((to_w * to_h) as usize);
        for y in &ys {
            for x in &xs {
                let mut sum = [0.0f64; 3];
                for &(py, wy) in y {
                    for &(px, wx) in x {
                        let p = image[py * w as usize + px];
                        for (c, sum) in sum.iter_mut().enumerate() {
                            *sum += f64::from(p[c]) * wx * wy;
                        }
                    }
                }
                out.push(sum.map(|v| v as f32));
            }
        }
        out
    }

    /// Each channel's spread in levels out of 255, away from the edges.
    fn levels(image: &[[f32; 3]], (w, h): (u32, u32)) -> [f64; 3] {
        let (m, w, h) = (4, w as usize, h as usize);
        let inside: Vec<[f32; 3]> = (m..h - m)
            .flat_map(|y| (m..w - m).map(move |x| image[y * w + x]))
            .collect();
        let n = inside.len() as f64;
        std::array::from_fn(|c| {
            let mean = inside.iter().map(|p| f64::from(p[c])).sum::<f64>() / n;
            let var = inside
                .iter()
                .map(|p| (f64::from(p[c]) - mean).powi(2))
                .sum::<f64>()
                / n;
            var.sqrt() * 255.0
        })
    }

    fn grain_of(serial: u32, amount: f32) -> Params {
        Params {
            on: 1,
            seed: 7,
            amount,
            map: serial,
            ..Params::default()
        }
    }

    /// Of the red channels of `a` and `b`.
    fn correlation(a: &[[f32; 3]], b: &[[f32; 3]]) -> f64 {
        let red = |x: &[[f32; 3]]| x.iter().map(|p| f64::from(p[0])).collect::<Vec<f64>>();
        let (a, b) = (red(a), red(b));
        let n = a.len() as f64;
        let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
        let cov: f64 = a.iter().zip(&b).map(|(a, b)| (a - ma) * (b - mb)).sum();
        let va: f64 = a.iter().map(|a| (a - ma).powi(2)).sum();
        let vb: f64 = b.iter().map(|b| (b - mb).powi(2)).sum();
        cov / (va * vb).sqrt()
    }

    /// The brief: with the flat map, grain measures k x 3 levels at 2040 wide.
    #[test]
    fn at_2040_wide_the_grain_is_k_times_the_maps_levels() {
        let _turn = one_at_a_time();
        let ctx = device().expect("no wgpu adapter");
        keep("grain-2040", flat_map(2040));
        let serial = loaded("grain-2040").unwrap().serial;
        let size = (3060, 2040);
        let without = levels(&render(&ctx, size.0, size.1, Params::default()), size);
        assert!(without.iter().all(|l| *l < 0.05), "no grain: {without:?}");
        for amount in [1.0, 4.0] {
            let out = render(&ctx, size.0, size.1, grain_of(serial, amount));
            let measured = levels(&shrink(&out, size, (2040, 1360)), (2040, 1360));
            let want = 3.0 * f64::from(amount);
            for got in measured {
                let off = got / want - 1.0;
                assert!(
                    off.abs() < 0.1,
                    "amount {amount}: {got:.2} levels, want {want}"
                );
            }
        }
    }

    /// The same photo has the same grain on every render, and a preview has
    /// the export's: judged at the lattice's size, or at its own when smaller.
    #[test]
    fn preview_and_export_agree() {
        let _turn = one_at_a_time();
        let ctx = device().expect("no wgpu adapter");
        keep("grain-agree", flat_map(680));
        let serial = loaded("grain-agree").unwrap().serial;
        let params = grain_of(serial, 2.0);
        let size = (2040, 1360);
        let export = render(&ctx, size.0, size.1, params);
        let again = render(&ctx, size.0, size.1, params);
        assert!(export == again, "the same pattern twice");
        for preview_size in [(680, 453), (1020, 680), (340, 227)] {
            let preview = render(&ctx, preview_size.0, preview_size.1, params);
            let judged = if preview_size.0 >= 680 {
                (680, 453)
            } else {
                preview_size
            };
            let preview = shrink(&preview, preview_size, judged);
            let export = shrink(&export, size, judged);
            let (p, e) = (levels(&preview, judged), levels(&export, judged));
            for (p, e) in p.iter().zip(e) {
                let off = p / e - 1.0;
                assert!(
                    off.abs() < 0.1,
                    "{preview_size:?}: preview {p:.2}, export {e:.2}"
                );
            }
            if judged == (680, 453) {
                let alike = correlation(&preview, &export);
                assert!(alike > 0.8, "{preview_size:?}: the same grain, {alike:.2}");
            }
        }
        let other = render(&ctx, size.0, size.1, Params { seed: 8, ..params });
        let alike = correlation(&export, &other);
        assert!(alike.abs() < 0.05, "another photo, other grain: {alike:.2}");
    }
}
