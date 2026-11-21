//! A fitted look and how it renders: the CPU twin of the RAW tone stage's
//! mode 5 (`ag_stage_tone`, modules.wgsl).
//!
//! From RAWmakase `v0.7.1` @ 78069aa3 (MIT):
//! `crates/rawmakase-model/src/camera_profiles/mod.rs` `Table::apply`,
//! `Table::lookup` and `CameraProfile::finish`; `enhanced.rs` `apply_curve`;
//! `fitted.rs` `FittedLook` and `TABLE_DIMS`. `Table::apply` is the DNG SDK's
//! HSV table (RawTherapee's `rtengine/dcp.cc` `hsdApply` does the same).

use super::Rgb;
use crate::colour::{srgb_decode, srgb_encode};
use serde::{Deserialize, Serialize};

/// The look table's size: hues, saturations, values.
pub const TABLE_DIMS: [usize; 3] = [72, 8, 8];
/// Samples of a tone curve over 0..1, and of the look's curve.
pub const CURVE_SAMPLES: usize = 4097;
const STEPS: usize = CURVE_SAMPLES - 1;

/// A nondecreasing tone curve of linear light over 0..1, sampled, with its
/// inverse: DNG Standard's for a photo (its exposure offset and Adobe's
/// curve), the base a look is fitted over.
#[derive(Clone, Debug)]
pub struct Tone {
    lut: Vec<f32>,
    inverse: Vec<f32>,
}

impl Tone {
    /// `f`, nondecreasing on 0..1, sampled.
    pub fn from_fn(f: impl Fn(f32) -> f32) -> Self {
        let lut: Vec<f32> = (0..=STEPS).map(|i| f(i as f32 / STEPS as f32)).collect();
        let inverse = inverse(|v| at(&lut, v));
        Self { lut, inverse }
    }
    /// The curve at linear `x`.
    pub fn at(&self, x: f32) -> f32 {
        at(&self.lut, x)
    }
    pub(super) fn inverse_at(&self, v: f32) -> f32 {
        at(&self.inverse, v)
    }
    /// `CameraProfile::finish`'s rule: the darkest and brightest channels of
    /// `pro` (clamped to 0..1) through the curve, the middle one kept in its
    /// place between them: the DNG SDK's RGBTone, the stage's `rt_rgb_tone`.
    pub fn rgb_tone(&self, pro: Rgb) -> Rgb {
        extrema(pro.map(|v| v.clamp(0., 1.)), |v| self.at(v))
    }
}

/// A monotone LUT of `CURVE_SAMPLES` over 0..1 at `x`, linearly between samples.
pub(super) fn at(lut: &[f32], x: f32) -> f32 {
    let p = x.clamp(0., 1.) * STEPS as f32;
    let i = (p as usize).min(STEPS - 1);
    lut[i] + (lut[i + 1] - lut[i]) * (p - i as f32)
}

/// The inverse of the nondecreasing `f` on 0..1, as a LUT: for each output the
/// middle of the inputs that reach it.
pub(super) fn inverse(f: impl Fn(f32) -> f32) -> Vec<f32> {
    let ys: Vec<f32> = (0..=STEPS).map(|i| f(i as f32 / STEPS as f32)).collect();
    (0..=STEPS)
        .map(|j| {
            let v = j as f32 / STEPS as f32;
            let i = ys.partition_point(|y| *y < v);
            if i == 0 {
                return 0.;
            }
            if i > STEPS {
                return 1.;
            }
            let (a, b) = (ys[i - 1], ys[i]);
            let t = if b > a { (v - a) / (b - a) } else { 0.5 };
            (i as f32 - 1. + t) / STEPS as f32
        })
        .collect()
}

/// The darkest and brightest channels of `p` through `f`, the middle one kept
/// in its place between them.
pub(super) fn extrema(p: Rgb, f: impl Fn(f32) -> f32) -> Rgb {
    let lo = p.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = p.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let (a, b) = (f(lo), f(hi));
    if hi - lo > 1e-8 {
        p.map(|v| a + (b - a) * (v - lo) / (hi - lo))
    } else {
        [a; 3]
    }
}

/// Hue (0..1), saturation and value as profile tables take them.
pub(super) fn hsv(p: Rgb) -> Rgb {
    let max = p.iter().copied().fold(0f32, f32::max);
    let min = p.iter().copied().fold(f32::INFINITY, f32::min).max(0.);
    let d = max - min;
    let h = if d < 1e-9 {
        0.
    } else if max == p[0] {
        ((p[1] - p[2]) / d).rem_euclid(6.) / 6.
    } else if max == p[1] {
        ((p[2] - p[0]) / d + 2.) / 6.
    } else {
        ((p[0] - p[1]) / d + 4.) / 6.
    };
    [h, if max > 0. { d / max } else { 0. }, max]
}

/// `p` (linear ProPhoto) times the grey gains.
pub(super) fn gained(p: Rgb, gains: [f32; 3]) -> Rgb {
    [p[0] * gains[0], p[1] * gains[1], p[2] * gains[2]]
}

/// A fitted look: per table node (value outermost, then hue, then saturation,
/// as DNG tables are stored) a hue shift in degrees and saturation and value
/// scales; the look's tone curve sampled at `CURVE_SAMPLES` points of
/// sRGB-encoded input; and gains on linear ProPhoto applied before the table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub table: Vec<[f32; 3]>,
    pub curve: Vec<f32>,
    pub gains: [f32; 3],
}

impl Look {
    /// The look that changes nothing: it renders as DNG Standard.
    pub fn identity() -> Self {
        Self {
            table: vec![[0., 1., 1.]; TABLE_DIMS.iter().product()],
            curve: (0..=STEPS).map(|i| i as f32 / STEPS as f32).collect(),
            gains: [1.; 3],
        }
    }

    /// Whether the look is whole: the table's and the curve's sizes, finite
    /// values in the bounds a DNG table allows, a nondecreasing curve.
    pub fn is_valid(&self) -> bool {
        self.table.len() == TABLE_DIMS.iter().product::<usize>()
            && self.curve.len() == CURVE_SAMPLES
            && self.table.iter().all(|p| {
                p.iter().all(|v| v.is_finite())
                    && p[0].abs() <= 360.
                    && (0. ..=10.).contains(&p[1])
                    && (0. ..=10.).contains(&p[2])
            })
            && self.curve.iter().all(|v| v.is_finite())
            && self.curve.windows(2).all(|w| w[0] <= w[1])
            && self.gains.iter().all(|g| g.is_finite() && *g > 0.)
    }

    /// What the table makes of linear ProPhoto `pro`, clamped to 0..1 first.
    pub(super) fn table_output(&self, pro: Rgb) -> Rgb {
        apply_table(&self.table, pro.map(|v| v.clamp(0., 1.)))
    }

    /// The look's curve on the darkest and brightest sRGB-encoded channels.
    pub(super) fn apply_curve(&self, rgb: Rgb) -> Rgb {
        let p = rgb.map(|v| srgb_encode(v.clamp(0., 1.)));
        extrema(p, |v| at(&self.curve, v)).map(srgb_decode)
    }

    /// The look's render of what the RAW tone stage receives, in linear
    /// ProPhoto (before the gains): the gains, the table, DNG Standard's tone,
    /// the look's curve. Linear ProPhoto out.
    pub fn render(&self, base: &Tone, pro: Rgb) -> Rgb {
        let p = self.table_output(gained(pro, self.gains));
        self.apply_curve(base.rgb_tone(p))
    }

    /// The look as the RAW tone stage's buffer holds it: the table's nodes
    /// (three floats each, in their order), the curve, then the gains.
    pub fn to_gpu(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.table.len() * 3 + CURVE_SAMPLES + 3);
        out.extend(self.table.iter().flatten());
        out.extend(&self.curve);
        out.extend(self.gains);
        out
    }
}

/// The table at hue `h` (0..1), saturation `s` and encoded value `v`,
/// trilinearly between its nodes: RAWmakase `Table::lookup`.
fn lookup(table: &[[f32; 3]], h: f32, s: f32, v: f32) -> Rgb {
    let [nh, ns, nv] = TABLE_DIMS;
    let coords = [
        h.rem_euclid(1.) * nh as f32,
        s.clamp(0., 1.) * (ns - 1) as f32,
        v.clamp(0., 1.) * (nv - 1) as f32,
    ];
    let base = coords.map(|v| v.floor() as usize);
    let f: [f32; 3] = std::array::from_fn(|c| coords[c] - base[c] as f32);
    let mut out = [0.; 3];
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let ix = (base[0] + x) % nh;
                let iy = (base[1] + y).min(ns - 1);
                let iz = (base[2] + z).min(nv - 1);
                let weight = [x, y, z]
                    .into_iter()
                    .enumerate()
                    .map(|(c, n)| if n == 0 { 1. - f[c] } else { f[c] })
                    .product::<f32>();
                let p = table[(iz * nh + ix) * ns + iy];
                for c in 0..3 {
                    out[c] += p[c] * weight;
                }
            }
        }
    }
    out
}

/// The HSV table on `rgb` (linear ProPhoto), its value looked up sRGB-encoded:
/// RAWmakase `Table::apply` for an sRGB-encoded 3D table.
fn apply_table(table: &[[f32; 3]], rgb: Rgb) -> Rgb {
    let [h, sat, max] = hsv(rgb);
    let v = srgb_encode(max);
    let delta = lookup(table, h, sat, v);
    let h = (h + delta[0] / 360.).rem_euclid(1.) * 6.;
    let s = (sat * delta[1]).clamp(0., 1.);
    let v = srgb_decode(v * delta[2]);
    let c = v * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let rgb = match h as usize {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    rgb.map(|q| q + v - c)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A stand-in for a photo's DNG Standard tone: brighter, then a shoulder.
    pub fn base() -> Tone {
        Tone::from_fn(|x| 1. - (1. - (x * 1.8).min(1.)).powf(1.6))
    }

    /// A look with a curve and a hue-dependent shift: what a camera might do.
    pub fn known_look() -> Look {
        let mut known = Look::identity();
        known.curve = (0..=STEPS)
            .map(|i| {
                let x = i as f32 / STEPS as f32;
                x + 0.15 * x * (1. - x) * (1. - 2. * x)
            })
            .collect();
        let [nh, ns, nv] = TABLE_DIMS;
        for z in 0..nv {
            for x in 0..nh {
                for y in 1..ns {
                    let hue = x as f32 / nh as f32 * std::f32::consts::TAU;
                    known.table[(z * nh + x) * ns + y] =
                        [8. * hue.sin(), 1. + 0.2 * hue.cos(), 1. + 0.05 * hue.sin()];
                }
            }
        }
        known
    }

    #[test]
    fn the_identity_look_renders_as_dng_standard() {
        let base = base();
        let look = Look::identity();
        assert!(look.is_valid());
        for pro in [
            [0.2, 0.1, 0.05],
            [0.5, 0.5, 0.5],
            [0.05, 0.3, 0.6],
            [0.9, 0.95, 1.2],
        ] {
            let (a, b) = (base.rgb_tone(pro), look.render(&base, pro));
            assert!(
                a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3),
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn a_table_turns_hue_and_scales_saturation_and_value() {
        let mut table = vec![[0., 1., 1.]; TABLE_DIMS.iter().product()];
        for node in table.iter_mut() {
            *node = [120., 0.5, 1.];
        }
        // Red, turned 120 degrees to green, at half the saturation.
        let out = apply_table(&table, [0.5, 0., 0.]);
        let [h, s, v] = hsv(out);
        assert!(
            (h - 1. / 3.).abs() < 1e-4 && (s - 0.5).abs() < 1e-4,
            "{out:?}"
        );
        assert!((v - 0.5).abs() < 1e-4);
        // Grey has no hue to turn: only its value can move.
        let grey = apply_table(&table, [0.3; 3]);
        assert!(grey.iter().all(|v| (v - 0.3).abs() < 1e-5), "{grey:?}");
    }

    #[test]
    fn a_tone_and_its_inverse_meet() {
        let base = base();
        for x in [0.01f32, 0.1, 0.3, 0.5] {
            let y = base.at(x);
            assert!((base.inverse_at(y) - x).abs() < 2e-3, "{x} {y}");
        }
    }

    #[test]
    fn the_gpu_layout_is_table_then_curve_then_gains() {
        let mut look = known_look();
        look.gains = [1.01, 0.99, 1.02];
        let gpu = look.to_gpu();
        let nodes = TABLE_DIMS.iter().product::<usize>();
        assert_eq!(gpu.len(), nodes * 3 + CURVE_SAMPLES + 3);
        assert_eq!(gpu[3..6], look.table[1]);
        assert_eq!(gpu[nodes * 3 + 10], look.curve[10]);
        assert_eq!(gpu[gpu.len() - 3..], look.gains);
    }
}
