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
//! LINEAR: darktable's gradient
//!
//! Harvested from darktable `src/develop/masks/gradient.c`, the LUT in
//! `_gradient_get_mask`, at cb30520d0a5e23f094ef98d0578d903ddaa64804:
//!
//! ```c
//! value = 0.5f + 0.5f * erff(distance / compression);
//! ```
//!
//! darktable draws its two border lines at `±compression`, which is the role
//! RapidRAW's `range` plays: the distance from the centre line to each outer
//! handle. So the curve is taken with its meaning, not just its shape. At the
//! handles the mask is 92% and 8%, and it fades to nothing at about twice that
//! distance. In the middle it is 13% steeper than the old ramp (1/sqrt(pi)
//! against 1/2), so a mask keeps the place and width it was drawn with.
//!
//! RADIAL: smoothstep, not darktable
//!
//! darktable's ellipse falls off as `f * f`. That is smooth at the outer edge
//! and keeps a corner at the inner one, the very thing being removed. This is
//! the curve RapidRAW already gives a feathered brush in `mask_generation.rs`,
//! so a radial mask and a brush stroke now fade alike. Exactly 1 inside and 0
//! outside the feather, as before; only the shape between changes.

/// The linear mask's strength at `t`, the signed distance from its centre line
/// in units of `range`. Positive is towards the end handle, where it fades out.
pub fn linear(t: f32) -> f32 {
    0.5 - 0.5 * dt_erf(t)
}

/// The radial mask's strength, from RapidRAW's straight ramp: 1 at the inner
/// edge of the feather, 0 at the outer, and beyond either in between.
pub fn radial(ramp: f32) -> f32 {
    let s = ramp.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

// darktable calls C's erff, which Rust's stable library does not have.
// Abramowitz and Stegun 7.1.26: off by at most 1.5e-7, against the 1/255 a mask
// is stored to.
fn dt_erf(x: f32) -> f32 {
    let a = x.abs() as f64;
    let t = 1.0 / (1.0 + 0.327_591_1 * a);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - poly * (-a * a).exp();
    (y as f32).copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erf_matches_the_reference_values() {
        for (x, want) in [
            (0.0, 0.0),
            (0.5, 0.520_499_9),
            (1.0, 0.842_700_8),
            (2.0, 0.995_322_3),
        ] {
            assert!((dt_erf(x) - want).abs() < 1e-6, "erf({x}) = {}", dt_erf(x));
            assert!((dt_erf(-x) + want).abs() < 1e-6);
        }
    }

    #[test]
    fn linear_is_half_on_the_line_and_darktables_values_at_the_handles() {
        assert_eq!(linear(0.0), 0.5);
        assert!((linear(-1.0) - 0.921_35).abs() < 1e-4);
        assert!((linear(1.0) - 0.078_65).abs() < 1e-4);
        // Gone, to the 1/255 it is stored at, by about twice the handle distance.
        assert!(linear(2.1) < 0.5 / 255.0);
        assert!(linear(-2.1) > 1.0 - 0.5 / 255.0);
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
