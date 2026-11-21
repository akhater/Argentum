//! Highlight reconstruction by Camera Raw's pull towards the other channels.
//!
//! From RAWmakase 8665ebda (MIT), `crates/rawmakase-engine/src/develop/quality/
//! highlights.rs`: `reconstruct` and its constants are theirs, unchanged there
//! up to v0.7.1 (later commits there only change how it is called). Ours: the
//! weight is read from a table (within 1e-7 of the power), and eight pixels
//! are pulled at a time.
//!
//! In white-balanced camera space every channel is pulled up towards the mean
//! of the other two, never down, by a weight that rises with that channel's own
//! sensor level `s` (its fraction of where the sensor stops recording):
//! `((s - 0.25) / 0.75)^1.8`. A clipped channel becomes the mean of the other
//! two, a channel near the white mostly follows them, a channel in the lower
//! quarter keeps its value, and where every channel is clipped the pixel comes
//! out neutral. It reads no neighbours. RAWmakase measured it on flat patches
//! rendered by Camera Raw 18.7, within 0.005 of the level per channel.
//!
//! WHERE IT RUNS
//!
//! After demosaic, before the white balance is applied: rawler calls it from
//! the develop (`RawDevelop::before_white_balance`, rawler fork), and it does
//! the balance and its inverse itself. Theirs reads a channel's sensor level
//! against the nominal white; ours against `highlights::ceilings`, where the
//! sensor actually stopped, which some files state too high (5D Mark II frames
//! that clip at 16033 of 16383).

use rayon::prelude::*;

/// The sensor fraction below which a channel is not pulled, and the exponent of
/// the weight's rise from there to the white.
const PULL_FLOOR: f32 = 0.25;
const PULL_POWER: f32 = 1.8;
/// Fixed-point iterations of the three pulls (the weights are at most 1 and the
/// targets are means, so the error shrinks by at least half each round).
const ROUNDS: usize = 8;

/// Steps of the weight's table over the rise from the floor to the white.
const WEIGHT_STEPS: usize = 4096;

/// `((s - 0.25) / 0.75)^1.8`, clamped to 0 and 1. Ours: read from a table
/// between the clamps, within 1e-7 of the power, because the power on every
/// channel of every bright pixel was most of the pull's time (a 24 MP frame
/// all in range took 450 ms).
fn weight(s: f32) -> f32 {
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let t = (s - PULL_FLOOR) / (1. - PULL_FLOOR);
    if t.is_nan() || t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let table = TABLE.get_or_init(|| {
        (0..=WEIGHT_STEPS)
            .map(|i| (i as f32 / WEIGHT_STEPS as f32).powf(PULL_POWER))
            .collect()
    });
    let at = t * WEIGHT_STEPS as f32;
    let i = (at as usize).min(WEIGHT_STEPS - 1);
    let f = at - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

/// Camera Raw's reconstruction of one white-balanced pixel: `sensor` is each
/// channel's fraction of the sensor's white (1 and above is clipped). The rule
/// as RAWmakase states it, pixel by pixel; `apply` runs the same arithmetic
/// eight pixels at a time, and a test holds the two equal bit for bit.
#[cfg(test)]
fn reconstruct(p: [f32; 3], sensor: [f32; 3]) -> [f32; 3] {
    let weight = sensor.map(weight);
    let mut x = p;
    for _ in 0..ROUNDS {
        for c in 0..3 {
            let mean = (x[(c + 1) % 3] + x[(c + 2) % 3]) * 0.5;
            x[c] = p[c] + weight[c] * (mean - p[c]).max(0.);
        }
    }
    x
}

/// What turns a camera value, as rawler's develop holds it before white
/// balance, into each channel's fraction of where that channel stops
/// recording: a factor per channel, red, green and blue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorScale([f32; 3]);

impl SensorScale {
    /// rawler scaled each sample as `(raw - black) / (scaled_white - black)`,
    /// and channel `c` stops recording at `ceilings[c]` (raw counts, from
    /// `highlights::ceilings`). `None` for levels that say nothing.
    pub fn new(ceilings: [f32; 4], black: f32, scaled_white: f32) -> Option<Self> {
        let span = scaled_white - black;
        let factor = |ceiling: f32| {
            let range = ceiling - black;
            (range > 0. && span > 0.).then(|| span / range)
        };
        Some(Self([
            factor(ceilings[0])?,
            factor(ceilings[1])?,
            factor(ceilings[2])?,
        ]))
    }
}

/// Pixels pulled together: each round of the pull is a chain of dependent
/// steps within a pixel, and side by side the chains of eight pixels run at
/// once. Ours; the arithmetic is `reconstruct`'s.
const LANES: usize = 8;

/// Reconstruct every pixel of `pixels`, camera values before white balance,
/// in place: balanced by `wb` (the coefficients rawler is about to apply),
/// pulled, and unbalanced again, so rawler's own balance lands on the result.
/// A channel the pull does not raise keeps its bits.
pub fn apply(pixels: &mut [[f32; 3]], wb: &[f32; 4], scale: SensorScale) {
    let wb = [wb[0], wb[1], wb[2]];
    if wb.iter().any(|w| !(w.is_finite() && *w > 0.)) {
        return;
    }
    pixels.par_chunks_mut(LANES * 512).for_each(|run| {
        run.chunks_mut(LANES)
            .for_each(|lanes| pull(lanes, wb, scale))
    });
}

/// Up to `LANES` pixels, as `apply` takes them.
fn pull(pixels: &mut [[f32; 3]], wb: [f32; 3], scale: SensorScale) {
    let mut p = [[0f32; LANES]; 3];
    let mut w = [[0f32; LANES]; 3];
    let mut any = false;
    for (l, pixel) in pixels.iter().enumerate() {
        for c in 0..3 {
            p[c][l] = pixel[c] * wb[c];
            w[c][l] = weight(pixel[c] * scale.0[c]);
            any |= w[c][l] > 0.;
        }
    }
    if !any {
        return;
    }
    let mut x = p;
    for _ in 0..ROUNDS {
        for c in 0..3 {
            let (a, b) = (x[(c + 1) % 3], x[(c + 2) % 3]);
            for l in 0..LANES {
                let mean = (a[l] + b[l]) * 0.5;
                x[c][l] = p[c][l] + w[c][l] * (mean - p[c][l]).max(0.);
            }
        }
    }
    for (l, pixel) in pixels.iter_mut().enumerate() {
        for c in 0..3 {
            if x[c][l] != p[c][l] {
                pixel[c] = x[c][l] / wb[c];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    // RAWmakase's tests of `reconstruct`, as they are at 8665ebda.

    #[test]
    fn a_clipped_channel_becomes_the_mean_of_the_other_two() {
        let w = (0.25f32 / 0.75).powf(PULL_POWER);
        let g = 0.5 / (1. - w / 2.);
        assert!(close(
            reconstruct([1., 0.5, 0.5], [1., 0.5, 0.5]),
            [1., g, g]
        ));
        let r = reconstruct([1.4, 1., 1.2], [0.7, 1., 0.85]);
        assert!(
            close(r, [1.4, (1.4 + r[2]) / 2., r[2]]) && r[2] > 1.3,
            "{r:?}"
        );
    }

    #[test]
    fn the_pull_is_upwards_only_and_the_recorded_value_is_a_floor() {
        let r = reconstruct([1., 0.4, 0.2], [1., 0.4, 0.2]);
        assert_eq!(r[0], 1.);
        assert!(r[1] > 0.4 && r[1] < 0.43 && r[2] == 0.2, "{r:?}");
        assert_eq!(reconstruct([1.; 3], [1.; 3]), [1.; 3]);
        let r = reconstruct([1., 1., 0.9], [1., 1., 0.9]);
        let w = (0.65f32 / 0.75).powf(PULL_POWER);
        assert!(close(r, [1., 1., 0.9 + w * 0.1]), "{r:?}");
    }

    // Ours: the step as rawler calls it.

    /// Their third test, `dark_pixels_are_untouched_and_white_balance_sets_the
    /// _sensor_fraction`, before white balance: a sensor white of 1 per channel.
    #[test]
    fn dark_pixels_keep_their_bits_and_white_balance_sets_the_sensor_fraction() {
        let wb = [2., 1., 1.5, f32::NAN];
        let mut pixels = [[0.15, 0.1, 0.2 / 1.5], [0.8, 1., 0.9]];
        let dark = pixels[0];
        apply(&mut pixels, &wb, SensorScale([1.; 3]));
        assert_eq!(pixels[0].map(f32::to_bits), dark.map(f32::to_bits));
        let p = pixels[1];
        let balanced = [p[0] * 2., p[1], p[2] * 1.5];
        assert!(
            (balanced[0] - 1.6).abs() < 1e-6
                && balanced[1] > 1.5
                && balanced[1] < 1.6
                && balanced[2] > 1.35,
            "{balanced:?}"
        );
    }

    /// `apply`, eight pixels at a time, gives each pixel exactly what
    /// `reconstruct` gives it alone, dark and bright, in a run whose length is
    /// not a multiple of eight.
    #[test]
    fn eight_at_a_time_is_the_rule_pixel_by_pixel() {
        let (wb, scale) = ([2.1, 1., 1.6, 0.], SensorScale([1.1, 1., 0.95]));
        let mut state = 7u32;
        let mut next = move || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1 << 24) as f32 * 1.3
        };
        let pixels: Vec<[f32; 3]> = (0..1003).map(|_| [next(), next(), next()]).collect();
        let mut pulled = pixels.clone();
        apply(&mut pulled, &wb, scale);
        for (before, after) in pixels.iter().zip(&pulled) {
            let sensor = std::array::from_fn(|c| before[c] * scale.0[c]);
            let balanced = std::array::from_fn(|c| before[c] * wb[c]);
            let want = reconstruct(balanced, sensor);
            for c in 0..3 {
                let expected = if want[c] != balanced[c] {
                    want[c] / wb[c]
                } else {
                    before[c]
                };
                assert_eq!(after[c].to_bits(), expected.to_bits(), "{before:?}");
            }
        }
    }

    /// The table stays within 1e-7 of RAWmakase's power everywhere.
    #[test]
    fn the_weight_is_the_power_within_a_ten_millionth() {
        let worst = (0..=100_000)
            .map(|i| 0.2 + 0.85 * i as f32 / 100_000.)
            .map(|s| {
                let exact = ((s - PULL_FLOOR) / (1. - PULL_FLOOR))
                    .clamp(0., 1.)
                    .powf(PULL_POWER);
                (weight(s) - exact).abs()
            })
            .fold(0f32, f32::max);
        assert!(worst < 1e-7, "{worst}");
    }

    /// A channel that stopped below the stated white counts as clipped there.
    #[test]
    fn the_sensor_level_is_read_against_where_the_channel_stopped() {
        // Black 1024, stated white 16383, green stopped at 16033: a green
        // sample at 16033 is at its white, not at 98% of it.
        let black = 1024.;
        let scale = SensorScale::new([16383., 16033., 16383., 16033.], black, 16383.).unwrap();
        let green_at_its_stop = (16033. - black) / (16383. - black);
        assert!((green_at_its_stop * scale.0[1] - 1.).abs() < 1e-6);
        assert_eq!(scale.0[0], 1.);
        assert!(SensorScale::new([black; 4], black, 16383.).is_none());
    }

    /// A blown pixel, every channel at its white, comes out neutral after
    /// white balance rather than magenta.
    #[test]
    fn a_blown_pixel_comes_out_neutral_after_white_balance() {
        let wb = [2.1, 1., 1.6, 0.];
        let mut pixels = [[1., 1., 1.]];
        apply(&mut pixels, &wb, SensorScale([1.; 3]));
        let balanced = [pixels[0][0] * wb[0], pixels[0][1], pixels[0][2] * wb[2]];
        let spread = balanced.iter().fold(0f32, |m, v| m.max(*v))
            - balanced.iter().fold(f32::MAX, |m, v| m.min(*v));
        assert!(spread < 0.01 * balanced[0], "{balanced:?}");
    }
}
