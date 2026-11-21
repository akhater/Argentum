//! Pairs of a photo and its embedded JPEG: the photo at the proxy's size, the
//! JPEG's alignment with it, and the pixels a fit may not learn from.
//!
//! From RAWmakase `v0.7.1` @ 78069aa3 (MIT),
//! `crates/rawmakase-decode/src/camera_look/pairs.rs`, as written, except:
//! the JPEG's colour is read by the app (`Preview` is handed in); the photo's
//! detail for the alignment is the stage's luminance, not the camera input's
//! green; and the search is not cancelled from outside.

use super::{Refusal, Rgb, Stage, refuse};
use crate::colour::srgb_encode;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// The proxy's long edge, and how much larger the stage it is reduced from is.
pub const PROXY_EDGE: u32 = 192;
pub const RENDER_FACTOR: u32 = 4;

/// The camera's JPEG, upright, as linear ProPhoto RGB.
#[derive(Clone)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub pro: Vec<Rgb>,
    /// A channel at the encoding's ends (8-bit >= 250 or <= 1): clipped by
    /// the camera.
    pub clipped: Vec<bool>,
}

/// The colour space a camera wrote its JPEG in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Srgb,
    /// Adobe RGB (1998): gamma 563/256 over its own primaries, white D65.
    AdobeRgb,
}

/// Linear Adobe RGB (1998) to linear sRGB, both D65, through XYZ: Adobe's
/// published primaries (Adobe RGB (1998) Color Image Encoding, 2005) and
/// sRGB's (IEC 61966-2-1), row by row.
fn adobe_to_srgb() -> [[f32; 3]; 3] {
    const ADOBE_TO_XYZ: [[f64; 3]; 3] = [
        [0.576_730_9, 0.185_554_0, 0.188_185_2],
        [0.297_376_9, 0.627_349_1, 0.075_274_1],
        [0.027_034_3, 0.070_687_2, 0.991_108_5],
    ];
    const XYZ_TO_SRGB: [[f64; 3]; 3] = [
        [3.240_454_2, -1.537_138_5, -0.498_531_4],
        [-0.969_266_0, 1.876_010_8, 0.041_556_0],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            (0..3)
                .map(|k| XYZ_TO_SRGB[i][k] * ADOBE_TO_XYZ[k][j])
                .sum::<f64>() as f32
        })
    })
}

impl Preview {
    /// The JPEG from its 8-bit pixels in `space`, `width` wide, row by row.
    pub fn from_rgb8(width: u32, height: u32, rgb: &[u8], space: Space) -> Self {
        let decode: Vec<f32> = (0..=255)
            .map(|v| {
                let e = v as f32 / 255.;
                match space {
                    Space::Srgb => crate::colour::srgb_decode(e),
                    Space::AdobeRgb => e.powf(563. / 256.),
                }
            })
            .collect();
        let to_srgb = match space {
            Space::Srgb => [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            Space::AdobeRgb => adobe_to_srgb(),
        };
        let (pixels, _) = rgb.as_chunks::<3>();
        Self {
            width,
            height,
            pro: pixels
                .iter()
                .map(|p| {
                    let linear = p.map(|v| decode[usize::from(v)]);
                    let srgb = crate::colour::mul(to_srgb, linear);
                    crate::colour::mul(crate::colour::SRGB_TO_PROPHOTO, srgb)
                })
                .collect(),
            clipped: pixels
                .iter()
                .map(|p| p.iter().any(|v| *v >= 250 || *v <= 1))
                .collect(),
        }
    }
}

/// The JPEG cut centrally to the photo's aspect ratio (`width` x `height`)
/// when they differ: cameras that crop to an aspect ratio in camera embed
/// either the whole frame around it (Canon) or the crop letterboxed in the
/// sensor's shape (Panasonic), both centred. Alignment then checks it.
pub fn to_aspect(p: Preview, width: u32, height: u32) -> Preview {
    let target = width as f32 / height as f32;
    let ratio = (p.width as f32 / p.height as f32) / target;
    if (ratio - 1.).abs() <= 0.02 {
        return p;
    }
    let (w, h) = if ratio > 1. {
        (
            ((p.height as f32 * target).round() as u32).min(p.width),
            p.height,
        )
    } else {
        (
            p.width,
            ((p.width as f32 / target).round() as u32).min(p.height),
        )
    };
    let (x0, y0) = ((p.width - w) / 2, (p.height - h) / 2);
    let pick = |i: u32| ((y0 + i / w) * p.width + x0 + i % w) as usize;
    Preview {
        width: w,
        height: h,
        pro: (0..w * h).map(|i| p.pro[pick(i)]).collect(),
        clipped: (0..w * h).map(|i| p.clipped[pick(i)]).collect(),
    }
}

/// The photo at the proxy's size, reduced in boxes from the stage.
pub struct Proxy {
    pub width: u32,
    pub height: u32,
    /// Inside the photo: every sample of the box is.
    pub valid: Vec<bool>,
    /// A sample of the box was clipped by the sensor.
    pub sensor_clipped: Vec<bool>,
    /// Linear sRGB, as the RAW tone stage receives it.
    pub linear: Vec<Rgb>,
}

/// `pixels` (`width` wide) reduced by `RENDER_FACTOR` in boxes, NaN where any
/// of a box's samples is.
pub fn reduce(pixels: &[Rgb], width: u32, height: u32) -> (u32, u32, Vec<Rgb>) {
    let f = RENDER_FACTOR;
    let (w, h) = (width / f, height / f);
    let mut out = vec![[0f32; 3]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f64; 3];
            for dy in 0..f {
                for dx in 0..f {
                    let p = pixels[((y * f + dy) * width + x * f + dx) as usize];
                    for c in 0..3 {
                        sum[c] += f64::from(p[c]);
                    }
                }
            }
            let n = f64::from(f * f);
            out[(y * w + x) as usize] = sum.map(|v| (v / n) as f32);
        }
    }
    (w, h, out)
}

/// The proxy of `stage`: valid pixels, and those a sample of which the sensor
/// clipped.
pub(super) fn proxy(stage: &Stage) -> Proxy {
    let f = RENDER_FACTOR;
    let (width, height, reduced) = reduce(&stage.linear, stage.width, stage.height);
    let mut sensor_clipped = vec![false; reduced.len()];
    for y in 0..height {
        for x in 0..width {
            sensor_clipped[(y * width + x) as usize] = (0..f * f)
                .any(|i| stage.clipped[((y * f + i / f) * stage.width + x * f + i % f) as usize]);
        }
    }
    Proxy {
        width,
        height,
        valid: reduced
            .iter()
            .map(|p| p.iter().all(|v| v.is_finite()))
            .collect(),
        sensor_clipped,
        linear: reduced,
    }
}

/// Where the JPEG lies on the photo: its scale about the centre, radial
/// distortion and shift in proxy pixels, and the correlation of the two's
/// detail in the centre and the four quadrants there.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Alignment {
    pub scale: f32,
    pub radial: [f32; 2],
    pub shift: [f32; 2],
    pub regions: [f32; 5],
}

/// The JPEG's box averages over the proxy's pixels.
pub(super) struct Sampled {
    pub pro: Vec<Rgb>,
    pub clipped: Vec<bool>,
}

/// Summed-area tables of the JPEG, for box averages in constant time.
struct Integral {
    width: usize,
    sums: Vec<[f64; 4]>,
}
impl Integral {
    fn new(p: &Preview) -> Self {
        let (w, h) = (p.width as usize, p.height as usize);
        let mut sums = vec![[0f64; 4]; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = [0f64; 4];
            for x in 0..w {
                let i = y * w + x;
                let v = p.pro[i];
                row[0] += f64::from(v[0]);
                row[1] += f64::from(v[1]);
                row[2] += f64::from(v[2]);
                row[3] += f64::from(u8::from(p.clipped[i]));
                let above = sums[y * (w + 1) + x + 1];
                sums[(y + 1) * (w + 1) + x + 1] = std::array::from_fn(|c| above[c] + row[c]);
            }
        }
        Self { width: w, sums }
    }
    /// The mean over pixel columns `x0..x1` and rows `y0..y1`, and whether any
    /// is clipped.
    fn mean(&self, [x0, y0, x1, y1]: [usize; 4]) -> (Rgb, bool) {
        let at = |x: usize, y: usize| self.sums[y * (self.width + 1) + x];
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let n = ((x1 - x0) * (y1 - y0)).max(1) as f64;
        let v: [f64; 4] = std::array::from_fn(|k| d[k] - b[k] - c[k] + a[k]);
        (std::array::from_fn(|k| (v[k] / n) as f32), v[3] > 0.)
    }
}

/// The JPEG sampled on a `width` x `height` grid covering it, scaled by
/// `scale` and bent by `radial` (1 + k0 r^2 + k1 r^4 of the half-diagonal: a
/// camera that corrects its lens in its JPEG) about the centre, and shifted by
/// `shift` proxy pixels.
fn sample(
    p: &Preview,
    integral: &Integral,
    [width, height]: [u32; 2],
    scale: f32,
    radial: [f32; 2],
    shift: [f32; 2],
) -> Sampled {
    let (sx, sy) = (
        p.width as f32 / width as f32,
        p.height as f32 / height as f32,
    );
    let n = (width * height) as usize;
    let mut pro = vec![[f32::NAN; 3]; n];
    let mut clipped = vec![true; n];
    for y in 0..height {
        for x in 0..width {
            let at = |v: u32, len: u32, d: f32, s: f32| {
                let c = len as f32 / 2.;
                ((v as f32 - c - d) / scale + c) * s
            };
            let (cx, cy) = (width as f32 / 2., height as f32 / 2.);
            let (dx, dy) = (
                x as f32 + 0.5 - cx - shift[0],
                y as f32 + 0.5 - cy - shift[1],
            );
            let r2 = (dx * dx + dy * dy) / (cx * cx + cy * cy);
            let bend = r2 * (radial[0] + r2 * radial[1]) / scale;
            let (bx, by) = (dx * bend * sx, dy * bend * sy);
            let (u0, u1) = (
                at(x, width, shift[0], sx) + bx,
                at(x + 1, width, shift[0], sx) + bx,
            );
            let (v0, v1) = (
                at(y, height, shift[1], sy) + by,
                at(y + 1, height, shift[1], sy) + by,
            );
            if u0 < 0. || v0 < 0. || u1 > p.width as f32 || v1 > p.height as f32 {
                continue;
            }
            let x0 = u0.round() as usize;
            let y0 = v0.round() as usize;
            let x1 = (u1.round() as usize).max(x0 + 1).min(p.width as usize);
            let y1 = (v1.round() as usize).max(y0 + 1).min(p.height as usize);
            let i = (y * width + x) as usize;
            (pro[i], clipped[i]) = integral.mean([x0, y0, x1, y1]);
        }
    }
    Sampled { pro, clipped }
}

/// Log luminance less its 5 x 5 mean: the detail two renders of one scene
/// share whatever their tone curves.
fn detail(lum: &[f32], width: u32, height: u32) -> Vec<f32> {
    let (w, h) = (width as i32, height as i32);
    let log: Vec<f32> = lum.iter().map(|v| (v.max(0.) + 1e-3).ln()).collect();
    let mut out = vec![f32::NAN; log.len()];
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            let mut sum = 0.;
            for dy in -2..=2 {
                for dx in -2..=2 {
                    sum += log[((y + dy) * w + x + dx) as usize];
                }
            }
            let i = (y * w + x) as usize;
            out[i] = log[i] - sum / 25.;
        }
    }
    out
}

/// The centre half and the four quadrants of a `width` x `height` image.
fn regions(width: u32, height: u32) -> [[u32; 4]; 5] {
    let (w, h) = (width, height);
    [
        [w / 4, h / 4, 3 * w / 4, 3 * h / 4],
        [0, 0, w / 2, h / 2],
        [w / 2, 0, w, h / 2],
        [0, h / 2, w / 2, h],
        [w / 2, h / 2, w, h],
    ]
}

fn correlation(a: &[f32], b: &[f32], width: u32, [x0, y0, x1, y1]: [u32; 4]) -> f32 {
    let mut n = 0f64;
    let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * width + x) as usize;
            let (p, q) = (f64::from(a[i]), f64::from(b[i]));
            if !p.is_finite() || !q.is_finite() {
                continue;
            }
            n += 1.;
            sa += p;
            sb += q;
            saa += p * p;
            sbb += q * q;
            sab += p * q;
        }
    }
    if n < 16. {
        return 0.;
    }
    let cov = sab / n - sa * sb / (n * n);
    let va = saa / n - sa * sa / (n * n);
    let vb = sbb / n - sb * sb / (n * n);
    if va <= 0. || vb <= 0. {
        return 0.;
    }
    (cov / (va * vb).sqrt()) as f32
}

/// The least correlation for a photo to line up: the centre and every
/// quadrant at `MIN_CORRELATION` ends the search; else the centre at
/// `MIN_CENTRE` and each quadrant at `MIN_QUADRANT`: a quadrant of sky or
/// blurred background has little detail to correlate, and a few pixels off in
/// it matter little to a colour fit, which the held-out tiles judge anyway.
const MIN_CORRELATION: f32 = 0.85;
const MIN_CENTRE: f32 = 0.8;
const MIN_QUADRANT: f32 = 0.45;

/// The JPEG aligned with the proxy (scales 0.97-1.03, then 0.88-1.12 with
/// radial distortion, shifts of up to 2 proxy pixels), or why it can't be:
/// another picture, cropped or distorted unlike the photo, or an aspect ratio
/// more than 2% from the photo's.
pub(super) fn align(p: &Preview, proxy: &Proxy) -> Result<(Sampled, Alignment), Refusal> {
    let (w, h) = (proxy.width, proxy.height);
    let ratio = (p.width as f32 / p.height as f32) / (w as f32 / h as f32);
    if (ratio - 1.).abs() > 0.02 {
        return Err(refuse(format!(
            "The preview's aspect ratio differs from the photo's by {:.1}%",
            (ratio - 1.).abs() * 100.
        )));
    }
    let integral = Integral::new(p);
    let lum: Vec<f32> = proxy
        .linear
        .iter()
        .zip(&proxy.valid)
        .map(|(c, v)| {
            if *v {
                crate::colour::luma(*c)
            } else {
                f32::NAN
            }
        })
        .collect();
    let ours = detail(&lum, w, h);
    let regions = regions(w, h);
    // The best of `tries` (scale, radial, shift), the first of equals, and `best`.
    let search = |tries: Vec<(f32, [f32; 2], [f32; 2])>, best: Option<(f32, Alignment)>| {
        let scored: Vec<(f32, Alignment)> = tries
            .into_par_iter()
            .map(|(scale, radial, shift)| {
                let sampled = sample(p, &integral, [w, h], scale, radial, shift);
                let lum: Vec<f32> = sampled.pro.iter().map(|q| luminance(*q)).collect();
                let theirs = detail(&lum, w, h);
                let regions = regions.map(|region| correlation(&ours, &theirs, w, region));
                let alignment = Alignment {
                    scale,
                    radial,
                    shift,
                    regions,
                };
                (regions.iter().sum::<f32>() / 5., alignment)
            })
            .collect();
        scored.into_iter().fold(best, |best, tried| match best {
            Some((mean, _)) if mean >= tried.0 => best,
            _ => Some(tried),
        })
    };
    let scale_of = |s: i32| 1. + s as f32 * 0.01;
    let shifts = || (-2..=2).flat_map(|dy| (-2..=2).map(move |dx| [dx as f32, dy as f32]));
    // Scale and shift; then, only where that isn't enough, the lens: scale and
    // the r^2 term, the r^4 term with it, and the shift again.
    let tries = (-3..=3)
        .flat_map(|s| shifts().map(move |shift| (scale_of(s), [0., 0.], shift)))
        .collect();
    let mut best = search(tries, None);
    let lined_up = |best: &Option<(f32, Alignment)>| {
        best.is_some_and(|(_, a)| a.regions.iter().all(|r| *r >= MIN_CORRELATION))
    };
    if !lined_up(&best) {
        let at = best.expect("searched").1;
        let tries = (-12..=12)
            .flat_map(|s| (-16..=16).map(move |k| (scale_of(s), [k as f32 * 0.01, 0.], at.shift)))
            .collect();
        best = search(tries, best);
        let at = best.expect("searched").1;
        let tries = (-5..=5)
            .flat_map(|k1| {
                (-2..=2).map(move |k0| {
                    let radial = [at.radial[0] + k0 as f32 * 0.01, k1 as f32 * 0.02];
                    (at.scale, radial, at.shift)
                })
            })
            .collect();
        best = search(tries, best);
        let at = best.expect("searched").1;
        let tries = shifts().map(|shift| (at.scale, at.radial, shift)).collect();
        best = search(tries, best);
        // The coarse steps can leave the frame's edges a pixel out: finer around them.
        let at = best.expect("searched").1;
        let tries = (-2..=2)
            .flat_map(|s| {
                (-2..=2).flat_map(move |k0| {
                    (-2..=2).map(move |k1| {
                        let radial = [
                            at.radial[0] + k0 as f32 * 0.004,
                            at.radial[1] + k1 as f32 * 0.008,
                        ];
                        (at.scale + s as f32 * 0.004, radial, at.shift)
                    })
                })
            })
            .collect();
        best = search(tries, best);
    }
    let (_, alignment) = best.expect("searched");
    let worst = alignment.regions[1..]
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    if alignment.regions[0] < MIN_CENTRE || worst < MIN_QUADRANT {
        let worst = worst.min(alignment.regions[0]);
        let mut refusal = refuse(format!(
            "The preview does not line up with the photo (detail correlation {worst:.2})"
        ));
        refusal.report.alignment = alignment;
        return Err(refusal);
    }
    let sampled = sample(
        p,
        &integral,
        [w, h],
        alignment.scale,
        alignment.radial,
        alignment.shift,
    );
    Ok((sampled, alignment))
}

/// Luminance (Y) of linear ProPhoto RGB.
pub(super) fn luminance(p: Rgb) -> f32 {
    0.288_04 * p[0] + 0.711_874 * p[1] + 0.000_086 * p[2]
}

/// A strong edge: this step of encoded luminance across 3 x 3 pixels. A ratio
/// would refuse the shadows' small steps, and with them the curve's blacks.
const EDGE_STEP: f32 = 0.3;

/// Which pixels a fit may learn from: inside the photo, unclipped by the
/// sensor and in the JPEG, not on a strong edge ([`EDGE_STEP`]), not in the
/// corners vignetting darkens (beyond 0.85 of the half-diagonal).
pub(super) fn usable(proxy: &Proxy, sampled: &Sampled) -> Vec<bool> {
    let (w, h) = (proxy.width as i32, proxy.height as i32);
    let lum: Vec<f32> = sampled.pro.iter().map(|p| luminance(*p)).collect();
    let half_diagonal = ((w * w + h * h) as f32).sqrt() / 2.;
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let i = i as usize;
            if !proxy.valid[i]
                || proxy.sensor_clipped[i]
                || sampled.clipped[i]
                || !sampled.pro[i].iter().all(|v| v.is_finite())
            {
                return false;
            }
            let (dx, dy) = (
                x as f32 + 0.5 - w as f32 / 2.,
                y as f32 + 0.5 - h as f32 / 2.,
            );
            if (dx * dx + dy * dy).sqrt() > 0.85 * half_diagonal {
                return false;
            }
            let (mut lo, mut hi) = (f32::INFINITY, 0f32);
            for ny in (y - 1).max(0)..=(y + 1).min(h - 1) {
                for nx in (x - 1).max(0)..=(x + 1).min(w - 1) {
                    let v = lum[(ny * w + nx) as usize];
                    if !v.is_finite() {
                        return false;
                    }
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            srgb_encode(hi.clamp(0., 1.)) - srgb_encode(lo.clamp(0., 1.)) <= EDGE_STEP
        })
        .collect()
}

/// The held-out third: whole tiles of an 8 x 8 grid (every third along each
/// row, staggered by row), so a held-out pixel's neighbours are mostly held
/// out too.
pub(super) fn holdout(width: u32, height: u32) -> Vec<bool> {
    (0..width * height)
        .map(|i| {
            let (tx, ty) = (i % width * 8 / width, i / width * 8 / height);
            (tx + 2 * ty) % 3 == 0
        })
        .collect()
}

/// Whether the JPEG (`srgb`, linear) is black and white: at most one usable
/// pixel in a thousand has channels further apart than 0.002.
pub(super) fn monochrome(srgb: &[Rgb], usable: &[bool]) -> bool {
    let (mut n, mut coloured) = (0usize, 0usize);
    for (p, u) in srgb.iter().zip(usable) {
        if !*u {
            continue;
        }
        n += 1;
        let spread = p.iter().fold(f32::NEG_INFINITY, |a, v| a.max(*v))
            - p.iter().fold(f32::INFINITY, |a, v| a.min(*v));
        coloured += usize::from(spread > 0.002);
    }
    n > 0 && coloured * 1000 <= n
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A scene of coloured blobs at the stage's size, linear sRGB.
    pub fn stage_of_blobs(width: u32, height: u32) -> Stage {
        stage_of(width, height, |x, y| {
            let (x, y) = (x / 1.5, y / 1.5);
            [
                0.25 + 0.2 * (x * 0.21).sin() * (y * 0.17).cos(),
                0.2 + 0.15 * (x * 0.05 + y * 0.09).sin(),
                0.18 + 0.15 * (x * 0.13 - y * 0.07).cos(),
            ]
        })
    }
    /// Another picture: diagonal stripes.
    pub fn stage_of_stripes(width: u32, height: u32) -> Stage {
        stage_of(width, height, |x, y| {
            let v = 0.2 + 0.15 * ((x + 2. * y) / 23.).sin();
            [v, v * 0.9, v * 1.1]
        })
    }
    fn stage_of(width: u32, height: u32, f: impl Fn(f32, f32) -> Rgb) -> Stage {
        let linear: Vec<Rgb> = (0..width * height)
            .map(|i| f((i % width) as f32, (i / width) as f32))
            .collect();
        Stage {
            width,
            height,
            clipped: vec![false; linear.len()],
            linear,
        }
    }
    /// The JPEG a camera would make of `stage` through `render` (linear sRGB
    /// in and out): 8-bit sRGB, read back as a preview.
    pub fn preview_through(stage: &Stage, render: impl Fn(Rgb) -> Rgb) -> Preview {
        let bytes: Vec<u8> = stage
            .linear
            .iter()
            .flat_map(|p| render(*p).map(|v| (srgb_encode(v) * 255. + 0.5) as u8))
            .collect();
        Preview::from_rgb8(stage.width, stage.height, &bytes, Space::Srgb)
    }

    fn preview(width: u32, height: u32, f: impl Fn(u32, u32) -> f32) -> Preview {
        Preview {
            width,
            height,
            pro: (0..width * height)
                .map(|i| [f(i % width, i / width); 3])
                .collect(),
            clipped: vec![false; (width * height) as usize],
        }
    }
    /// A scene of blobs, the same seen at any size.
    fn scene(x: f32, y: f32) -> f32 {
        0.2 + 0.15 * (x * 0.21).sin() * (y * 0.17).cos() + 0.1 * (x * 0.05 + y * 0.09).sin()
    }

    /// A proxy whose luminance is `p`'s, sampled as given.
    fn proxy_of(
        p: &Preview,
        w: u32,
        h: u32,
        scale: f32,
        radial: [f32; 2],
        shift: [f32; 2],
    ) -> Proxy {
        let integral = Integral::new(p);
        let s = sample(p, &integral, [w, h], scale, radial, shift);
        // Grey: its luminance in sRGB is its luminance in ProPhoto.
        let linear: Vec<Rgb> = s.pro.iter().map(|q| [luminance(*q); 3]).collect();
        Proxy {
            width: w,
            height: h,
            valid: linear.iter().map(|q| q[0].is_finite()).collect(),
            sensor_clipped: vec![false; (w * h) as usize],
            linear,
        }
    }

    #[test]
    fn a_shifted_and_scaled_preview_is_found_where_it_is() {
        let p = preview(960, 640, |x, y| scene(x as f32 / 5., y as f32 / 5.));
        let proxy = proxy_of(&p, 192, 128, 1.02, [0., 0.], [1., -2.]);
        let (_, a) = align(&p, &proxy).unwrap_or_else(|e| panic!("{}", e.reason));
        assert_eq!((a.scale, a.shift), (1.02, [1., -2.]), "{a:?}");
        assert!(a.regions.iter().all(|r| *r > 0.99), "{a:?}");
    }

    #[test]
    fn a_lens_the_camera_corrected_in_its_jpeg_still_lines_up() {
        let p = preview(960, 640, |x, y| scene(x as f32 / 5., y as f32 / 5.));
        let proxy = proxy_of(&p, 192, 128, 1.01, [0.08, 0.03], [0., 0.]);
        let (_, a) = align(&p, &proxy).unwrap_or_else(|e| panic!("{}", e.reason));
        assert_ne!(a.radial, [0., 0.], "{a:?}");
        assert!(a.regions.iter().all(|r| *r >= MIN_CORRELATION), "{a:?}");
    }

    #[test]
    fn another_picture_or_aspect_is_refused() {
        let p = preview(960, 640, |x, y| scene(x as f32 / 5., y as f32 / 5.));
        let other = preview(960, 640, |x, y| scene(y as f32 / 3. + 40., x as f32 / 7.));
        let proxy = proxy_of(&other, 192, 128, 1., [0., 0.], [0., 0.]);
        assert!(align(&p, &proxy).is_err());
        let cropped = preview(900, 640, |x, y| scene(x as f32 / 5., y as f32 / 5.));
        let proxy = proxy_of(&p, 192, 128, 1., [0., 0.], [0., 0.]);
        let refused = align(&cropped, &proxy).err().expect("refused");
        assert!(refused.reason.contains("aspect"), "{}", refused.reason);
    }

    #[test]
    fn a_letterboxed_preview_is_cut_to_the_photo() {
        // A square photo letterboxed in a 4:3 preview, black at the sides.
        let p = preview(960, 720, |x, y| {
            if (120..840).contains(&x) {
                scene(x as f32 / 5., y as f32 / 5.)
            } else {
                0.
            }
        });
        let cut = to_aspect(p, 128, 128);
        assert_eq!([cut.width, cut.height], [720, 720]);
        let proxy = proxy_of(&cut, 128, 128, 1., [0., 0.], [0., 0.]);
        assert!(align(&cut, &proxy).is_ok());
    }

    #[test]
    fn clipped_edges_and_corners_are_not_learned_from() {
        let p = preview(96, 64, |x, _| if x < 48 { 0.1 } else { 0.5 });
        let mut proxy = proxy_of(&p, 24, 16, 1., [0., 0.], [0., 0.]);
        proxy.sensor_clipped[16 * 24 / 2 + 3] = true;
        let integral = Integral::new(&p);
        let s = sample(&p, &integral, [24, 16], 1., [0., 0.], [0., 0.]);
        let u = usable(&proxy, &s);
        let at = |x: usize, y: usize| u[y * 24 + x];
        assert!(at(6, 8) && at(18, 8), "flat areas");
        assert!(!at(3, 8), "sensor clipped");
        assert!(!at(11, 8) && !at(12, 8), "the edge");
        assert!(!at(0, 0) && !at(23, 15), "the corners");
    }

    #[test]
    fn a_third_is_held_out_in_tiles() {
        let h = holdout(192, 128);
        let share = h.iter().filter(|v| **v).count() as f32 / h.len() as f32;
        assert!((share - 1. / 3.).abs() < 0.05, "{share}");
        assert_eq!(h[0], h[1]);
        assert_eq!(h[0], h[192]);
    }

    /// Adobe RGB's white is white and its primaries reach past sRGB's: its
    /// pure green has negative red and blue in linear sRGB.
    #[test]
    fn an_adobe_rgb_jpeg_is_read_in_its_own_primaries() {
        let white = Preview::from_rgb8(1, 1, &[255, 255, 255], Space::AdobeRgb);
        let w = crate::colour::mul(crate::colour::PROPHOTO_TO_SRGB, white.pro[0]);
        assert!(w.iter().all(|v| (v - 1.).abs() < 2e-3), "{w:?}");
        let green = Preview::from_rgb8(1, 1, &[0, 255, 0], Space::AdobeRgb);
        let g = crate::colour::mul(crate::colour::PROPHOTO_TO_SRGB, green.pro[0]);
        assert!(g[0] < -0.1 && g[1] > 1. && g[2] < 0., "{g:?}");
        let mid = Preview::from_rgb8(1, 1, &[128, 128, 128], Space::AdobeRgb).pro[0][1];
        assert!(
            (mid - (128f32 / 255.).powf(563. / 256.)).abs() < 2e-3,
            "{mid}"
        );
    }

    #[test]
    fn a_grey_jpeg_is_black_and_white() {
        let grey = vec![[0.3f32; 3]; 2000];
        let usable = vec![true; 2000];
        assert!(monochrome(&grey, &usable));
        let mut tinted = grey.clone();
        for p in tinted.iter_mut().step_by(10) {
            p[0] = 0.35;
        }
        assert!(!monochrome(&tinted, &usable));
    }
}
