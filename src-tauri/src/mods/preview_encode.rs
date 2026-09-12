//! The preview encode, with a toe instead of a cliff.
//!
//! THE PROBLEM
//!
//! `apply_cpu_default_raw_processing` gamma-encodes a RAW and then applies a
//! contrast boost:
//!
//! ```text
//! g = x^(1/2.38)
//! y = (g - 0.5) * 1.28 + 0.5      then clamped to [0, 1]
//! ```
//!
//! That second line is a straight line, `y = 1.28g - 0.14`, so it *must* cross
//! zero — at `g = 0.109`, which is scene-linear 0.00516. Everything below that
//! is clamped to exactly black before the GPU ever sees the image, and no
//! exposure, white balance or curve can bring it back.
//!
//! Measured on one of AK's backlit frames:
//!
//! ```text
//! pure black before the encode   40.5%
//! pure black after  the encode   51.4%
//! destroyed by the encode        10.9%
//! ```
//!
//! and the resulting tone distribution against darktable on the same file:
//!
//! ```text
//!                bin 0-15   midtones
//! darktable         6.2%      17.8%
//! argentum         62.6%       9.4%
//! ```
//!
//! It also explains why some photos look fine and others are ruined, which AK
//! noticed long before any of this was measured: a bright evenly-lit frame has
//! little data down there, a backlit one loses half of itself.
//!
//! THE FIX
//!
//! Keep their curve exactly where it already works, and replace the part that
//! clips with a toe that reaches zero smoothly.
//!
//! Above a knee at `g = 0.25` this is their line, unchanged — so midtones and
//! highlights render exactly as before and every tool downstream still sees what
//! it was tuned for. Below the knee it follows a quadratic chosen to match the
//! line's value *and* slope at the knee and to pass through the origin, so there
//! is no visible join and nothing is clipped.
//!
//! Deliberately not a new look: this is the smallest change that stops throwing
//! shadow detail away. Whether to then render those shadows more like darktable
//! is a separate decision.

/// Gamma and contrast as `apply_cpu_default_raw_processing` uses them. Keep in
/// step with theirs, and with `to_scene_linear` in `mods/auto_wb.rs` and
/// `ag_to_scene_linear` in `shaders/modules.wgsl`, which both invert this.
pub const GAMMA: f32 = 2.38;
pub const CONTRAST: f32 = 1.28;

/// Where the toe takes over. Their line is `1.28g - 0.14`, which hits zero at
/// `g = 0.109`; the knee sits comfortably above that, so the join lands on a
/// part of the curve that was never clipped.
const KNEE: f32 = 0.25;

/// Quadratic coefficients for the toe, derived rather than tuned.
///
/// Requiring `y(0) = 0`, and value and slope continuity with `y = 1.28g - 0.14`
/// at the knee, gives `a = 0.14 / knee²` and `b = 1.28 - 2a·knee`.
const TOE_A: f32 = 0.14 / (KNEE * KNEE);
const TOE_B: f32 = CONTRAST - 2.0 * TOE_A * KNEE;

/// Their contrast line, for values above the knee.
#[inline]
fn line(g: f32) -> f32 {
    (g - 0.5) * CONTRAST + 0.5
}

/// One channel, gamma-encoded then contrasted, without the cliff.
#[inline]
pub fn encode(linear: f32) -> f32 {
    let g = linear.max(0.0).powf(1.0 / GAMMA);
    let y = if g >= KNEE {
        line(g)
    } else {
        TOE_A * g * g + TOE_B * g
    };
    // The top still clamps: above 1.0 is out of the display's range and their
    // pipeline expects it bounded. Only the bottom changes.
    // NOT `y.clamp(0.0, 1.0)`, which clippy asks for and which is not the same
    // function: `min`/`max` fall through to the other operand on NaN and answer
    // 1.0, where `clamp` answers NaN. Decoded frames do carry NaN - the white
    // balance array arrives with one in its fourth slot - and a NaN here reaches
    // the canvas as a zero-sized image. Left as it is on purpose.
    #[allow(clippy::manual_clamp)]
    y.min(1.0).max(0.0)
}

/// Undo `encode`. Needed by anything working in scene-linear — the illuminant
/// detector and the white balance shader both do.
#[inline]
pub fn decode(encoded: f32) -> f32 {
    let y = encoded.clamp(0.0, 1.0);
    let knee_y = line(KNEE);

    let g = if y >= knee_y {
        (y - 0.5) / CONTRAST + 0.5
    } else {
        // Invert a·g² + b·g = y, taking the positive root.
        let disc = TOE_B * TOE_B + 4.0 * TOE_A * y;
        (-TOE_B + disc.max(0.0).sqrt()) / (2.0 * TOE_A)
    };

    g.max(0.0).powf(GAMMA)
}

/// Apply the encode to a whole image, in place, replacing their CPU pass.
pub fn apply(image: &mut image::DynamicImage) {
    use rayon::prelude::*;

    let mut rgb = image.to_rgb32f();
    rgb.par_chunks_mut(3).for_each(|px| {
        px[0] = encode(px[0]);
        px[1] = encode(px[1]);
        px[2] = encode(px[2]);
    });
    *image = image::DynamicImage::ImageRgb32F(rgb);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: values that used to clip must survive.
    ///
    /// 0.00516 scene-linear is exactly where their line reaches zero. Anything
    /// at or below it came out pure black before.
    #[test]
    fn shadows_are_no_longer_clipped() {
        for linear in [1e-5f32, 1e-4, 0.001, 0.00516, 0.01] {
            let out = encode(linear);
            assert!(
                out > 0.0,
                "scene-linear {linear} still encodes to black ({out})"
            );
        }
    }

    /// Above the knee nothing changes, so the look of everything that already
    /// worked is preserved and downstream tools see what they expect.
    #[test]
    fn midtones_and_highlights_are_untouched() {
        for linear in [0.05f32, 0.1845, 0.4, 0.7, 1.0] {
            let g = linear.powf(1.0 / GAMMA);
            if g < KNEE {
                continue;
            }
            let theirs = ((g - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0);
            let ours = encode(linear);
            assert!(
                (theirs - ours).abs() < 1e-6,
                "changed a midtone: {linear} gave {ours}, theirs {theirs}"
            );
        }
    }

    /// No visible join at the knee — value and slope both continuous.
    #[test]
    fn the_toe_joins_smoothly() {
        let below = KNEE - 1e-4;
        let above = KNEE + 1e-4;
        let y_below = TOE_A * below * below + TOE_B * below;
        let y_above = line(above);
        assert!(
            (y_below - y_above).abs() < 1e-3,
            "step at the knee: {y_below} then {y_above}"
        );

        let slope_below = 2.0 * TOE_A * KNEE + TOE_B;
        assert!(
            (slope_below - CONTRAST).abs() < 1e-4,
            "slope kink at the knee: {slope_below} vs {CONTRAST}"
        );
    }

    /// Monotonic, or shadows would invert.
    #[test]
    fn never_goes_backwards() {
        let mut previous = -1.0;
        for i in 0..=2000 {
            let out = encode(i as f32 / 1000.0);
            assert!(out >= previous, "encode dipped at {i}");
            previous = out;
        }
    }

    /// Round trip, since the detector and the shader both invert this.
    ///
    /// Only up to where the encode reaches display white. Their line crosses
    /// 1.0 at scene-linear 0.757 and clamps there; that top clamp is existing
    /// behaviour and deliberately untouched, so nothing above it is
    /// recoverable — by design, not by accident.
    #[test]
    fn decode_undoes_encode() {
        for linear in [0.0f32, 1e-4, 0.005, 0.05, 0.1845, 0.5, 0.75] {
            let back = decode(encode(linear));
            assert!(
                (back - linear).abs() < 1e-3,
                "round trip lost {linear}, got {back}"
            );
        }
    }

    /// Black is still black. Lifting it would fog the picture.
    #[test]
    fn zero_stays_zero() {
        assert_eq!(encode(0.0), 0.0);
    }
}
