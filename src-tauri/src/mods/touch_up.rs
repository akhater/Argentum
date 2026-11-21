//! Liquify and skin smoothing (Retouch): what a brush stroke does to the
//! pixels under it. RapidRAW's.
//!
//! Copied from RapidRAW f3c68838 ("rework liquify and retouch with
//! transform-aware strokes, composed displacement warping and frequency
//! separation smoothing"): `src-tauri/src/touch_up.rs`, the whole file (at
//! v1.7.0 `src-tauri/src/retouch/touch_up.rs`, moved in ad1dc9b9; d33b3e7d
//! only made `box_mean` public). The stroke reading half of `inpainting.rs`
//! `generate_liquify_patch` is `liquify_strokes` here.
//!
//! WHY
//!
//! 1.6.6 read the stroke points as raw pixels, but the canvas records them on
//! the photo as shown: after a straighten, rotation, flip, crop-free
//! perspective or lens correction, liquify and skin smoothing landed away from
//! the brush. Every point now goes through the photo's geometry backwards
//! (`inverse_transform_point`, as Heal's source point already did), the brush
//! radius with the local scale and a mirrored photo's twirl with its
//! handedness. Liquify strokes are cut into dabs and composed into one
//! displacement field, so a stroke over an already pushed area moves what is
//! now visible there (1.6.6 summed every segment at the original pixel). Skin
//! smoothing is frequency separation (a guided filter on the low frequencies,
//! most texture kept) instead of a bilateral blur.
//!
//! Changes from the source: `liquify_strokes` moved here from `inpainting.rs`
//! so it can be tested without the app's state.

use image::{GrayImage, RgbImage};
use rayon::prelude::*;

use super::clone_heal::Bounds;
use crate::mask_generation::SubMask;

// RapidRAW f3c68838: src-tauri/src/touch_up.rs
const DAB_SPACING: f32 = 0.15;
const CLICK_AMOUNT: f32 = 0.45;
const PUSH_STRENGTH: f32 = 0.6;
const PINCH_STRENGTH: f32 = 0.45;
const EXPAND_STRENGTH: f32 = 0.3;
const TWIRL_STRENGTH: f32 = 1.4;
const RECONSTRUCT_STRENGTH: f32 = 5.0;

#[derive(Clone, Copy, PartialEq)]
pub enum LiquifyMode {
    Push,
    Pinch,
    Expand,
    Twirl,
    Reconstruct,
}

impl LiquifyMode {
    pub fn from_name(name: &str) -> Self {
        match name {
            "pinch" => Self::Pinch,
            "expand" => Self::Expand,
            "twirl" => Self::Twirl,
            "reconstruct" => Self::Reconstruct,
            _ => Self::Push,
        }
    }
}

pub struct LiquifyPoint {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub handedness: f32,
}

pub struct LiquifyStroke {
    pub points: Vec<LiquifyPoint>,
    pub mode: LiquifyMode,
    pub pressure: f32,
    pub feather: f32,
}

struct Dab {
    x: f32,
    y: f32,
    step: (f32, f32),
    radius: f32,
    inner_radius: f32,
    amount: f32,
    strength: f32,
    handedness: f32,
    mode: LiquifyMode,
}

fn stroke_dabs(stroke: &LiquifyStroke) -> Vec<Dab> {
    let dab = |x: f32, y: f32, radius: f32, handedness: f32, step: (f32, f32), amount: f32| Dab {
        x,
        y,
        step,
        radius,
        inner_radius: radius * (1.0 - stroke.feather.clamp(0.01, 1.0)),
        amount,
        strength: stroke.pressure,
        handedness,
        mode: stroke.mode,
    };
    let points = &stroke.points;
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };

    let mut dabs = Vec::new();
    let mut previous = (first.x, first.y);
    let mut travelled = 0.0_f32;
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length < 1e-3 {
            continue;
        }
        let mut s = 0.0_f32;
        loop {
            let radius = a.radius + (b.radius - a.radius) * (s / length);
            let spacing = (DAB_SPACING * radius).max(0.5);
            if s + spacing - travelled > length {
                travelled += length - s;
                break;
            }
            s += (spacing - travelled).max(0.0);
            travelled = 0.0;
            let t = s / length;
            let (x, y) = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let radius = a.radius + (b.radius - a.radius) * t;
            dabs.push(dab(
                x,
                y,
                radius,
                b.handedness,
                (x - previous.0, y - previous.1),
                DAB_SPACING,
            ));
            previous = (x, y);
        }
    }

    if dabs.is_empty() {
        let step = (last.x - first.x, last.y - first.y);
        let amount = if points.len() == 1 {
            CLICK_AMOUNT
        } else {
            (step.0.hypot(step.1) / last.radius.max(1.0)).clamp(0.05, CLICK_AMOUNT)
        };
        dabs.push(dab(
            last.x,
            last.y,
            last.radius,
            last.handedness,
            step,
            amount,
        ));
    }
    dabs
}

fn falloff(dist: f32, radius: f32, inner_radius: f32) -> f32 {
    if dist <= inner_radius {
        return 1.0;
    }
    let t = (dist - inner_radius) / (radius - inner_radius).max(1e-3);
    let c = 0.5 * (1.0 + (t * std::f32::consts::PI).cos());
    c * c
}

struct DisplacementField {
    x0: f32,
    y0: f32,
    spacing: f32,
    cols: usize,
    rows: usize,
    data: Vec<[f32; 2]>,
}

impl DisplacementField {
    fn node(&self, i: i64, j: i64) -> [f32; 2] {
        if i < 0 || j < 0 || i >= self.cols as i64 || j >= self.rows as i64 {
            return [0.0, 0.0];
        }
        self.data[j as usize * self.cols + i as usize]
    }

    fn sample(&self, x: f32, y: f32) -> [f32; 2] {
        let gx = (x - self.x0) / self.spacing;
        let gy = (y - self.y0) / self.spacing;
        let (i, j) = (gx.floor() as i64, gy.floor() as i64);
        let (tx, ty) = (gx - i as f32, gy - j as f32);
        let (a, b) = (self.node(i, j), self.node(i + 1, j));
        let (c, d) = (self.node(i, j + 1), self.node(i + 1, j + 1));
        let lerp = |k: usize| {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            top + (bottom - top) * ty
        };
        [lerp(0), lerp(1)]
    }

    fn displaced(&self, dab: &Dab, i: usize, j: usize) -> [f32; 2] {
        let current = self.data[j * self.cols + i];
        let px = self.x0 + i as f32 * self.spacing;
        let py = self.y0 + j as f32 * self.spacing;
        let (rx, ry) = (px - dab.x, py - dab.y);
        let dist = rx.hypot(ry);
        if dist >= dab.radius {
            return current;
        }
        let weight = falloff(dist, dab.radius, dab.inner_radius) * dab.strength;
        let offset = match dab.mode {
            LiquifyMode::Push => (
                -dab.step.0 * weight * PUSH_STRENGTH,
                -dab.step.1 * weight * PUSH_STRENGTH,
            ),
            LiquifyMode::Pinch => {
                let s = weight * dab.amount * PINCH_STRENGTH;
                (rx * s, ry * s)
            }
            LiquifyMode::Expand => {
                let s = -weight * dab.amount * EXPAND_STRENGTH;
                (rx * s, ry * s)
            }
            LiquifyMode::Twirl => {
                let angle = weight * dab.amount * TWIRL_STRENGTH * dab.handedness;
                let (sin, cos) = angle.sin_cos();
                (rx * cos - ry * sin - rx, rx * sin + ry * cos - ry)
            }
            LiquifyMode::Reconstruct => {
                let keep = 1.0 - (weight * dab.amount * RECONSTRUCT_STRENGTH).min(1.0);
                return [current[0] * keep, current[1] * keep];
            }
        };
        let previous = self.sample(px + offset.0, py + offset.1);
        [offset.0 + previous[0], offset.1 + previous[1]]
    }

    fn apply(&mut self, dab: &Dab) {
        let to_index = |v: f32, origin: f32, max: usize| {
            (((v - origin) / self.spacing).max(0.0) as usize).min(max - 1)
        };
        let i0 = to_index(dab.x - dab.radius, self.x0, self.cols);
        let i1 = to_index(dab.x + dab.radius + self.spacing, self.x0, self.cols);
        let j0 = to_index(dab.y - dab.radius, self.y0, self.rows);
        let j1 = to_index(dab.y + dab.radius + self.spacing, self.y0, self.rows);
        let field = &*self;
        let updated: Vec<[f32; 2]> = (j0..=j1)
            .into_par_iter()
            .flat_map_iter(|j| (i0..=i1).map(move |i| field.displaced(dab, i, j)))
            .collect();
        let width = i1 - i0 + 1;
        for (row, values) in updated.chunks(width).enumerate() {
            let start = (j0 + row) * self.cols + i0;
            self.data[start..start + width].copy_from_slice(values);
        }
    }
}

fn bicubic_sample(image: &RgbImage, x: f32, y: f32) -> [u8; 3] {
    let weights = |t: f32| {
        let (t2, t3) = (t * t, t * t * t);
        [
            0.5 * (-t3 + 2.0 * t2 - t),
            0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
            0.5 * (-3.0 * t3 + 4.0 * t2 + t),
            0.5 * (t3 - t2),
        ]
    };
    let (xf, yf) = (x.floor(), y.floor());
    let (wx, wy) = (weights(x - xf), weights(y - yf));
    let max_x = image.width() as i64 - 1;
    let max_y = image.height() as i64 - 1;
    let mut sum = [0.0_f32; 3];
    for (dy, weight_y) in wy.iter().enumerate() {
        let sy = (yf as i64 + dy as i64 - 1).clamp(0, max_y) as u32;
        for (dx, weight_x) in wx.iter().enumerate() {
            let sx = (xf as i64 + dx as i64 - 1).clamp(0, max_x) as u32;
            let weight = weight_x * weight_y;
            for (acc, &value) in sum.iter_mut().zip(image.get_pixel(sx, sy).0.iter()) {
                *acc += value as f32 * weight;
            }
        }
    }
    sum.map(|v| v.round().clamp(0.0, 255.0) as u8)
}

pub fn liquify_patch(
    image: &RgbImage,
    strokes: &[LiquifyStroke],
) -> Option<(RgbImage, GrayImage, (u32, u32))> {
    let dabs: Vec<Dab> = strokes.iter().flat_map(stroke_dabs).collect();
    let min_radius = dabs.iter().map(|d| d.radius).reduce(f32::min)?;
    let (img_w, img_h) = (image.width() as f32, image.height() as f32);
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for dab in &dabs {
        (min_x, max_x) = (min_x.min(dab.x - dab.radius), max_x.max(dab.x + dab.radius));
        (min_y, max_y) = (min_y.min(dab.y - dab.radius), max_y.max(dab.y + dab.radius));
    }
    let (min_x, min_y) = (min_x.floor().max(0.0), min_y.floor().max(0.0));
    let (max_x, max_y) = (max_x.ceil().min(img_w - 1.0), max_y.ceil().min(img_h - 1.0));
    if min_x > max_x || min_y > max_y {
        return None;
    }

    let spacing = (min_radius / 6.0).clamp(1.0, 4.0);
    let (x0, y0) = (min_x - 2.0 * spacing, min_y - 2.0 * spacing);
    let cols = ((max_x - min_x) / spacing).ceil() as usize + 5;
    let rows = ((max_y - min_y) / spacing).ceil() as usize + 5;
    let mut field = DisplacementField {
        x0,
        y0,
        spacing,
        cols,
        rows,
        data: vec![[0.0, 0.0]; cols * rows],
    };
    for dab in &dabs {
        field.apply(dab);
    }

    let (left, top) = (min_x as u32, min_y as u32);
    let width = max_x as u32 - left + 1;
    let height = max_y as u32 - top + 1;
    let mut color = vec![0u8; (width * height * 3) as usize];
    let mut mask = vec![0u8; (width * height) as usize];
    color
        .par_chunks_mut(width as usize * 3)
        .zip(mask.par_chunks_mut(width as usize))
        .enumerate()
        .for_each(|(row, (color_row, mask_row))| {
            let y = top + row as u32;
            for (col, mask_value) in mask_row.iter_mut().enumerate() {
                let x = left + col as u32;
                let [dx, dy] = field.sample(x as f32, y as f32);
                let magnitude = dx.hypot(dy);
                let rgb = if magnitude > 1e-3 {
                    *mask_value = (magnitude * 255.0).min(255.0) as u8;
                    bicubic_sample(image, x as f32 + dx, y as f32 + dy)
                } else {
                    image.get_pixel(x, y).0
                };
                color_row[col * 3..col * 3 + 3].copy_from_slice(&rgb);
            }
        });

    Some((
        RgbImage::from_raw(width, height, color)?,
        GrayImage::from_raw(width, height, mask)?,
        (left, top),
    ))
}

fn box_mean(src: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut horizontal = vec![0.0_f32; src.len()];
    horizontal
        .par_chunks_mut(width)
        .zip(src.par_chunks(width))
        .for_each(|(out, row)| {
            let mut prefix = Vec::with_capacity(width + 1);
            prefix.push(0.0_f64);
            for &v in row {
                prefix.push(prefix[prefix.len() - 1] + v as f64);
            }
            for (x, value) in out.iter_mut().enumerate() {
                let (lo, hi) = (x.saturating_sub(radius), (x + radius + 1).min(width));
                *value = ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32;
            }
        });

    let mut out = vec![0.0_f32; src.len()];
    let mut column_sums = vec![0.0_f64; width];
    for row in horizontal.chunks(width).take(radius.min(height)) {
        for (sum, &v) in column_sums.iter_mut().zip(row) {
            *sum += v as f64;
        }
    }
    for (y, out_row) in out.chunks_mut(width).enumerate() {
        if y + radius < height {
            let row = &horizontal[(y + radius) * width..(y + radius + 1) * width];
            for (sum, &v) in column_sums.iter_mut().zip(row) {
                *sum += v as f64;
            }
        }
        if y > radius {
            let row = &horizontal[(y - radius - 1) * width..(y - radius) * width];
            for (sum, &v) in column_sums.iter_mut().zip(row) {
                *sum -= v as f64;
            }
        }
        let count = ((y + radius + 1).min(height) - y.saturating_sub(radius)) as f64;
        for (value, &sum) in out_row.iter_mut().zip(&column_sums) {
            *value = (sum / count) as f32;
        }
    }
    out
}

fn gaussian_blur(src: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil() as i64;
    let kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / total).collect();
    let convolve = |src: &[f32], horizontal: bool| {
        let mut out = vec![0.0_f32; src.len()];
        out.par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, out_row)| {
                for (x, value) in out_row.iter_mut().enumerate() {
                    *value = kernel
                        .iter()
                        .enumerate()
                        .map(|(k, weight)| {
                            let offset = k as i64 - radius;
                            let (sx, sy) = if horizontal {
                                ((x as i64 + offset).clamp(0, width as i64 - 1), y as i64)
                            } else {
                                (x as i64, (y as i64 + offset).clamp(0, height as i64 - 1))
                            };
                            src[sy as usize * width + sx as usize] * weight
                        })
                        .sum();
                }
            });
        out
    };
    convolve(&convolve(src, true), false)
}

fn guided_smooth(src: &[f32], width: usize, height: usize, radius: usize, eps: f32) -> Vec<f32> {
    let (a, b): (Vec<f32>, Vec<f32>) = {
        let mean = box_mean(src, width, height, radius);
        let squares: Vec<f32> = src.par_iter().map(|v| v * v).collect();
        let mean_sq = box_mean(&squares, width, height, radius);
        mean.par_iter()
            .zip(&mean_sq)
            .map(|(&m, &sq)| {
                let variance = (sq - m * m).max(0.0);
                let a = variance / (variance + eps);
                (a, m * (1.0 - a))
            })
            .unzip()
    };
    let mean_a = box_mean(&a, width, height, radius);
    drop(a);
    let mean_b = box_mean(&b, width, height, radius);
    drop(b);
    src.par_iter()
        .zip(&mean_a)
        .zip(&mean_b)
        .map(|((&v, &a), &b)| a * v + b)
        .collect()
}

pub fn retouch_patch(
    image: &RgbImage,
    bounds: Bounds,
    brush_radius: f32,
    strength: f32,
) -> RgbImage {
    let strength = strength.clamp(0.0, 1.0);
    let amount = 1.0 - (1.0 - strength).powi(3);
    let radius = (brush_radius * (0.03 + 0.08 * strength))
        .clamp(2.0, 48.0)
        .round() as usize;
    let sigma = (radius as f32 * 0.15).clamp(0.8, 3.0);
    let eps = (0.008 + 0.032 * strength).powi(2);
    let texture = 1.0 - 0.3 * strength;

    let (min_x, max_x, min_y, max_y) = bounds;
    let pad = 2 * radius + (3.0 * sigma).ceil() as usize + 1;
    let left = min_x.saturating_sub(pad);
    let top = min_y.saturating_sub(pad);
    let right = (max_x + pad).min(image.width() as usize - 1);
    let bottom = (max_y + pad).min(image.height() as usize - 1);
    let (width, height) = (right - left + 1, bottom - top + 1);

    let channels: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            let src: Vec<f32> = (0..width * height)
                .into_par_iter()
                .map(|i| {
                    image.get_pixel((left + i % width) as u32, (top + i / width) as u32)[c] as f32
                        / 255.0
                })
                .collect();
            let low = gaussian_blur(&src, width, height, sigma);
            let smooth = guided_smooth(&low, width, height, radius, eps);
            src.iter()
                .zip(&low)
                .zip(&smooth)
                .map(|((&v, &l), &s)| v + (s + (v - l) * texture - v) * amount)
                .collect()
        })
        .collect();

    let (out_w, out_h) = (max_x - min_x + 1, max_y - min_y + 1);
    let mut out = vec![0u8; out_w * out_h * 3];
    out.par_chunks_mut(out_w * 3)
        .enumerate()
        .for_each(|(row, chunk)| {
            let base = (min_y - top + row) * width + (min_x - left);
            for (col, pixel) in chunk.chunks_mut(3).enumerate() {
                for (value, channel) in pixel.iter_mut().zip(&channels) {
                    *value = (channel[base + col] * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
    RgbImage::from_raw(out_w as u32, out_h as u32, out).expect("out_w * out_h * 3 bytes")
}

// RapidRAW f3c68838: src-tauri/src/inpainting.rs (`line_points`, `line_radius`,
// `is_eraser_line` and the stroke half of `generate_liquify_patch`)

fn line_points(line: &serde_json::Value) -> Vec<(f64, f64)> {
    line.get("points")
        .and_then(|v| v.as_array())
        .map(|points| {
            points
                .iter()
                .filter_map(|p| Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Half the line's `brushSize`, in pixels of the photo as shown.
pub fn line_radius(line: &serde_json::Value) -> f32 {
    line.get("brushSize")
        .and_then(|v| v.as_f64())
        .unwrap_or(50.0) as f32
        / 2.0
}

pub fn is_eraser_line(line: &serde_json::Value) -> bool {
    line.get("tool").and_then(|v| v.as_str()) == Some("eraser")
}

/// The liquify strokes of a patch's sub-masks, in raw pixels. `to_raw` maps a
/// point of the photo as shown (where the canvas records strokes) back to the
/// raw image: `inverse_transform_point` with the photo's adjustments.
pub fn liquify_strokes(
    sub_masks: &[SubMask],
    to_raw: impl Fn(f64, f64) -> (f64, f64),
) -> Vec<LiquifyStroke> {
    let mut strokes = Vec::new();
    for sm in sub_masks
        .iter()
        .filter(|sm| sm.mask_type.eq_ignore_ascii_case("liquify"))
    {
        let params = &sm.parameters;
        let pressure = (params
            .get("pressure")
            .and_then(|v| v.as_f64())
            .unwrap_or(50.0) as f32
            / 100.0)
            .clamp(0.01, 1.0);
        let layer_mode = LiquifyMode::from_name(
            params
                .get("liquifyMode")
                .and_then(|v| v.as_str())
                .unwrap_or("push"),
        );
        let lines = params.get("lines").and_then(|v| v.as_array());

        for line in lines.into_iter().flatten() {
            let mode = if is_eraser_line(line) {
                LiquifyMode::Reconstruct
            } else {
                layer_mode
            };
            let radius = line_radius(line);
            let points = line_points(line)
                .into_iter()
                .map(|(x, y)| {
                    let c = to_raw(x, y);
                    let ex = to_raw(x + 1.0, y);
                    let ey = to_raw(x, y + 1.0);
                    let det = (ex.0 - c.0) * (ey.1 - c.1) - (ex.1 - c.1) * (ey.0 - c.0);
                    LiquifyPoint {
                        x: c.0 as f32,
                        y: c.1 as f32,
                        radius: radius * det.abs().sqrt() as f32,
                        handedness: if det < 0.0 { -1.0 } else { 1.0 },
                    }
                })
                .collect();
            strokes.push(LiquifyStroke {
                points,
                mode,
                pressure,
                feather: line.get("feather").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
            });
        }
    }
    strokes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask_generation::SubMaskMode;
    use image::Rgb;
    use serde_json::json;

    fn liquify_layer(mode: &str, lines: serde_json::Value) -> SubMask {
        serde_json::from_value(json!({
            "id": "l",
            "type": "liquify",
            "visible": true,
            "mode": "additive",
            "parameters": { "liquifyMode": mode, "pressure": 50, "lines": lines },
        }))
        .expect("a liquify sub-mask")
    }

    fn line(points: &[(f64, f64)], brush_size: f64) -> serde_json::Value {
        json!({
            "tool": "brush",
            "brushSize": brush_size,
            "feather": 0.5,
            "points": points.iter().map(|&(x, y)| json!({ "x": x, "y": y })).collect::<Vec<_>>(),
        })
    }

    /// The canvas records a stroke on the photo as shown. On a straightened
    /// photo, 1.6.6 used that point as a raw pixel and liquified somewhere
    /// else; the stroke must land where the photo's geometry puts it.
    #[test]
    fn a_dab_on_a_rotated_photo_lands_under_the_brush() {
        let adjustments = json!({ "rotation": 10.0 });
        let (w, h) = (6000.0, 4000.0);
        let to_raw = |x: f64, y: f64| {
            crate::image_processing::inverse_transform_point(x, y, w, h, &adjustments)
        };
        let brushed = (1000.0, 800.0);
        let target = to_raw(brushed.0, brushed.1);
        let radius = 50.0;

        // 1.6.6 put the dab at the brushed point itself: off by eight radii.
        let miss = (target.0 - brushed.0).hypot(target.1 - brushed.1);
        assert!(miss > 8.0 * radius, "1.6.6 missed by {miss:.0} px");

        let layer = liquify_layer("push", json!([line(&[brushed], 2.0 * radius)]));
        let strokes = liquify_strokes(std::slice::from_ref(&layer), to_raw);
        let dab = &strokes[0].points[0];
        assert!((dab.x as f64 - target.0).abs() < 1e-3);
        assert!((dab.y as f64 - target.1).abs() < 1e-3);
        // A rotation keeps scale and handedness.
        assert!((dab.radius - radius as f32).abs() < 1e-3);
        assert_eq!(dab.handedness, 1.0);
    }

    #[test]
    fn a_mirrored_photo_twirls_the_way_it_is_brushed() {
        let adjustments = json!({ "flipHorizontal": true });
        let to_raw = |x: f64, y: f64| {
            crate::image_processing::inverse_transform_point(x, y, 400.0, 300.0, &adjustments)
        };
        let layer = liquify_layer("twirl", json!([line(&[(100.0, 100.0)], 40.0)]));
        let strokes = liquify_strokes(std::slice::from_ref(&layer), to_raw);
        assert_eq!(strokes[0].points[0].x, 300.0);
        assert_eq!(strokes[0].points[0].handedness, -1.0);
    }

    #[test]
    fn an_eraser_line_reconstructs() {
        let mut eraser = line(&[(10.0, 10.0)], 20.0);
        eraser["tool"] = json!("eraser");
        let layer = liquify_layer("pinch", json!([eraser]));
        let strokes = liquify_strokes(std::slice::from_ref(&layer), |x, y| (x, y));
        assert!(strokes[0].mode == LiquifyMode::Reconstruct);
        assert_eq!(SubMaskMode::Additive, layer.mode);
    }

    fn stripes(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, _| {
            if (x / 8) % 2 == 0 {
                Rgb([200, 200, 200])
            } else {
                Rgb([40, 40, 40])
            }
        })
    }

    #[test]
    fn a_push_moves_pixels_around_the_dab_only() {
        let img = stripes(200, 200);
        let stroke = LiquifyStroke {
            points: [(90.0, 100.0), (110.0, 100.0)]
                .map(|(x, y)| LiquifyPoint {
                    x,
                    y,
                    radius: 20.0,
                    handedness: 1.0,
                })
                .into(),
            mode: LiquifyMode::Push,
            pressure: 1.0,
            feather: 0.5,
        };
        let (color, mask, (left, top)) =
            liquify_patch(&img, std::slice::from_ref(&stroke)).expect("a patch");
        // The patch covers the stroke and its radius, nothing more.
        // (dabs start one spacing, 3 px, into the stroke)
        assert!(
            (69..=74).contains(&left) && (79..=81).contains(&top),
            "{left},{top}"
        );
        assert!(color.width() <= 63 && color.height() <= 43);
        // Something moved under the stroke, and the mask says where.
        let moved = color
            .enumerate_pixels()
            .filter(|(x, y, p)| img.get_pixel(x + left, y + top) != *p)
            .count();
        assert!(moved > 100, "{moved} pixels moved");
        assert!(mask.get_pixel(100 - left, 100 - top)[0] > 0);
        assert_eq!(mask.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn a_second_push_carries_what_the_first_one_moved() {
        // Composed: pushing again from where the first push ended moves the
        // same content further, where a sum at the original pixel would not.
        let push = |from: f32, to: f32| LiquifyStroke {
            points: [(from, 100.0), (to, 100.0)]
                .map(|(x, y)| LiquifyPoint {
                    x,
                    y,
                    radius: 30.0,
                    handedness: 1.0,
                })
                .into(),
            mode: LiquifyMode::Push,
            pressure: 1.0,
            feather: 0.5,
        };
        let one = [push(80.0, 100.0)];
        let two = [push(80.0, 100.0), push(100.0, 120.0)];
        let shift = |strokes: &[LiquifyStroke]| {
            let dabs: Vec<Dab> = strokes.iter().flat_map(stroke_dabs).collect();
            let mut field = DisplacementField {
                x0: 0.0,
                y0: 0.0,
                spacing: 1.0,
                cols: 240,
                rows: 200,
                data: vec![[0.0, 0.0]; 240 * 200],
            };
            for dab in &dabs {
                field.apply(dab);
            }
            -field.sample(120.0, 100.0)[0]
        };
        assert!(shift(&two) > shift(&one) + 5.0);
    }

    #[test]
    fn skin_smoothing_keeps_flat_skin_and_softens_blotches() {
        let flat = RgbImage::from_pixel(80, 80, Rgb([180, 140, 120]));
        let out = retouch_patch(&flat, (20, 59, 20, 59), 40.0, 0.5);
        assert!(out.pixels().all(|p| p.0 == [180, 140, 120]));

        // Low-contrast blotches (what skin smoothing is for) lose contrast.
        let blotchy = RgbImage::from_fn(80, 80, |x, y| {
            let v = if ((x / 6) + (y / 6)) % 2 == 0 {
                150
            } else {
                160
            };
            Rgb([v, v, v])
        });
        let out = retouch_patch(&blotchy, (20, 59, 20, 59), 40.0, 1.0);
        let spread = |img: &RgbImage| {
            let (lo, hi) = img
                .pixels()
                .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
            hi - lo
        };
        assert!(spread(&out) < 10, "spread {}", spread(&out));
    }
}
