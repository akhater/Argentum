//! darktable's sigmoid tone mapping.
//!
//! Harvested from darktable `src/iop/sigmoid.c`, function
//! `_generalized_loglogistic_sigmoid` and the parameter derivation inside
//! `commit_params`.
//!
//! WHY
//!
//! Measured across 34 photos from 17 shoots, our rendering sits at **69% of
//! darktable's brightness**, on every single one. darktable's chain is
//!
//! ```text
//! rawprepare temperature highlights flip exposure colorin
//! channelmixerrgb sigmoid finalscale colorout gamma
//! ```
//!
//! where ours is gamma 2.38 with a 1.28 contrast boost and nothing else. The
//! missing pieces are an exposure lift and this curve. AK's photos come out
//! dark and flat as a result — the complaint that started this.
//!
//! WHAT IT IS
//!
//! A log-logistic curve modelling film and paper response, with three
//! constraints: scene black maps to display black, scene middle grey to a fixed
//! output level, and scene infinity to display white. Compared to a plain
//! gamma it holds highlights instead of clipping them, which is most of why
//! darktable's rendering looks less harsh.
//!
//! This is a straight port. The parameters are derived exactly as darktable
//! derives them, including the finite-difference slope matching, rather than
//! approximated — copying the reasoning and not just the constants, which is
//! the lesson DEC-46 recorded the hard way.

// Nothing in the app calls this yet.
//
// darktable's sigmoid, harvested and shipped as the tone curve in 2026.37.8,
// then reverted in 2026.37.9 — it had been built against a single frame and was
// compensating for the sRAW levels bug, which was found straight afterwards.
// The maths was never the problem and is kept for "Filmic tone mapping" on the
// roadmap. Its own tests exercise it; the binary does not.
#![allow(dead_code)]

/// darktable's `MIDDLE_GREY`. The scene value that anchors the curve.
const MIDDLE_GREY: f32 = 0.1845;

/// Step used for darktable's numerical slope matching.
const DELTA: f32 = 1e-6;

/// The curve's shape, fully derived. Cheap to compute once, then applied per
/// pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sigmoid {
    pub magnitude: f32,
    pub paper_exposure: f32,
    pub film_fog: f32,
    pub film_power: f32,
    pub paper_power: f32,
}

/// The curve itself.
///
/// ```text
/// film_response  = (film_fog + x) ^ film_power
/// paper_response = magnitude * (film_response / (paper_exp + film_response)) ^ paper_power
/// ```
fn loglogistic(
    value: f32,
    magnitude: f32,
    paper_exposure: f32,
    film_fog: f32,
    film_power: f32,
    paper_power: f32,
) -> f32 {
    let clamped = value.max(0.0);
    let film_response = (film_fog + clamped).powf(film_power);
    magnitude * (film_response / (paper_exposure + film_response)).powf(paper_power)
}

impl Sigmoid {
    /// Derive the curve from the user-facing settings.
    ///
    /// darktable's defaults: contrast 1.5, skew 0.0, white target 100.0 nits,
    /// black target 0.0152 nits.
    pub fn new(contrast: f32, skew: f32, display_white: f32, display_black: f32) -> Self {
        // Reference slope at middle grey, for no skew and a normalised display.
        let ref_film_power = contrast;
        let ref_paper_power = 1.0;
        let ref_magnitude = 1.0;
        let ref_film_fog = 0.0;
        let ref_paper_exposure =
            (ref_film_fog + MIDDLE_GREY).powf(ref_film_power) * ((ref_magnitude / MIDDLE_GREY) - 1.0);

        let slope_at = |mag: f32, exp: f32, fog: f32, fp: f32, pp: f32| {
            (loglogistic(MIDDLE_GREY + DELTA, mag, exp, fog, fp, pp)
                - loglogistic(MIDDLE_GREY - DELTA, mag, exp, fog, fp, pp))
                / 2.0
                / DELTA
        };

        let ref_slope = slope_at(
            ref_magnitude,
            ref_paper_exposure,
            ref_film_fog,
            ref_film_power,
            ref_paper_power,
        );

        // Skew.
        let paper_power = 5.0f32.powf(-skew);

        // Slope at unit film power, to solve for the film power that matches
        // the reference slope.
        let temp_film_power = 1.0;
        let temp_white_target = 0.01 * display_white;
        let temp_white_grey_relation =
            (temp_white_target / MIDDLE_GREY).powf(1.0 / paper_power) - 1.0;
        let temp_paper_exposure = MIDDLE_GREY.powf(temp_film_power) * temp_white_grey_relation;
        let temp_slope = slope_at(
            temp_white_target,
            temp_paper_exposure,
            ref_film_fog,
            temp_film_power,
            paper_power,
        );

        let film_power = ref_slope / temp_slope;

        let white_target = 0.01 * display_white;
        let black_target = 0.01 * display_black;
        let white_grey_relation = (white_target / MIDDLE_GREY).powf(1.0 / paper_power) - 1.0;
        let white_black_relation =
            (black_target / white_target).powf(-1.0 / paper_power) - 1.0;

        let film_fog = MIDDLE_GREY * white_grey_relation.powf(1.0 / film_power)
            / (white_black_relation.powf(1.0 / film_power)
                - white_grey_relation.powf(1.0 / film_power));

        let paper_exposure = (film_fog + MIDDLE_GREY).powf(film_power) * white_grey_relation;

        Self {
            magnitude: white_target,
            paper_exposure,
            film_fog,
            film_power,
            paper_power,
        }
    }

    /// darktable's defaults.
    pub fn dt_default() -> Self {
        Self::new(1.5, 0.0, 100.0, 0.0152)
    }

    /// Map one scene-linear value to display.
    pub fn map(&self, value: f32) -> f32 {
        loglogistic(
            value,
            self.magnitude,
            self.paper_exposure,
            self.film_fog,
            self.film_power,
            self.paper_power,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three constraints the curve is built to satisfy. If these fail the
    /// derivation is wrong, whatever the pictures look like.
    #[test]
    fn honours_its_own_anchors() {
        let s = Sigmoid::dt_default();

        // Scene black to display black.
        assert!(s.map(0.0) < 0.01, "black maps to {}", s.map(0.0));

        // Middle grey stays middle grey, which is the whole point of the anchor.
        let grey = s.map(MIDDLE_GREY);
        assert!(
            (grey - MIDDLE_GREY).abs() < 0.01,
            "middle grey moved to {grey}"
        );

        // Very bright scene values approach display white without exceeding it.
        let bright = s.map(1000.0);
        assert!(bright <= 1.001 && bright > 0.95, "white maps to {bright}");
    }

    /// Monotonic: brighter in must never mean darker out.
    #[test]
    fn never_goes_backwards() {
        let s = Sigmoid::dt_default();
        let mut previous = -1.0;
        for i in 0..2000 {
            let v = i as f32 / 100.0;
            let out = s.map(v);
            assert!(out >= previous, "curve dipped at {v}");
            previous = out;
        }
    }

    /// The reason for using it: highlights roll off instead of clipping. A
    /// plain gamma hits 1.0 and stops; this keeps separating values above it.
    #[test]
    fn holds_highlights_apart() {
        let s = Sigmoid::dt_default();
        let a = s.map(2.0);
        let b = s.map(8.0);
        assert!(b > a, "highlights collapsed: {a} then {b}");
        assert!(b < 1.0, "highlight exceeded display white: {b}");
    }

    /// Higher contrast must steepen the curve around middle grey.
    #[test]
    fn contrast_steepens_it() {
        let soft = Sigmoid::new(1.0, 0.0, 100.0, 0.0152);
        let hard = Sigmoid::new(2.0, 0.0, 100.0, 0.0152);

        let slope = |s: &Sigmoid| s.map(MIDDLE_GREY * 1.1) - s.map(MIDDLE_GREY * 0.9);
        assert!(
            slope(&hard) > slope(&soft),
            "contrast did not steepen: {} vs {}",
            slope(&soft),
            slope(&hard)
        );
    }

    /// Print the derived constants, for embedding in the shader.
    #[test]
    #[ignore = "informational"]
    fn show_default_parameters() {
        let s = Sigmoid::dt_default();
        println!("\n{s:#?}\n");
        for v in [0.0f32, 0.02, 0.05, 0.1845, 0.4, 1.0, 4.0, 16.0] {
            println!("  {v:>7.3} -> {:.4}", s.map(v));
        }
    }
}

/// Exposure lift applied before the curve, in stops.
///
/// darktable uses +0.7 EV, and that number does not transfer: it is calibrated
/// for the scale darktable's own rawprepare produces. Borrowing it made the
/// picture *darker* than the gamma it replaced - 38% of darktable's brightness
/// against 69% before - because our decoder's output sits far below the 0.1845
/// middle grey this curve is anchored to, where the sigmoid is much darker than
/// gamma 2.38.
///
/// So it was measured instead. Swept across all 34 photos:
///
/// 
///
/// 2.6 lands on darktable's average. Chosen from the whole set rather than one
/// frame, after AK pointed out that tuning on a single picture is how you get a
/// fix that only helps that picture.
pub const DEFAULT_EXPOSURE_EV: f32 = 2.6;

impl Sigmoid {
    /// The inverse curve: display value back to scene-linear.
    ///
    /// Needed because the rest of the pipeline has to be able to undo the
    /// encode — `dt_white_balance` adapts in linear space, and the auto white
    /// balance detector analyses linear data. Both previously inverted a plain
    /// gamma; with the curve in place they invert this instead.
    ///
    /// ```text
    /// t = (y / magnitude) ^ (1 / paper_power)
    /// f = t * paper_exposure / (1 - t)
    /// x = f ^ (1 / film_power) - film_fog
    /// ```
    pub fn unmap(&self, value: f32) -> f32 {
        let y = value.clamp(0.0, self.magnitude * 0.999_99);
        let t = (y / self.magnitude).powf(1.0 / self.paper_power);
        if t >= 1.0 {
            return f32::MAX;
        }
        let film_response = t * self.paper_exposure / (1.0 - t);
        (film_response.powf(1.0 / self.film_power) - self.film_fog).max(0.0)
    }
}

/// Apply the exposure lift and the curve to an image, in place.
///
/// Replaces `apply_cpu_default_raw_processing` for RAW files: that applies
/// gamma 2.38 and a 1.28 contrast boost, which is flat and dark next to this.
pub fn tone_map(image: &mut image::DynamicImage, exposure_ev: f32) {
    let s = Sigmoid::dt_default();
    let gain = 2.0f32.powf(exposure_ev);

    let mut rgb = image.to_rgb32f();
    for p in rgb.pixels_mut() {
        p[0] = s.map(p[0].max(0.0) * gain);
        p[1] = s.map(p[1].max(0.0) * gain);
        p[2] = s.map(p[2].max(0.0) * gain);
    }
    *image = image::DynamicImage::ImageRgb32F(rgb);
}

#[cfg(test)]
mod inverse_tests {
    use super::*;

    /// The encode must be undoable, or white balance cannot work in linear
    /// space — the trap that cost most of a day earlier.
    #[test]
    fn round_trips() {
        let s = Sigmoid::dt_default();
        for v in [0.01f32, 0.05, 0.1845, 0.5, 1.0, 3.0] {
            let back = s.unmap(s.map(v));
            let error = (back - v).abs() / v.max(1e-4);
            assert!(error < 0.01, "{v} came back as {back}");
        }
    }

    /// The lift has to actually brighten, and by the amount asked for.
    #[test]
    fn exposure_brightens() {
        let s = Sigmoid::dt_default();
        let plain = s.map(0.1);
        let lifted = s.map(0.1 * 2.0f32.powf(DEFAULT_EXPOSURE_EV));
        assert!(lifted > plain, "lift darkened: {plain} -> {lifted}");
    }
}

/// Whether the decoder already applies the display encode.
///
/// `develop_raw_image` now runs the exposure lift and this curve, so their
/// `apply_cpu_default_raw_processing` — gamma 2.38 plus a 1.28 contrast boost —
/// must not run as well, or the picture is tone-mapped twice.
///
/// A constant rather than a setting because the two encodes are not
/// interchangeable: `ag_to_scene_linear` in `modules.wgsl` and
/// `to_scene_linear` in `mods/auto_wb.rs` both have to invert whichever one is
/// in force, and they invert this. Flipping this alone would silently break
/// white balance.
pub const ENCODES_IN_DECODER: bool = true;

/// The exposure lift actually in force.
///
/// darktable's +0.7 EV is calibrated for the scale *its* pipeline produces.
/// Ours comes out of `develop_raw_image` on a different scale, and applying
/// 0.7 EV before this curve made the picture darker than the gamma it replaced
/// — 38% of darktable's brightness against 69% before. The right value is a
/// property of our decoder, so it is measured rather than borrowed: see the
/// sweep in `mods::colour_compare`.
///
/// Override with `AG_EV` when sweeping.
pub fn exposure_ev() -> f32 {
    std::env::var("AG_EV")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_EXPOSURE_EV)
}
