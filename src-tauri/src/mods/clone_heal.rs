//! Heal's tone match: the smooth correction that carries the tone around a
//! spot into the texture copied from its source. RapidRAW's.
//!
//! Copied from RapidRAW 19427777 ("rework clone and heal ..."):
//! `src-tauri/src/clone_heal.rs` `heal_patch`, `build_level`, `relax`, with
//! their helpers `pixel`, `masked_pixels` and `patch_canvas` (at v1.7.0 the
//! file is `src-tauri/src/retouch/clone_heal.rs`, moved in ad1dc9b9,
//! unchanged). Not taken: `clone_patch` (ours is the same loop), and the
//! automatically found source and its outline (`find_source_offset`,
//! `split_region`), which change how Heal is used.
//!
//! WHY
//!
//! A heal is the source's pixels plus a correction: the destination minus the
//! source on the ring just outside the spot, spread smoothly inside it (a
//! Laplace solve with the ring as its boundary). 1.6.6 relaxed that with 400
//! fixed SOR sweeps from zero. A sweep moves the ring's value about one pixel
//! inwards, so the middle of a spot over about 80 px was never reached: a
//! 100 px heal kept 11% of the tone mismatch, a 200 px heal 45% (92% at its
//! centre). This solves the same equation on a pyramid of the spot (down to
//! 256 unknowns, at most 12 levels), coarse to fine, each level until no value
//! moves by 0.01 of a level, so the whole spot takes the ring's tone.
//!
//! Changes from the source: generic over the image, so the heal reads the same
//! `DynamicImage` pixels it always did, with no full-size copy.

use image::{GenericImageView, GrayImage, Pixel, RgbImage, Rgba};
use rayon::prelude::*;

/// `(min_x, max_x, min_y, max_y)` of the spot, inclusive, in image pixels.
pub type Bounds = (usize, usize, usize, usize);

/// The image a heal reads from: 8-bit RGBA pixels, readable from any thread.
pub trait HealSource: GenericImageView<Pixel = Rgba<u8>> + Sync {}
impl<I: GenericImageView<Pixel = Rgba<u8>> + Sync> HealSource for I {}

// RapidRAW 19427777: src-tauri/src/clone_heal.rs

const NO_NEIGHBOR: u32 = u32::MAX;

#[inline]
fn rgb8(image: &impl HealSource, x: u32, y: u32) -> [u8; 3] {
    image.get_pixel(x, y).to_rgb().0
}

#[inline]
fn pixel(image: &impl HealSource, x: i64, y: i64) -> [f32; 3] {
    let x = x.clamp(0, image.width() as i64 - 1) as u32;
    let y = y.clamp(0, image.height() as i64 - 1) as u32;
    rgb8(image, x, y).map(f32::from)
}

fn masked_pixels(mask: &GrayImage, bounds: Bounds) -> Vec<(u32, u32)> {
    let (min_x, max_x, min_y, max_y) = bounds;
    (min_y..=max_y)
        .into_par_iter()
        .flat_map_iter(|y| {
            (min_x..=max_x)
                .filter(move |&x| mask.get_pixel(x as u32, y as u32)[0] > 0)
                .map(move |x| (y as u32, x as u32))
        })
        .collect()
}

fn patch_canvas(
    image: &impl HealSource,
    bounds: Bounds,
    fill: impl Fn(u32, u32) -> Option<[u8; 3]> + Sync,
) -> RgbImage {
    let (min_x, max_x, min_y, max_y) = bounds;
    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    let mut out = vec![0u8; width * height * 3];
    out.par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(row, chunk)| {
            let y = (min_y + row) as u32;
            for col in 0..width {
                let x = (min_x + col) as u32;
                let rgb = fill(x, y).unwrap_or_else(|| rgb8(image, x, y));
                chunk[col * 3..col * 3 + 3].copy_from_slice(&rgb);
            }
        });
    RgbImage::from_raw(width as u32, height as u32, out).expect("width * height * 3 bytes")
}

/// One level of the pyramid: the spot's pixels (unknown) and the ring around
/// them (known), sorted by `(y, x)`.
struct Level {
    width: u32,
    height: u32,
    coords: Vec<(u32, u32)>,
    unknown: Vec<bool>,
    values: Vec<[f32; 3]>,
}

impl Level {
    fn find(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        self.coords.binary_search(&(y as u32, x as u32)).ok()
    }
}

fn build_level(
    width: u32,
    height: u32,
    unknown_coords: Vec<(u32, u32)>,
    known_value: impl Fn(u32, u32) -> [f32; 3] + Sync,
) -> Level {
    let is_unknown = |y: u32, x: u32| unknown_coords.binary_search(&(y, x)).is_ok();

    let mut ring: Vec<(u32, u32)> = unknown_coords
        .par_iter()
        .flat_map_iter(|&(y, x)| {
            let mut candidates = Vec::with_capacity(4);
            if x > 0 {
                candidates.push((y, x - 1));
            }
            if x + 1 < width {
                candidates.push((y, x + 1));
            }
            if y > 0 {
                candidates.push((y - 1, x));
            }
            if y + 1 < height {
                candidates.push((y + 1, x));
            }
            candidates
                .into_iter()
                .filter(|&(ny, nx)| !is_unknown(ny, nx))
        })
        .collect();
    ring.par_sort_unstable();
    ring.dedup();

    let mut coords = Vec::with_capacity(unknown_coords.len() + ring.len());
    let mut unknown = Vec::with_capacity(coords.capacity());
    let (mut a, mut b) = (0, 0);
    while a < unknown_coords.len() || b < ring.len() {
        if b >= ring.len() || (a < unknown_coords.len() && unknown_coords[a] < ring[b]) {
            coords.push(unknown_coords[a]);
            unknown.push(true);
            a += 1;
        } else {
            coords.push(ring[b]);
            unknown.push(false);
            b += 1;
        }
    }

    let values = coords
        .par_iter()
        .zip(unknown.par_iter())
        .map(|(&(y, x), &is_unknown)| {
            if is_unknown {
                [0.0; 3]
            } else {
                known_value(x, y)
            }
        })
        .collect();

    Level {
        width,
        height,
        coords,
        unknown,
        values,
    }
}

/// Red-black SOR over the level's unknowns until no value moves by
/// `tolerance`, or `max_iterations` sweeps.
fn relax(level: &mut Level, omega: f32, max_iterations: usize, tolerance: f32) {
    let cells: Vec<(u32, [u32; 4])> = (0..level.coords.len())
        .into_par_iter()
        .filter(|&i| level.unknown[i])
        .map(|i| {
            let (y, x) = level.coords[i];
            let (x, y) = (x as i64, y as i64);
            let neighbors = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .map(|(nx, ny)| level.find(nx, ny).map_or(NO_NEIGHBOR, |n| n as u32));
            (i as u32, neighbors)
        })
        .collect();
    let (red, black): (Vec<_>, Vec<_>) = cells.into_iter().partition(|&(i, _)| {
        let (y, x) = level.coords[i as usize];
        (x + y) % 2 == 0
    });

    for _ in 0..max_iterations {
        let mut max_delta = 0.0f32;
        for cells in [&red, &black] {
            let values = &level.values;
            let updates: Vec<([f32; 3], f32)> = cells
                .par_iter()
                .map(|&(i, neighbors)| {
                    let mut sum = [0.0f32; 3];
                    let mut count = 0.0f32;
                    for n in neighbors {
                        if n != NO_NEIGHBOR {
                            let v = values[n as usize];
                            sum[0] += v[0];
                            sum[1] += v[1];
                            sum[2] += v[2];
                            count += 1.0;
                        }
                    }
                    let current = values[i as usize];
                    if count == 0.0 {
                        return (current, 0.0);
                    }
                    let mut next = current;
                    let mut delta = 0.0f32;
                    for c in 0..3 {
                        next[c] = current[c] + omega * (sum[c] / count - current[c]);
                        delta = delta.max((next[c] - current[c]).abs());
                    }
                    (next, delta)
                })
                .collect();
            for (&(i, _), (value, delta)) in cells.iter().zip(updates) {
                level.values[i as usize] = value;
                max_delta = max_delta.max(delta);
            }
        }
        if max_delta < tolerance {
            break;
        }
    }
}

/// The healed spot: the pixels `offset` away, plus the destination-minus-source
/// difference on the ring solved smoothly across the spot. Returns the
/// `bounds` crop; pixels outside `mask` keep the image's own.
pub fn heal_patch(
    image: &impl HealSource,
    mask: &GrayImage,
    bounds: Bounds,
    offset: (i32, i32),
) -> RgbImage {
    let (img_w, img_h) = image.dimensions();
    let (ox, oy) = (offset.0 as i64, offset.1 as i64);
    let difference = |x: i64, y: i64| {
        let dest = pixel(image, x, y);
        let src = pixel(image, x + ox, y + oy);
        [dest[0] - src[0], dest[1] - src[1], dest[2] - src[2]]
    };

    let mut levels = vec![build_level(
        img_w,
        img_h,
        masked_pixels(mask, bounds),
        |x, y| difference(x as i64, y as i64),
    )];

    while levels.len() < 12 {
        let fine = levels.last().expect("the finest level is always there");
        let unknown_count = fine.unknown.iter().filter(|&&u| u).count();
        if unknown_count <= 256 {
            break;
        }
        let scale = 1i64 << levels.len();
        let mut parents: Vec<(u32, u32)> = fine
            .coords
            .iter()
            .zip(&fine.unknown)
            .filter(|&(_, &u)| u)
            .map(|(&(y, x), _)| (y / 2, x / 2))
            .collect();
        parents.par_sort_unstable();
        parents.dedup();
        let width = fine.width.div_ceil(2);
        let height = fine.height.div_ceil(2);
        levels.push(build_level(width, height, parents, |x, y| {
            let x0 = x as i64 * scale;
            let y0 = y as i64 * scale;
            let x1 = (x0 + scale).min(img_w as i64);
            let y1 = (y0 + scale).min(img_h as i64);
            let mut sum = [0.0f32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let d = difference(sx, sy);
                    sum[0] += d[0];
                    sum[1] += d[1];
                    sum[2] += d[2];
                }
            }
            let n = ((x1 - x0) * (y1 - y0)).max(1) as f32;
            [sum[0] / n, sum[1] / n, sum[2] / n]
        }));
    }

    let coarsest = levels.last_mut().expect("the finest level is always there");
    let (mut ring_sum, mut ring_count) = ([0.0f32; 3], 0.0f32);
    for (v, _) in coarsest
        .values
        .iter()
        .zip(&coarsest.unknown)
        .filter(|(_, u)| !**u)
    {
        for c in 0..3 {
            ring_sum[c] += v[c];
        }
        ring_count += 1.0;
    }
    let ring_mean = ring_sum.map(|s| s / ring_count.max(1.0));
    for (v, _) in coarsest
        .values
        .iter_mut()
        .zip(&coarsest.unknown)
        .filter(|(_, u)| **u)
    {
        *v = ring_mean;
    }
    relax(coarsest, 1.85, 2000, 0.01);

    for index in (0..levels.len() - 1).rev() {
        let (fine_levels, coarse_levels) = levels.split_at_mut(index + 1);
        let fine = &mut fine_levels[index];
        let coarse = &coarse_levels[0];
        let guesses: Vec<(usize, [f32; 3])> = (0..fine.coords.len())
            .into_par_iter()
            .filter(|&i| fine.unknown[i])
            .map(|i| {
                let (y, x) = fine.coords[i];
                let parent = coarse
                    .find((x / 2) as i64, (y / 2) as i64)
                    .map_or([0.0; 3], |p| coarse.values[p]);
                let cx = (x as f32 + 0.5) / 2.0 - 0.5;
                let cy = (y as f32 + 0.5) / 2.0 - 0.5;
                let x0 = cx.floor();
                let y0 = cy.floor();
                let fx = cx - x0;
                let fy = cy - y0;
                let at = |dx: i64, dy: i64| {
                    coarse
                        .find(x0 as i64 + dx, y0 as i64 + dy)
                        .map_or(parent, |p| coarse.values[p])
                };
                let (v00, v10, v01, v11) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
                let mut out = [0.0f32; 3];
                for c in 0..3 {
                    let top = v00[c] + (v10[c] - v00[c]) * fx;
                    let bottom = v01[c] + (v11[c] - v01[c]) * fx;
                    out[c] = top + (bottom - top) * fy;
                }
                (i, out)
            })
            .collect();
        for (i, value) in guesses {
            fine.values[i] = value;
        }
        relax(fine, 1.9, if index == 0 { 150 } else { 80 }, 0.01);
    }

    let finest = &levels[0];
    patch_canvas(image, bounds, |x, y| {
        if mask.get_pixel(x, y)[0] == 0 {
            return None;
        }
        let v = finest
            .find(x as i64, y as i64)
            .map_or([0.0; 3], |i| finest.values[i]);
        let src = pixel(image, x as i64 + ox, y as i64 + oy);
        Some([0, 1, 2].map(|c| (src[c] + v[c]).round().clamp(0.0, 255.0) as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Luma, Rgb};

    /// A disc of `diameter` px centred on `(cx, cy)`, and its bounds.
    fn disc(w: u32, h: u32, cx: i64, cy: i64, diameter: i64) -> (GrayImage, Bounds) {
        let r = diameter as f64 / 2.0;
        let mut mask = GrayImage::new(w, h);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (usize::MAX, 0, usize::MAX, 0);
        let reach = diameter / 2 + 2;
        for y in (cy - reach).max(0)..(cy + reach).min(h as i64) {
            for x in (cx - reach).max(0)..(cx + reach).min(w as i64) {
                let (dx, dy) = ((x - cx) as f64 + 0.5, (y - cy) as f64 + 0.5);
                if dx * dx + dy * dy <= r * r {
                    mask.put_pixel(x as u32, y as u32, Luma([255]));
                    min_x = min_x.min(x as usize);
                    max_x = max_x.max(x as usize);
                    min_y = min_y.min(y as usize);
                    max_y = max_y.max(y as usize);
                }
            }
        }
        (mask, (min_x, max_x, min_y, max_y))
    }

    /// Largest difference from `expected(x, y)` over the healed disc.
    fn worst(
        patch: &RgbImage,
        mask: &GrayImage,
        bounds: Bounds,
        expected: impl Fn(u32, u32) -> [u8; 3],
    ) -> i32 {
        let mut worst = 0;
        for (px, py, p) in patch.enumerate_pixels() {
            let (x, y) = (px + bounds.0 as u32, py + bounds.2 as u32);
            if mask.get_pixel(x, y)[0] > 0 {
                let e = expected(x, y);
                for c in 0..3 {
                    worst = worst.max((p[c] as i32 - e[c] as i32).abs());
                }
            }
        }
        worst
    }

    #[test]
    fn a_large_spot_takes_the_tone_around_it_all_the_way_in() {
        // Grey 100 on the left, 160 on the right; a 200 px disc on the left
        // healed from the right. The ring says "60 darker", everywhere.
        let mut img = RgbImage::new(700, 300);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 350 {
                Rgb([100; 3])
            } else {
                Rgb([160; 3])
            };
        }
        let img = DynamicImage::ImageRgb8(img);
        let (mask, bounds) = disc(700, 300, 150, 150, 200);
        let patch = heal_patch(&img, &mask, bounds, (400, 0));
        assert!(worst(&patch, &mask, bounds, |_, _| [100; 3]) <= 1);
    }

    #[test]
    fn a_smooth_gradient_is_carried_across() {
        // The destination is a ramp, the source flat: the heal must rebuild
        // the ramp (linear is harmonic, so it is the exact solution).
        let mut img = RgbImage::new(600, 260);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = if x < 300 {
                Rgb([(40 + x / 2) as u8, (60 + y / 4) as u8, 90])
            } else {
                Rgb([200, 30, 120])
            };
        }
        let source = img.clone();
        let img = DynamicImage::ImageRgb8(img);
        let (mask, bounds) = disc(600, 260, 140, 130, 180);
        let patch = heal_patch(&img, &mask, bounds, (310, 0));
        assert!(worst(&patch, &mask, bounds, |x, y| source.get_pixel(x, y).0) <= 1);
    }

    #[test]
    fn pixels_outside_the_spot_are_the_image_s_own() {
        let mut img = RgbImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([x as u8 * 3, y as u8 * 3, 7]);
        }
        let source = img.clone();
        let img = DynamicImage::ImageRgb8(img);
        let (mask, bounds) = disc(64, 64, 20, 20, 20);
        let patch = heal_patch(&img, &mask, bounds, (30, 30));
        for (px, py, p) in patch.enumerate_pixels() {
            let (x, y) = (px + bounds.0 as u32, py + bounds.2 as u32);
            if mask.get_pixel(x, y)[0] == 0 {
                assert_eq!(p.0, source.get_pixel(x, y).0);
            }
        }
    }

    /// Before and after on real photos: `AG_RAWS` (';'-separated RAWs).
    /// For each photo, spots of 50, 100 and 200 px at four places, each
    /// healed from the nearby source whose tone differs most. Against a solve
    /// run to 1e-4, reports how much of the ring's mismatch is left over the
    /// spot and at its centre, and the ΔE00 to it, for 1.6.6's 400 SOR
    /// sweeps and for this solver.
    #[test]
    #[ignore = "reads RAW files; run by hand"]
    fn measure_on_raws() {
        let raws = std::env::var("AG_RAWS").expect("set AG_RAWS");
        println!(
            "photo\tpx\tspot\tring\tleft_old%\tleft_new%\tcentre_old%\tcentre_new%\tde_old\tde_new\tms_old\tms_new"
        );
        for (n, path) in raws.split(';').filter(|p| !p.trim().is_empty()).enumerate() {
            let bytes = std::fs::read(path).expect("read raw");
            let linear = crate::raw_processing::develop_raw_image(
                &bytes,
                false,
                2.5,
                "default".to_string(),
                None,
                Some(path),
            )
            .expect("decodes");
            // What the heal reads for a RAW (`prepare_source_image`).
            let img = crate::image_processing::apply_linear_to_srgb(linear);
            let (w, h) = img.dimensions();
            for diameter in [50i64, 100, 200] {
                for (spot, (fx, fy)) in [(0.3, 0.35), (0.7, 0.35), (0.3, 0.65), (0.7, 0.65)]
                    .into_iter()
                    .enumerate()
                {
                    let (cx, cy) = ((w as f64 * fx) as i64, (h as f64 * fy) as i64);
                    let (mask, bounds) = disc(w, h, cx, cy, diameter);
                    let ring = ring_of(&mask, bounds);
                    let step = diameter * 3 / 2;
                    let offset = [(step, 0), (-step, 0), (0, step), (0, -step)]
                        .into_iter()
                        .chain([(step, step), (-step, step), (step, -step), (-step, -step)])
                        .max_by(|a, b| {
                            ring_mismatch(&img, &ring, *a)
                                .total_cmp(&ring_mismatch(&img, &ring, *b))
                        })
                        .map(|(x, y)| (x as i32, y as i32))
                        .expect("eight offsets");
                    let mismatch = ring_mismatch(&img, &ring, (offset.0 as i64, offset.1 as i64));
                    let t = std::time::Instant::now();
                    let old = legacy_sor_400(&img, &mask, bounds, offset);
                    let ms_old = t.elapsed().as_millis();
                    let t = std::time::Instant::now();
                    let new = heal_patch(&img, &mask, bounds, offset);
                    let ms_new = t.elapsed().as_millis();
                    let reference = reference_solve(&img, &mask, bounds, offset);
                    let (lo, co, de_o) = left(&old, &reference, &mask, bounds, (cx, cy), diameter);
                    let (ln, cn, de_n) = left(&new, &reference, &mask, bounds, (cx, cy), diameter);
                    println!(
                        "{}\t{diameter}\t{}\t{mismatch:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{de_o:.2}\t{de_n:.2}\t{ms_old}\t{ms_new}",
                        n + 1,
                        spot + 1,
                        100.0 * lo / mismatch,
                        100.0 * ln / mismatch,
                        100.0 * co / mismatch,
                        100.0 * cn / mismatch,
                    );
                }
            }
        }
    }

    fn ring_of(mask: &GrayImage, bounds: Bounds) -> Vec<(i64, i64)> {
        let (min_x, max_x, min_y, max_y) = bounds;
        let inside = |x: i64, y: i64| mask.get_pixel(x as u32, y as u32)[0] > 0;
        let mut ring = Vec::new();
        for y in min_y as i64 - 1..=max_y as i64 + 1 {
            for x in min_x as i64 - 1..=max_x as i64 + 1 {
                if !inside(x, y)
                    && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                        .iter()
                        .any(|&(nx, ny)| inside(nx, ny))
                {
                    ring.push((x, y));
                }
            }
        }
        ring
    }

    /// Mean destination-minus-source difference on the ring, in levels.
    fn ring_mismatch(img: &DynamicImage, ring: &[(i64, i64)], offset: (i64, i64)) -> f64 {
        let sum: f64 = ring
            .iter()
            .map(|&(x, y)| {
                let d = pixel(img, x, y);
                let s = pixel(img, x + offset.0, y + offset.1);
                (0..3).map(|c| (d[c] - s[c]).abs() as f64).sum::<f64>() / 3.0
            })
            .sum();
        sum / ring.len() as f64
    }

    /// The correction solved to 1e-4 by plain SOR (the exact answer both
    /// solvers aim at), as the healed spot's colours before rounding.
    fn reference_solve(
        img: &DynamicImage,
        mask: &GrayImage,
        bounds: Bounds,
        offset: (i32, i32),
    ) -> Vec<[f32; 3]> {
        let (min_x, max_x, min_y, max_y) = bounds;
        let (bw, bh) = (max_x - min_x + 3, max_y - min_y + 3);
        let at = |x: usize, y: usize| (min_x as i64 + x as i64 - 1, min_y as i64 + y as i64 - 1);
        let mut v = vec![[0.0f64; 3]; bw * bh];
        let mut inside = vec![false; bw * bh];
        for y in 0..bh {
            for x in 0..bw {
                let (ix, iy) = at(x, y);
                if mask.get_pixel(ix as u32, iy as u32)[0] > 0 {
                    inside[y * bw + x] = true;
                } else {
                    let d = pixel(img, ix, iy);
                    let s = pixel(img, ix + offset.0 as i64, iy + offset.1 as i64);
                    v[y * bw + x] = [0, 1, 2].map(|c| (d[c] - s[c]) as f64);
                }
            }
        }
        loop {
            let mut max_delta = 0.0f64;
            for y in 1..bh - 1 {
                for x in 1..bw - 1 {
                    let i = y * bw + x;
                    if !inside[i] {
                        continue;
                    }
                    let (l, r, u, d) = (v[i - 1], v[i + 1], v[i - bw], v[i + bw]);
                    for (c, value) in v[i].iter_mut().enumerate() {
                        let avg = (l[c] + r[c] + u[c] + d[c]) / 4.0;
                        let next = *value + 1.97 * (avg - *value);
                        max_delta = max_delta.max((next - *value).abs());
                        *value = next;
                    }
                }
            }
            if max_delta < 1e-4 {
                break;
            }
        }
        let (cw, ch) = (max_x - min_x + 1, max_y - min_y + 1);
        let mut out = vec![[0.0f32; 3]; cw * ch];
        for y in 0..ch {
            for x in 0..cw {
                let (ix, iy) = ((min_x + x) as i64, (min_y + y) as i64);
                let s = pixel(img, ix + offset.0 as i64, iy + offset.1 as i64);
                let c = v[(y + 1) * bw + x + 1];
                out[y * cw + x] = [0, 1, 2].map(|k| s[k] + c[k] as f32);
            }
        }
        out
    }

    /// Mean distance to the reference over the spot and over its centre
    /// (a quarter of its radius), in levels, and the mean ΔE00 to it.
    fn left(
        patch: &RgbImage,
        reference: &[[f32; 3]],
        mask: &GrayImage,
        bounds: Bounds,
        centre: (i64, i64),
        diameter: i64,
    ) -> (f64, f64, f64) {
        let lab = |p: [f32; 3]| {
            argentum_engine::colour::lab(
                p.map(|v| argentum_engine::colour::srgb_decode(v.clamp(0.0, 255.0) / 255.0)),
            )
        };
        let (mut all, mut n, mut mid, mut m, mut de) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let r = diameter as f64 / 8.0;
        for (px, py, p) in patch.enumerate_pixels() {
            let (x, y) = (px as usize + bounds.0, py as usize + bounds.2);
            if mask.get_pixel(x as u32, y as u32)[0] == 0 {
                continue;
            }
            let e = reference[py as usize * patch.width() as usize + px as usize];
            let got = p.0.map(f32::from);
            let d = (0..3).map(|c| (got[c] - e[c]).abs() as f64).sum::<f64>() / 3.0;
            all += d;
            n += 1.0;
            de += argentum_engine::colour::delta_e2000(lab(got), lab(e));
            let (dx, dy) = (x as f64 - centre.0 as f64, y as f64 - centre.1 as f64);
            if dx * dx + dy * dy <= r * r {
                mid += d;
                m += 1.0;
            }
        }
        (all / n, mid / m, de / n)
    }

    /// 1.6.6's heal solve, as `generate_manual_cleanup_patch` ran it before
    /// this module: 400 SOR sweeps, omega 1.6, from zero.
    fn legacy_sor_400(
        source_image: &DynamicImage,
        mask_bitmap: &GrayImage,
        bounds: Bounds,
        offset: (i32, i32),
    ) -> RgbImage {
        let (min_x, max_x, min_y, max_y) = bounds;
        let (offset_x, offset_y) = offset;
        let (img_w, img_h) = source_image.dimensions();
        let (crop_w, crop_h) = ((max_x - min_x + 1) as u32, (max_y - min_y + 1) as u32);
        let mut color_image = RgbImage::new(crop_w, crop_h);
        let bw = max_x - min_x + 3;
        let bh = max_y - min_y + 3;
        let mut v_r = vec![0.0f32; bw * bh];
        let mut v_g = vec![0.0f32; bw * bh];
        let mut v_b = vec![0.0f32; bw * bh];
        let mut region = vec![0u8; bw * bh];
        for y in 0..bh {
            for x in 0..bw {
                let img_x = min_x as i32 + x as i32 - 1;
                let img_y = min_y as i32 + y as i32 - 1;
                if img_x >= 0
                    && img_x < img_w as i32
                    && img_y >= 0
                    && img_y < img_h as i32
                    && mask_bitmap.get_pixel(img_x as u32, img_y as u32)[0] > 0
                {
                    region[y * bw + x] = 1;
                }
            }
        }
        let mut omega_coords = Vec::with_capacity(bw * bh);
        for y in 1..(bh - 1) {
            for x in 1..(bw - 1) {
                if region[y * bw + x] == 0 {
                    if region[(y - 1) * bw + x] == 1
                        || region[(y + 1) * bw + x] == 1
                        || region[y * bw + x - 1] == 1
                        || region[y * bw + x + 1] == 1
                    {
                        region[y * bw + x] = 2;
                        let img_x = (min_x as i32 + x as i32 - 1) as u32;
                        let img_y = (min_y as i32 + y as i32 - 1) as u32;
                        let src_x = (img_x as i32 + offset_x).clamp(0, img_w as i32 - 1) as u32;
                        let src_y = (img_y as i32 + offset_y).clamp(0, img_h as i32 - 1) as u32;
                        let dest_px = source_image.get_pixel(img_x, img_y);
                        let src_px = source_image.get_pixel(src_x, src_y);
                        v_r[y * bw + x] = dest_px[0] as f32 - src_px[0] as f32;
                        v_g[y * bw + x] = dest_px[1] as f32 - src_px[1] as f32;
                        v_b[y * bw + x] = dest_px[2] as f32 - src_px[2] as f32;
                    }
                } else if region[y * bw + x] == 1 {
                    omega_coords.push((x, y));
                }
            }
        }
        let omega = 1.6f32;
        for _ in 0..400 {
            for &(x, y) in &omega_coords {
                let idx = y * bw + x;
                let sum_r = v_r[idx - bw] + v_r[idx + bw] + v_r[idx - 1] + v_r[idx + 1];
                let sum_g = v_g[idx - bw] + v_g[idx + bw] + v_g[idx - 1] + v_g[idx + 1];
                let sum_b = v_b[idx - bw] + v_b[idx + bw] + v_b[idx - 1] + v_b[idx + 1];
                v_r[idx] = (1.0 - omega) * v_r[idx] + omega * 0.25 * sum_r;
                v_g[idx] = (1.0 - omega) * v_g[idx] + omega * 0.25 * sum_g;
                v_b[idx] = (1.0 - omega) * v_b[idx] + omega * 0.25 * sum_b;
            }
        }
        for &(x, y) in &omega_coords {
            let img_x = (min_x as i32 + x as i32 - 1) as u32;
            let img_y = (min_y as i32 + y as i32 - 1) as u32;
            let src_x = (img_x as i32 + offset_x).clamp(0, img_w as i32 - 1) as u32;
            let src_y = (img_y as i32 + offset_y).clamp(0, img_h as i32 - 1) as u32;
            let src_px = source_image.get_pixel(src_x, src_y);
            let idx = y * bw + x;
            let out_r = (src_px[0] as f32 + v_r[idx]).clamp(0.0, 255.0) as u8;
            let out_g = (src_px[1] as f32 + v_g[idx]).clamp(0.0, 255.0) as u8;
            let out_b = (src_px[2] as f32 + v_b[idx]).clamp(0.0, 255.0) as u8;
            let out_x = img_x as i32 - min_x as i32;
            let out_y = img_y as i32 - min_y as i32;
            if out_x >= 0 && out_x < crop_w as i32 && out_y >= 0 && out_y < crop_h as i32 {
                color_image.put_pixel(out_x as u32, out_y as u32, Rgb([out_r, out_g, out_b]));
            }
        }
        color_image
    }
}
