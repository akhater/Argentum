use crate::cache_utils::hash_geometry;
use crate::image_processing::{
    GeometryParams, get_geometry_params_from_json, is_geometry_identity, warp_image_geometry,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, GrayImage, Luma, Rgb, Rgb32FImage, Rgba, RgbaImage};
use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

pub const SOURCE_SPACE_MAP_PREFIX: &str = "data:image/png;space=source;base64,";

const MAP_CACHE_CAPACITY: usize = 4;

static LUMA_MAP_CACHE: Mutex<Vec<(u64, Arc<GrayImage>)>> = Mutex::new(Vec::new());
static RGBA_MAP_CACHE: Mutex<Vec<(u64, Arc<RgbaImage>)>> = Mutex::new(Vec::new());

pub fn encode_source_space_map(image: &DynamicImage) -> Result<String, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}{}",
        SOURCE_SPACE_MAP_PREFIX,
        BASE64.encode(buf.get_ref())
    ))
}

pub fn is_source_space_map(data_url: &str) -> bool {
    data_url.starts_with(SOURCE_SPACE_MAP_PREFIX)
}

pub fn effect_map_key(data_url: &str, adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    data_url.hash(&mut hasher);

    if is_source_space_map(data_url) {
        hash_geometry(adjustments, &mut hasher);
    }

    hasher.finish()
}

pub fn resolve_luma_map(data_url: &str, adjustments: &serde_json::Value) -> Option<Arc<GrayImage>> {
    cached_map(
        &LUMA_MAP_CACHE,
        effect_map_key(data_url, adjustments),
        || {
            let map = decode_data_url(data_url)?;
            Some(if is_source_space_map(data_url) {
                warp_effect_map(&map, adjustments).into_luma8()
            } else {
                map.into_luma8()
            })
        },
    )
}

pub fn resolve_rgba_map(data_url: &str, adjustments: &serde_json::Value) -> Option<Arc<RgbaImage>> {
    cached_map(
        &RGBA_MAP_CACHE,
        effect_map_key(data_url, adjustments),
        || {
            let map = decode_data_url(data_url)?.into_rgba8();
            if !is_source_space_map(data_url) {
                return Some(map);
            }

            let alpha = GrayImage::from_fn(map.width(), map.height(), |x, y| {
                Luma([map.get_pixel(x, y)[3]])
            });
            let warped_rgb =
                warp_effect_map(&DynamicImage::ImageRgba8(map), adjustments).into_rgb8();
            let warped_alpha =
                warp_effect_map(&DynamicImage::ImageLuma8(alpha), adjustments).into_luma8();

            // RapidRAW f3e9fe4f: src-tauri/src/effect_maps.rs. The map's normals
            // turn with the photo, so Relight's light falls from the right side.
            let (width, height) = warped_rgb.dimensions();
            let rotations = local_warp_rotations(width, height, adjustments);

            Some(RgbaImage::from_fn(width, height, |x, y| {
                let [r, g, b] = warped_rgb.get_pixel(x, y).0;
                let (r, g) = match &rotations {
                    Some(rotations) => rotate_normal(r, g, rotations[(y * width + x) as usize]),
                    None => (r, g),
                };
                Rgba([r, g, b, warped_alpha.get_pixel(x, y)[0]])
            }))
        },
    )
}

fn effect_map_geometry_params(adjustments: &serde_json::Value) -> GeometryParams {
    let mut params = get_geometry_params_from_json(adjustments);
    params.lens_vignette_enabled = false;
    params.lens_tca_enabled = false;
    // Argentum: the camera's own shading would scale the map's values and its
    // colour fringing sample its channels at two places; only its distortion
    // moves pixels.
    if let Some(builtin) = params.lens_builtin.as_mut() {
        builtin.amounts.chromatic = 0.0;
        builtin.amounts.vignetting = 0.0;
    }
    params
}

fn warp_effect_map(map: &DynamicImage, adjustments: &serde_json::Value) -> DynamicImage {
    let params = effect_map_geometry_params(adjustments);

    if is_geometry_identity(&params) {
        map.clone()
    } else {
        warp_image_geometry(map, params)
    }
}

// RapidRAW f3e9fe4f: src-tauri/src/effect_maps.rs. How much the warp turns the
// picture at each output pixel, as (cos, sin): an image of its own source
// coordinates is warped, and the rotation part of their local slope is read.
// The third channel is 1 wherever the source was inside the picture.
fn local_warp_rotations(
    width: u32,
    height: u32,
    adjustments: &serde_json::Value,
) -> Option<Vec<(f32, f32)>> {
    let params = effect_map_geometry_params(adjustments);
    if is_geometry_identity(&params) {
        return None;
    }

    let coords = Rgb32FImage::from_fn(width, height, |x, y| Rgb([x as f32, y as f32, 1.0]));
    let warped = warp_image_geometry(&DynamicImage::ImageRgb32F(coords), params).into_rgb32f();
    let (w, h) = (width as usize, height as usize);
    let src = warped.as_raw();
    let source_at = |x: usize, y: usize| {
        let i = (y * w + x) * 3;
        [src[i], src[i + 1], src[i + 2]]
    };

    Some(
        (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let (x0, x1) = (x.saturating_sub(1), (x + 1).min(w - 1));
                let (y0, y1) = (y.saturating_sub(1), (y + 1).min(h - 1));
                if x0 == x1 || y0 == y1 {
                    return (1.0, 0.0);
                }

                let (left, right) = (source_at(x0, y), source_at(x1, y));
                let (top, bottom) = (source_at(x, y0), source_at(x, y1));
                if [left, right, top, bottom].iter().any(|p| p[2] < 0.999) {
                    return (1.0, 0.0);
                }

                let (dx, dy) = ((x1 - x0) as f32, (y1 - y0) as f32);
                let k00 = (right[0] - left[0]) / dx;
                let k10 = (right[1] - left[1]) / dx;
                let k01 = (bottom[0] - top[0]) / dy;
                let k11 = (bottom[1] - top[1]) / dy;

                let (sin, cos) = (-(k10 - k01).atan2(k00 + k11)).sin_cos();
                (cos, sin)
            })
            .collect(),
    )
}

// RapidRAW f3e9fe4f: src-tauri/src/effect_maps.rs. Turns one normal by
// (cos, sin), in the encoding `relight.rs` reads: x = r/127.5 - 1, y = 1 - g/127.5.
fn rotate_normal(r: u8, g: u8, (cos, sin): (f32, f32)) -> (u8, u8) {
    let nx = r as f32 / 127.5 - 1.0;
    let ny = 1.0 - g as f32 / 127.5;
    let rotated_x = cos * nx - sin * ny;
    let rotated_y = sin * nx + cos * ny;
    let pack = |v: f32| ((v + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
    (pack(rotated_x), pack(-rotated_y))
}

fn decode_data_url(data_url: &str) -> Option<DynamicImage> {
    let b64_data = match data_url.find(',') {
        Some(idx) => &data_url[idx + 1..],
        None => data_url,
    };
    let decoded = BASE64.decode(b64_data).ok()?;
    image::load_from_memory(&decoded).ok()
}

fn cached_map<T>(
    cache: &Mutex<Vec<(u64, Arc<T>)>>,
    key: u64,
    build: impl FnOnce() -> Option<T>,
) -> Option<Arc<T>> {
    if let Some((_, map)) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(k, _)| *k == key)
    {
        return Some(Arc::clone(map));
    }

    let map = Arc::new(build()?);

    let mut cache_lock = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache_lock.len() >= MAP_CACHE_CAPACITY {
        cache_lock.remove(0);
    }
    cache_lock.push((key, Arc::clone(&map)));

    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A normal map pointing every pixel the same way, as the normal model
    /// writes it: x = r/127.5 - 1, y = 1 - g/127.5 (down), z from b.
    fn uniform_normal_map(width: u32, height: u32, nx: f32, ny: f32) -> String {
        let pack = |v: f32| ((v + 1.0) * 127.5).round() as u8;
        let map = RgbaImage::from_pixel(width, height, Rgba([pack(nx), pack(-ny), 128, 255]));
        encode_source_space_map(&DynamicImage::ImageRgba8(map)).unwrap()
    }

    fn decode_normal(p: &Rgba<u8>) -> (f32, f32) {
        (p[0] as f32 / 127.5 - 1.0, 1.0 - p[1] as f32 / 127.5)
    }

    /// Where the warp puts a direction, found from the picture itself: a dot
    /// at the centre and one to its right, warped as the photo is, and the
    /// angle between where the two land.
    fn warped_direction_degrees(width: u32, height: u32, adjustments: &serde_json::Value) -> f32 {
        let (cx, cy) = (width / 2, height / 2);
        let dots = Rgb32FImage::from_fn(width, height, |x, y| {
            let near = |dx: u32| x.abs_diff(cx + dx) <= 1 && y.abs_diff(cy) <= 1;
            Rgb([near(0) as u8 as f32, near(20) as u8 as f32, 0.0])
        });
        let warped = warp_image_geometry(
            &DynamicImage::ImageRgb32F(dots),
            effect_map_geometry_params(adjustments),
        )
        .into_rgb32f();
        let centroid = |c: usize| {
            let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
            for (x, y, p) in warped.enumerate_pixels() {
                sx += x as f32 * p[c];
                sy += y as f32 * p[c];
                sw += p[c];
            }
            (sx / sw, sy / sw)
        };
        let ((x0, y0), (x1, y1)) = (centroid(0), centroid(1));
        (y1 - y0).atan2(x1 - x0).to_degrees()
    }

    #[test]
    fn normals_turn_with_the_photo() {
        let (width, height) = (96, 64);
        for degrees in [-30.0, 12.0, 45.0] {
            let adjustments = json!({ "transformRotate": degrees });
            let expected = warped_direction_degrees(width, height, &adjustments);
            assert!(expected.abs() > 5.0, "the warp should turn the picture");

            let url = uniform_normal_map(width, height, 1.0, 0.0);
            let map = resolve_rgba_map(&url, &adjustments).unwrap();
            let (nx, ny) = decode_normal(map.get_pixel(width / 2, height / 2));
            let got = ny.atan2(nx).to_degrees();

            assert!(
                (got - expected).abs() < 2.0,
                "rotate {degrees}: the picture turned {expected:.1} degrees, its normals {got:.1}"
            );
            assert!(((nx * nx + ny * ny).sqrt() - 1.0).abs() < 0.02);
        }
    }

    #[test]
    fn a_map_with_no_geometry_is_returned_exactly() {
        let map = RgbaImage::from_fn(40, 30, |x, y| {
            Rgba([
                (x * 6) as u8,
                (y * 8) as u8,
                (x + y) as u8,
                200 + (x % 50) as u8,
            ])
        });
        let url = encode_source_space_map(&DynamicImage::ImageRgba8(map.clone())).unwrap();
        let resolved = resolve_rgba_map(&url, &json!({})).unwrap();
        assert_eq!(resolved.as_raw(), map.as_raw());
    }

    /// Barrel distortion, a lens profile's and scale move pixels without
    /// turning them, so the normals come out as the warp alone leaves them.
    #[test]
    fn a_warp_that_does_not_turn_leaves_the_normals_alone() {
        let (width, height) = (80, 60);
        let map = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x * 3) as u8, (y * 4) as u8, 100 + (x % 40) as u8, 255])
        });
        let url = encode_source_space_map(&DynamicImage::ImageRgba8(map.clone())).unwrap();
        for adjustments in [
            json!({ "transformDistortion": 40.0 }),
            json!({ "lensDistortionParams": { "k1": 0.04, "k2": -0.01, "k3": 0.0 } }),
            json!({ "transformScale": 120.0 }),
        ] {
            let warped_only =
                warp_effect_map(&DynamicImage::ImageRgba8(map.clone()), &adjustments).into_rgb8();
            let resolved = resolve_rgba_map(&url, &adjustments).unwrap();
            for (x, y, p) in resolved.enumerate_pixels() {
                let w = warped_only.get_pixel(x, y);
                assert_eq!([p[0], p[1], p[2]], w.0, "{adjustments} at {x},{y}");
            }
        }
    }

    /// Toggling the camera's own lens correction rewarps the map, so the
    /// cache must not hand back the one warped before.
    #[test]
    fn the_cameras_lens_correction_changes_the_key() {
        let url = uniform_normal_map(8, 8, 0.0, 0.0);
        let params = json!({ "distortion": [1.0, 0.01] });
        let key = |adjustments: serde_json::Value| effect_map_key(&url, &adjustments);

        let off = key(json!({ "lensBuiltin": false, "lensBuiltinParams": params }));
        let on = key(json!({ "lensBuiltin": true, "lensBuiltinParams": params }));
        let other_params = json!({ "distortion": [1.0, 0.02] });
        let other = key(json!({ "lensBuiltin": true, "lensBuiltinParams": other_params }));
        assert_ne!(off, on);
        assert_ne!(on, other);
    }

    /// The camera's shading and colour fringing correct the photo's light, not
    /// a depth or normal map: a flat map stays flat, and its channels move
    /// together.
    #[test]
    fn the_cameras_shading_and_fringing_leave_a_map_alone() {
        use argentum_engine::rm_lens_builtin::{LensCorrection, Radial};
        let radial = |values: Vec<f32>| Radial {
            knots: vec![0.0, 1.0],
            values,
        };
        let correction = LensCorrection {
            vignetting: Some(radial(vec![1.0, 1.8])),
            chromatic: Some([radial(vec![1.0, 1.01]), radial(vec![1.0, 0.99])]),
            ..Default::default()
        };
        let adjustments = json!({ "lensBuiltin": true, "lensBuiltinParams": correction });
        let (width, height) = (60, 40);
        // The warp's last column and row sample past the edge.
        let inside = |x: u32, y: u32| x + 1 < width && y + 1 < height;

        let flat = GrayImage::from_pixel(width, height, Luma([100]));
        let url = encode_source_space_map(&DynamicImage::ImageLuma8(flat)).unwrap();
        let depth = resolve_luma_map(&url, &adjustments).unwrap();
        for (x, y, p) in depth.enumerate_pixels().filter(|(x, y, _)| inside(*x, *y)) {
            assert!(p[0].abs_diff(100) <= 1, "at {x},{y}: {}", p[0]);
        }

        let ramp = RgbaImage::from_fn(width, height, |x, _| {
            let v = (x * 4) as u8;
            Rgba([v, v, v, 255])
        });
        let url = encode_source_space_map(&DynamicImage::ImageRgba8(ramp)).unwrap();
        let normals = resolve_rgba_map(&url, &adjustments).unwrap();
        for (x, y, p) in normals.enumerate_pixels().filter(|(x, y, _)| inside(*x, *y)) {
            assert!(p[0].abs_diff(p[2]) <= 1, "at {x},{y}: {p:?}");
        }
    }

    #[test]
    fn no_turn_is_lossless() {
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                assert_eq!(rotate_normal(r, g, (1.0, 0.0)), (r, g));
                assert_eq!(rotate_normal(r, g, (1.0, -0.0)), (r, g));
            }
        }
    }
}
