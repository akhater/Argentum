//! What the integration tests share: local RAWs from the environment, a GPU
//! if there is one, synthetic sensor data for everything else, and the
//! app's own shader functions to compare with (`app_shader`).

#![allow(dead_code)]

pub mod app_shader;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use argentum_engine::rawler::cfa::{CFA, PlaneColor};
use argentum_engine::rawler::decoders::{Camera, Orientation};
use argentum_engine::rawler::rawimage::{
    BlackLevel, CFAConfig, RawImage, RawImageData, RawPhotometricInterpretation, WhiteLevel,
};
use argentum_engine::{Gpu, Source};

/// The RAWs named by `AG_RAWS` (paths separated by `;`) and `AG_RAW`, as the
/// app's own tests name theirs. Empty, with a line saying how to run it, when
/// neither is set.
pub fn raws(test: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var("AG_RAWS")
        .unwrap_or_default()
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    if let Ok(one) = std::env::var("AG_RAW")
        && !one.trim().is_empty()
        && !out.contains(&PathBuf::from(&one))
    {
        out.push(PathBuf::from(one));
    }
    if out.is_empty() {
        eprintln!(
            "{test}: SKIPPED on real photos - set AG_RAW to a RAW file, or AG_RAWS to several separated by ';'"
        );
    }
    out
}

/// The first of `raws`.
pub fn raw(test: &str) -> Option<PathBuf> {
    raws(test).into_iter().next()
}

/// A GPU of the engine's own, or `None` with a line saying the test skipped.
pub fn gpu(test: &str) -> Option<Arc<Gpu>> {
    match Gpu::new() {
        Ok(gpu) => {
            eprintln!(
                "{test}: GPU {} ({:?})",
                gpu.adapter.name, gpu.adapter.backend
            );
            Some(Arc::new(gpu))
        }
        Err(e) => {
            eprintln!("{test}: SKIPPED - no GPU adapter ({e})");
            None
        }
    }
}

pub fn open(path: &PathBuf) -> Source {
    Source::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A small RGGB mosaic with sensor levels: black 512, white 15000, and
/// `value(row, col)` at each photosite. No colour matrix, so the develop
/// leaves the colours as the sensor's.
pub fn bayer(width: usize, height: usize, value: impl Fn(usize, usize) -> u16) -> RawImage {
    let cfa = CFA::new("RGGB");
    let camera = Camera {
        cfa: cfa.clone(),
        ..Default::default()
    };
    let data = (0..height)
        .flat_map(|r| (0..width).map(move |c| (r, c)))
        .map(|(r, c)| value(r, c))
        .collect();
    RawImage {
        camera,
        make: "Synthetic".into(),
        model: "Bayer".into(),
        clean_make: "Synthetic".into(),
        clean_model: "Bayer".into(),
        width,
        height,
        cpp: 1,
        bps: 14,
        wb_coeffs: [2.0, 1.0, 1.5, f32::NAN],
        whitelevel: WhiteLevel::new(vec![15000]),
        blacklevel: BlackLevel::new(&[512u32], 1, 1, 1),
        xyz_to_cam: [[0.0; 3]; 4],
        photometric: RawPhotometricInterpretation::Cfa(CFAConfig::new(
            &cfa,
            &PlaneColor::new("RGB"),
        )),
        active_area: None,
        crop_area: None,
        blackareas: Vec::new(),
        orientation: Orientation::Normal,
        data: RawImageData::Integer(data),
        color_matrix: HashMap::new(),
        dng_tags: HashMap::new(),
    }
}

/// A test scene on a mosaic: a smooth gradient, a few sharp edges, some
/// pseudo-random texture, and a sun-like disc that clips - green first, as a
/// daylight frame does, with a core where everything clips.
pub fn scene(width: usize, height: usize) -> RawImage {
    let (cx, cy) = (width as f32 * 0.7, height as f32 * 0.35);
    let r = width.min(height) as f32 * 0.18;
    bayer(width, height, move |row, col| {
        let colour = match (row % 2, col % 2) {
            (0, 0) => 0,
            (1, 1) => 2,
            _ => 1,
        };
        let x = col as f32 / width as f32;
        let y = row as f32 / height as f32;
        let mut light = 0.08 + 0.25 * x + 0.1 * y;
        if (col / 24 + row / 24) % 2 == 0 {
            light += 0.12;
        }
        // Texture: a cheap hash, a few percent.
        let n = ((row * 7919 + col * 104_729) % 1013) as f32 / 1013.0;
        light += 0.03 * n;
        let d = ((col as f32 - cx).powi(2) + (row as f32 - cy).powi(2)).sqrt();
        if d < r {
            light += 2.0 * (1.0 - d / r) + 0.5;
        }
        // Per-colour sensitivity, green the strongest: green clips first.
        let gain = [0.55, 1.0, 0.7][colour];
        let v = 512.0 + light * gain * 14488.0;
        v.min(15000.0) as u16
    })
}

/// A synthetic photo, keyed by its size.
pub fn source(width: usize, height: usize) -> Source {
    Source::new((width * 31 + height) as u64, scene(width, height))
}

/// Colours for the parity tests: greys, primaries, skin and sky, values from
/// black to far above white, a few below zero as a colour matrix can leave
/// them, and pseudo-random ones in between. One row, every value one a half
/// float holds exactly: the pixel pass's input is half floats, as the app's
/// is, so both sides see the same numbers.
pub fn colours() -> argentum_engine::Rgb {
    let mut seed = 0x2545_f491u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut pixels: Vec<[f32; 3]> = vec![
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.18, 0.18, 0.18],
        [0.001, 0.001, 0.001],
        [f32::MIN_POSITIVE, 1e-30, 1e-7],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.8, 0.55, 0.4],
        [0.45, 0.3, 0.22],
        [0.1, 0.35, 1.0],
        [1.0, 0.45, 0.05],
        [3.0, 2.5, 1.0],
        [40.0, 38.0, 41.0],
        [1000.0, 999.9, 0.18],
        [-0.02, 0.3, 0.5],
        [0.6, -0.01, 0.2],
    ];
    for _ in 0..4000 {
        let scale = [0.05, 0.3, 1.0, 4.0][(next() * 4.0) as usize % 4];
        pixels.push([next() * scale, next() * scale, next() * scale]);
    }
    for _ in 0..1000 {
        let v = next().powi(3);
        pixels.push([v, v * (0.9 + 0.2 * next()), v * (0.9 + 0.2 * next())]);
    }
    let n = pixels.len();
    argentum_engine::Rgb::new(n, 1, pixels.into_iter().map(half_exact).collect())
}

/// `c` as a half float holds it.
pub fn half_exact(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| half::f16::from_f32(v).to_f32())
}

/// `rgb` as the pixel shader's test entry takes it, one pixel per element.
pub fn elements(rgb: &argentum_engine::Rgb) -> Vec<[f32; 4]> {
    rgb.pixels.iter().map(|p| [p[0], p[1], p[2], 0.0]).collect()
}

/// A pipeline running only `T`, on this GPU, with `patch` applied: the
/// engine's side of a parity test, through the pixel pass.
pub fn only<T: argentum_engine::Tool>(
    gpu: Arc<Gpu>,
    patch: serde_json::Value,
) -> argentum_engine::Pipeline {
    let registry = argentum_engine::Registry::new().with::<T>();
    let mut p = argentum_engine::Pipeline::with_gpu(
        Arc::new(registry),
        argentum_engine::GpuSlot::with(gpu),
    )
    .unwrap();
    let mut edit = serde_json::Map::new();
    edit.insert(T::ID.to_string(), patch);
    p.apply(&serde_json::from_value(serde_json::Value::Object(edit)).unwrap())
        .unwrap();
    p
}

/// How far apart two pictures are once on screen: the largest difference of
/// any channel after the sRGB curve, in 8-bit levels.
pub fn worst_levels(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    use argentum_engine::colour::srgb_encode;
    a.iter()
        .zip(b)
        .flat_map(|(x, y)| (0..3).map(move |c| (srgb_encode(x[c]) - srgb_encode(y[c])).abs()))
        .fold(0.0, f32::max)
        * 255.0
}
