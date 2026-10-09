//! How a linear or radial mask fades out.
//!
//! WHY
//!
//! RapidRAW draws both as a straight ramp clamped to 0..1. The clamp leaves a
//! corner where the ramp meets full strength and another where it meets none,
//! and the eye reads a corner in a smooth gradient as a faint line (a Mach
//! band). A graduated sky showed two edges, one at each outer handle, so the
//! mask looked like a band with sides rather than a fade. AK's words: no fall
//! off.
//!
//! LINEAR: smoothstep between the two lines
//!
//! The mask's two outer lines (`range` either side of its centre line) are
//! where the effect is full and where it has gone, and Argentum draws them as
//! exactly that (`src/argentum/LinearMask.tsx`). So the curve has to be 100% on
//! one and 0% on the other, and level at both, so neither shows as a line.
//! Smoothstep is that curve.
//!
//! darktable's gradient (`0.5 + 0.5 * erf(d / compression)`) was tried first
//! and dropped: it is 92% and 8% at its lines and carries on past them, so the
//! lines stopped meaning where the fade starts and ends.
//!
//! RADIAL: the same curve, across the feather
//!
//! The curve RapidRAW already gives a feathered brush in `mask_generation.rs`,
//! so a radial mask and a brush stroke fade alike. darktable's ellipse
//! (`f * f`) keeps a corner at the inner edge, so it was not taken. Exactly 1
//! inside and 0 outside the feather, as before; only the shape between changes.

/// The linear mask's strength at `t`, the signed distance from its centre line
/// in units of `range`: 1 at -1, 0 at +1.
pub fn linear(t: f32) -> f32 {
    radial((1.0 - t) * 0.5)
}

/// The radial mask's strength, from RapidRAW's straight ramp: 1 at the inner
/// edge of the feather, 0 at the outer, and beyond either in between.
pub fn radial(ramp: f32) -> f32 {
    let s = ramp.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_exactly_full_and_none_on_its_lines() {
        assert_eq!(linear(-1.0), 1.0);
        assert_eq!(linear(0.0), 0.5);
        assert_eq!(linear(1.0), 0.0);
        assert_eq!(linear(-2.5), 1.0);
        assert_eq!(linear(2.5), 0.0);
    }

    #[test]
    fn neither_curve_has_a_corner() {
        // A corner is a jump in slope. Walk each curve and compare the slope on
        // either side of every point; the old ramp jumped by 0.5 at each handle.
        let step = 1e-3_f32;
        let largest_jump = |f: &dyn Fn(f32) -> f32, from: f32, to: f32| {
            let mut worst = 0.0_f32;
            let mut x = from;
            while x < to {
                let left = (f(x) - f(x - step)) / step;
                let right = (f(x + step) - f(x)) / step;
                worst = worst.max((right - left).abs());
                x += step;
            }
            worst
        };
        assert!(largest_jump(&linear, -4.0, 4.0) < 0.01);
        assert!(largest_jump(&radial, -0.5, 1.5) < 0.01);
        let old = |t: f32| (0.5 - 0.5 * t).clamp(0.0, 1.0);
        assert!(
            largest_jump(&old, -4.0, 4.0) > 0.4,
            "the test can see a corner"
        );
    }

    #[test]
    fn radial_keeps_its_bounds() {
        assert_eq!(radial(1.7), 1.0);
        assert_eq!(radial(1.0), 1.0);
        assert_eq!(radial(0.5), 0.5);
        assert_eq!(radial(0.0), 0.0);
        assert_eq!(radial(-3.0), 0.0);
    }

    #[test]
    fn both_fade_the_right_way() {
        let mut x = -3.0_f32;
        while x < 3.0 {
            assert!(linear(x + 0.01) <= linear(x));
            assert!(radial(x + 0.01) >= radial(x));
            x += 0.01;
        }
    }
}
