//! The contrast threshold found from the flattest part of the photo, so
//! capture sharpening leaves flat areas alone. Copied from
//! src-tauri/src/mods/sharpen.rs @ 2bcb953d.

use rayon::prelude::*;

use crate::Rgb;

/// The automatic contrast threshold (0..1), measured on linear RGB, as the
/// app measures it on the image its render is given.
pub fn measure_contrast(rgb: &Rgb) -> f32 {
    let l: Vec<f32> = rgb
        .pixels
        .par_iter()
        .map(|p| rt_l(p[0], p[1], p[2]))
        .collect();
    auto_contrast_threshold(&l, rgb.width, rgb.height)
}
// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// RawTherapee's L, which is L* scaled to 0..32768. Its constants below are in
/// these units, so the luminance is too.
fn rt_l(r: f32, g: f32, b: f32) -> f32 {
    let y = (0.212671 * r + 0.715160 * g + 0.072169 * b).max(0.0);
    let e = 216.0 / 24389.0;
    let k = 24389.0 / 27.0;
    let f = if y > e {
        y.cbrt()
    } else {
        (k * y + 16.0) / 116.0
    };
    (116.0 * f - 16.0) * 327.68
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// rt_ calcBlendFactor, RawTherapee rt_algo.cc.
fn blend_factor(val: f32, threshold: f32) -> f32 {
    let x = -16.0 + (16.0 / threshold) * val;
    0.5 * (1.0 + x / (1.0 + x * x).sqrt())
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
fn tile_average(l: &[f32], w: usize, y0: usize, x0: usize, size: usize) -> f32 {
    let mut sum = 0.0f32;
    for y in y0..y0 + size {
        sum += l[y * w + x0..y * w + x0 + size].iter().sum::<f32>();
    }
    sum / (size * size) as f32
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
fn tile_variance(l: &[f32], w: usize, y0: usize, x0: usize, size: usize, avg: f32) -> f32 {
    let mut var = 0.0f32;
    for y in y0..y0 + size {
        for &v in &l[y * w + x0..y * w + x0 + size] {
            var += (v - avg) * (v - avg);
        }
    }
    var / ((size * size) as f32 * avg)
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// rt_ calcContrastThreshold: the lowest threshold at which no more than 1%
/// of this flat tile would be sharpened.
fn calc_contrast_threshold(l: &[f32], w: usize, tile_y: usize, tile_x: usize, size: usize) -> f32 {
    const SCALE: f32 = 0.0625 / 327.68;
    let at = |y: usize, x: usize| l[y * w + x];
    let mut contrast = Vec::with_capacity((size - 4) * (size - 4));
    for j in tile_y + 2..tile_y + size - 2 {
        for i in tile_x + 2..tile_x + size - 2 {
            let c = ((at(j, i + 1) - at(j, i - 1)).powi(2)
                + (at(j + 1, i) - at(j - 1, i)).powi(2)
                + (at(j, i + 2) - at(j, i - 2)).powi(2)
                + (at(j + 2, i) - at(j - 2, i)).powi(2))
            .sqrt()
                * SCALE;
            contrast.push(c);
        }
    }
    let limit = ((size - 4) * (size - 4)) as f32 / 100.0;
    let mut c = 1;
    while c < 100 {
        let threshold = c as f32 / 100.0;
        let sum: f32 = contrast.iter().map(|&v| blend_factor(v, threshold)).sum();
        if sum <= limit {
            break;
        }
        c += 1;
    }
    (c + 1) as f32 / 100.0
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// rt_ the automatic half of buildBlendMask, RawTherapee rt_algo.cc
/// @ 1c2b3f33772eaf491b7bd4d66323ac36d13aa344.
///
/// Finds the flattest tile of mid-tone luminance - an 80 px pass, then a
/// finer 40 px pass round the best of it - and sets the threshold so that
/// tile is left almost entirely alone. Returns the threshold in 0..1, and 0
/// when nothing flat enough was found, which RawTherapee reads as "sharpen
/// everywhere".
fn auto_contrast_threshold(l: &[f32], w: usize, h: usize) -> f32 {
    const MIN_LUMINANCE: f32 = 2000.0;
    const MAX_LUMINANCE: f32 = 20000.0;
    const MIN_TILE_VARIANCE: f32 = 0.5;

    let variance_or_inf = |y: usize, x: usize, size: usize| {
        let avg = tile_average(l, w, y, x, size);
        if !(MIN_LUMINANCE..=MAX_LUMINANCE).contains(&avg) {
            return f32::INFINITY;
        }
        let v = tile_variance(l, w, y, x, size, avg);
        if v < MIN_TILE_VARIANCE {
            f32::INFINITY
        } else {
            v
        }
    };

    for pass in 0..2usize {
        let tilesize = 80 / (pass + 1);
        let skip = if pass == 0 { tilesize } else { tilesize / 4 };
        // RawTherapee's counts, which keep the last tile inside the image:
        // pass 0 steps a whole tile, pass 1 a quarter, less the three
        // quarter-steps a 40 px tile overhangs.
        let tiles_w = (w / skip).saturating_sub(3 * pass);
        let tiles_h = (h / skip).saturating_sub(3 * pass);
        if tiles_w == 0 || tiles_h == 0 {
            return 0.0;
        }
        let best = (0..tiles_h)
            .into_par_iter()
            .flat_map_iter(|i| (0..tiles_w).map(move |j| (i, j)))
            .map(|(i, j)| (variance_or_inf(i * skip, j * skip, tilesize), i, j))
            .reduce(
                || (f32::INFINITY, 0, 0),
                |a, b| if b.0 < a.0 { b } else { a },
            );
        let (minvar, min_i, min_j) = best;

        if minvar <= 1.0 || pass == 1 {
            let min_y = skip * min_i;
            let min_x = skip * min_j;
            if pass == 0 {
                return calc_contrast_threshold(l, w, min_y, min_x, tilesize);
            }
            // Second pass: every position within +-skip of the best tile.
            let y_start = min_y.saturating_sub(skip);
            let x_start = min_x.saturating_sub(skip);
            let y_end = (min_y + skip).min(h - tilesize);
            let x_end = (min_x + skip).min(w - tilesize);
            let mut best = (f32::INFINITY, 0, 0);
            for y in y_start..=y_end {
                for x in x_start..=x_end {
                    let v = variance_or_inf(y, x, tilesize);
                    if v < best.0 {
                        best = (v, y, x);
                    }
                }
            }
            return if best.0 <= 8.0 {
                calc_contrast_threshold(l, w, best.1, best.2, tilesize)
            } else {
                0.0
            };
        }
    }
    0.0
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blend_factor_is_one_half_at_the_threshold() {
        assert!((blend_factor(0.1, 0.1) - 0.5).abs() < 1e-6);
        assert!(blend_factor(0.0, 0.1) < 0.01);
        assert!(blend_factor(0.3, 0.1) > 0.99);
    }
}