//! Automatic white balance — illuminant detection from the image itself.
//!
//! Harvested from darktable `src/iop/channelmixerrgb.c` @ `98a9ade9`,
//! function `_auto_detect_WB()`. Reference copy kept at
//! `docs/harvest/dt_auto_detect_wb.c`.
//!
//! Based on:
//!   - *A Fast White Balance Algorithm Based on Pixel Greyness* —
//!     Ba Thai, Guang Deng, Robert Ross
//!   - *Edge-Based Color Constancy* —
//!     Joost van de Weijer, Theo Gevers, Arjan Gijsenij
//!
//! The idea: shift every pixel's chromaticity so the D50 white point sits at
//! the origin, then take a weighted average of what's left. Whatever the average
//! comes out as is the scene's illuminant — the colour cast we need to remove.
//!
//! Two weightings:
//!   - **Surfaces** weights each patch by the variance of both chroma channels
//!     times their covariance. That deliberately *discards* flat patches, which
//!     say nothing about the illuminant, and uncorrelated ones, which are noise
//!     or chromatic aberration rather than real surfaces. What survives is
//!     genuinely coloured texture.
//!   - **Edges** weights by edge strength instead — the grey-edge hypothesis,
//!     that image derivatives average to neutral. Holds up better when one
//!     colour dominates the frame and grey-world falls apart.

use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// Sampling stride, and the radius of the 3x3 neighbourhood. darktable's `OFF`.
/// Sampling every 4th pixel: ~16x less work for a result that doesn't visibly
/// differ, since we're averaging over the whole frame anyway.
const OFF: usize = 4;

/// Guard against division by zero. darktable's `NORM_MIN`.
const NORM_MIN: f32 = 1.525_878_9e-5; // 2^-16

/// D65 white point — the white of linear sRGB, and therefore what a correctly
/// balanced image should be adapted *to* here.
pub const D65_X: f32 = 0.312_726_9;
pub const D65_Y: f32 = 0.329_023_2;

/// The UI sliders run -100..100 but are divided by these before reaching the
/// shader — see `SCALES` in `image_processing.rs`. Keep in sync with it: if
/// upstream retunes the sliders, these must follow or auto-WB will be wrong by
/// exactly that factor.
const SLIDER_SCALE_TEMPERATURE: f32 = 25.0;
const SLIDER_SCALE_TINT: f32 = 100.0;

/// Linear sRGB (D65) to CIE XYZ. RapidRAW's pipeline works in linear sRGB,
/// so this is the matrix that gets us to a space where chromaticity means
/// something.
const SRGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectMode {
    /// Weighted by chroma variance and covariance — real coloured surfaces.
    /// The one that works, and the only one exposed in the UI.
    Surfaces,
    /// Grey-edge hypothesis.
    ///
    /// **Disabled in the UI.** The port is faithful to darktable line for line,
    /// but the result doesn't survive the move to this pipeline: it returns
    /// illuminants far off the locus (`xy = (0.125, 0.381)` on a test frame),
    /// pins tint at its limit, and moves the *wrong way* when a scene is warmed.
    ///
    /// Best guess: it accumulates **normalised** edge directions, so the
    /// magnitude of the result depends on how consistently edges point one way
    /// rather than on the light. darktable runs it in a different working space
    /// and further along its pipeline, where that presumably averages out.
    ///
    /// Kept rather than deleted — the maths is right, the context isn't. See
    /// the ignored test `edges_mode_also_responds_to_the_light`.
    Edges,
}

/// What the detector found, plus the slider values that reproduce it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoWhiteBalance {
    /// Detected illuminant chromaticity (CIE 1931 2° observer).
    pub x: f32,
    pub y: f32,
    /// Correlated colour temperature in kelvin, for display.
    pub temperature_k: f32,
    /// Values for the existing temperature/tint sliders, both -100..100.
    pub temperature: f32,
    pub tint: f32,
}

/// Per-pixel chromaticity, shifted so D50 is the origin.
/// `[x', y', Y]` — the first two are what we average, `Y` is kept for weighting.
struct Chromaticity {
    data: Vec<[f32; 3]>,
    width: usize,
    height: usize,
}

impl Chromaticity {
    #[inline]
    fn at(&self, row: usize, col: usize) -> &[f32; 3] {
        &self.data[row * self.width + col]
    }
}

/// Undo RapidRAW's default RAW preprocessing to get back to scene-linear.
///
/// `apply_cpu_default_raw_processing` (image_processing.rs) gamma-encodes and
/// boosts contrast before we ever see the pixels:
///
/// ```text
/// out = clamp((in^(1/2.38) - 0.5) * 1.28 + 0.5, 0, 1)
/// ```
///
/// darktable's detection assumes **scene-linear** RGB — its RGB→XYZ matrix is
/// only meaningful on linear data. Feeding it gamma-encoded pixels skews every
/// chromaticity, and unevenly per channel, which biases the detected illuminant.
/// This was measurably making results warmer than darktable's.
///
/// The curve itself lives in `mods::preview_encode`, which owns it and its
/// inverse. Two constants used to sit here as well, with a note to keep them in
/// step with upstream's — they had not been read by anything since the encode
/// gained a toe and the inverse moved, so the note was asking for maintenance
/// on a copy nobody was using.
#[inline]
fn to_scene_linear(value: f32) -> f32 {
    // Single source of truth: mods::preview_encode owns the curve and its
    // inverse. It used to be duplicated here as gamma-then-contrast, which was
    // fine while the encode was a straight line and wrong the moment it gained
    // a toe.
    crate::mods::preview_encode::decode(value)
}

/// Convert the image to D50-relative chromaticity.
///
/// darktable's first loop: RGB → XYZ → xyY, then translate so D50 lands on the
/// origin and normalise by the D50 vector's length. After this, "how far from
/// neutral is this pixel" is just its distance from zero.
fn to_chromaticity(image: &DynamicImage, linearise: bool) -> Chromaticity {
    let rgb = image.to_rgb32f();
    let (width, height) = (rgb.width() as usize, rgb.height() as usize);
    let norm = (D65_X * D65_X + D65_Y * D65_Y).sqrt();

    let mut data = Vec::with_capacity(width * height);
    for pixel in rgb.pixels() {
        // Clip negatives — highlight reconstruction and lens correction can
        // leave them, and they'd poison the average.
        let (r, g, b) = if linearise {
            (
                to_scene_linear(pixel[0]),
                to_scene_linear(pixel[1]),
                to_scene_linear(pixel[2]),
            )
        } else {
            (pixel[0].max(0.0), pixel[1].max(0.0), pixel[2].max(0.0))
        };

        let x_ = SRGB_TO_XYZ[0][0] * r + SRGB_TO_XYZ[0][1] * g + SRGB_TO_XYZ[0][2] * b;
        let y_ = SRGB_TO_XYZ[1][0] * r + SRGB_TO_XYZ[1][1] * g + SRGB_TO_XYZ[1][2] * b;
        let z_ = SRGB_TO_XYZ[2][0] * r + SRGB_TO_XYZ[2][1] * g + SRGB_TO_XYZ[2][2] * b;

        let sum = (x_ + y_ + z_).max(NORM_MIN);
        data.push([(x_ / sum - D65_X) / norm, (y_ / sum - D65_Y) / norm, y_]);
    }

    Chromaticity {
        data,
        width,
        height,
    }
}

/// The nine taps of the patch, at stride `OFF`, with their B-spline weights.
/// `(row offset, col offset, weight)` — 1-2-1 / 2-4-2 / 1-2-1.
const NEIGHBOURS: [(isize, isize, f32); 9] = [
    (-1, -1, 1.0),
    (-1, 0, 2.0),
    (-1, 1, 1.0),
    (0, -1, 2.0),
    (0, 0, 4.0),
    (0, 1, 2.0),
    (1, -1, 1.0),
    (1, 0, 2.0),
    (1, 1, 1.0),
];

/// One tap of the patch.
#[inline]
fn tap(chroma: &Chromaticity, row: usize, col: usize, dr: isize, dc: isize, channel: usize) -> f32 {
    let r = (row as isize + dr * OFF as isize) as usize;
    let c = (col as isize + dc * OFF as isize) as usize;
    chroma.at(r, c)[channel]
}

/// The 3x3 B-spline blur darktable uses for both modes.
#[inline]
fn local_average(chroma: &Chromaticity, row: usize, col: usize, channel: usize) -> f32 {
    let mut acc = 0.0f32;
    for (dr, dc, weight) in NEIGHBOURS {
        acc += weight * tap(chroma, row, col, dr, dc, channel);
    }
    acc / 16.0
}

/// Detect the scene illuminant. Returns its chromaticity in CIE xy.
fn detect_illuminant(
    image: &DynamicImage,
    mode: DetectMode,
    linearise: bool,
) -> Option<(f32, f32)> {
    let chroma = to_chromaticity(image, linearise);

    // Need room for the 3x3 neighbourhood at stride OFF, plus darktable's
    // margin. Tiny images can't be sampled meaningfully.
    if chroma.width < 8 * OFF || chroma.height < 8 * OFF {
        return None;
    }

    let mut sum = [0.0f64; 2];
    let mut elements = 0.0f64;

    let mut row = 2 * OFF;
    while row < chroma.height - 4 * OFF {
        let mut col = 2 * OFF;
        while col < chroma.width - 4 * OFF {
            match mode {
                DetectMode::Surfaces => {
                    // Average the patch, then weight it by how much genuine
                    // coloured *surface* it contains.
                    let mut central = [0.0f32; 2];
                    for (c, slot) in central.iter_mut().enumerate() {
                        *slot = local_average(&chroma, row, col, c);
                    }

                    // Variance of each chroma channel across the 9 taps.
                    // Zero variance means a flat patch, which carries no
                    // information about the illuminant — darktable discards it
                    // by letting the weight fall to zero.
                    let mut var = [0.0f32; 3];
                    for c in 0..2 {
                        let mut acc = 0.0f32;
                        for (dr, dc, _) in NEIGHBOURS {
                            let d = tap(&chroma, row, col, dr, dc, c) - central[c];
                            acc += d * d;
                        }
                        var[c] = acc / 9.0;
                    }

                    // Covariance between the two chroma channels. Near zero
                    // means they aren't correlated — noise or chromatic
                    // aberration rather than a real surface, so drop it too.
                    let mut cov = 0.0f32;
                    for (dr, dc, _) in NEIGHBOURS {
                        cov += (tap(&chroma, row, col, dr, dc, 0) - central[0])
                            * (tap(&chroma, row, col, dr, dc, 1) - central[1]);
                    }
                    var[2] = cov / 9.0;

                    // Minkowski p-norm of the *average* — with p = 8 this is
                    // near max(|x|,|y|), which normalises each patch's
                    // contribution by how far from neutral it already is.
                    let p = 8.0f32;
                    let p_norm = (central[0].abs().powf(p) + central[1].abs().powf(p))
                        .powf(1.0 / p)
                        + NORM_MIN;

                    let weight = var[0] * var[1] * var[2];

                    for c in 0..2 {
                        sum[c] += (central[c] * weight / p_norm) as f64;
                    }
                    elements += (weight / p_norm) as f64;
                }
                DetectMode::Edges => {
                    // Weight by edge strength instead: image minus blur.
                    let mut dd = [0.0f32; 2];
                    for (c, slot) in dd.iter_mut().enumerate() {
                        *slot = chroma.at(row, col)[c] - local_average(&chroma, row, col, c);
                    }

                    let p = 8.0f32;
                    let p_norm =
                        (dd[0].abs().powf(p) + dd[1].abs().powf(p)).powf(1.0 / p) + NORM_MIN;

                    // Note the subtraction — darktable's sign convention here.
                    for c in 0..2 {
                        sum[c] -= (dd[c] / p_norm) as f64;
                    }
                    elements += 1.0;
                }
            }
            col += OFF;
        }
        row += OFF;
    }

    if elements <= 0.0 {
        return None;
    }

    // Undo the normalisation and shift back from D50-relative to absolute xy.
    let norm_d65 = (D65_X * D65_X + D65_Y * D65_Y).sqrt();
    let x = norm_d65 * (sum[0] / elements) as f32 + D65_X;
    let y = norm_d65 * (sum[1] / elements) as f32 + D65_Y;

    if !x.is_finite() || !y.is_finite() || y.abs() < NORM_MIN {
        return None;
    }

    Some((x, y))
}

/// Correlated colour temperature from chromaticity — McCamy's approximation.
/// Display only; the correction itself uses xy directly.
fn cct_from_xy(x: f32, y: f32) -> f32 {
    let n = (x - 0.332_0) / (0.185_8 - y);
    (449.0 * n * n * n + 3525.0 * n * n + 6823.3 * n + 5520.33).clamp(1000.0, 25000.0)
}

/// Turn a detected illuminant into slider values.
///
/// This is now an exact inverse rather than an approximation. The shader
/// (`modules.wgsl`, `dt_white_balance`) turns the slider into an illuminant:
///
/// ```text
/// kelvin = 6500 * exp(t * 0.28)       then xy from the daylight locus
/// y      = y_locus + n * 0.05         tint shifts perpendicular
/// ```
///
/// So we run it backwards: kelvin from the detected chromaticity, `t` from
/// kelvin, then `n` from however far the detected `y` sits off the locus.
///
/// A warm scene detects as *low* kelvin, which gives a negative `t` — the
/// slider moves left and the picture cools. That's the right way round: the
/// slider says what you want the picture to look like, not what the scene was.
///
/// **Keep in step with `modules.wgsl`.** These two are inverses of each other
/// by hand, not by construction — change the curve there and auto-WB silently
/// stops agreeing with the sliders it sets.
fn solve_slider_values(x: f32, y: f32) -> (f32, f32) {
    // Recover kelvin from x ALONE. The forward model only ever moves y (tint),
    // so x still carries the pure temperature — inverting on x makes this an
    // exact inverse. Using cct_from_xy here instead was a real bug: McCamy's
    // CCT follows isotherms, so it reads a tinted point as a different colour
    // temperature, and the y_locus subtracted below was then taken at the wrong
    // kelvin. Round trip drifted ~15% at the ends (put in -60, got back -51).
    let kelvin = kelvin_from_locus_x(x);

    // kelvin = 6500 * exp(t * 0.28)  →  t = ln(kelvin / 6500) / 0.28
    let t = (kelvin / 6500.0).ln() / 0.28;

    // How far off the daylight locus the detected illuminant sits is the tint.
    let (_, y_locus) = kelvin_to_xy(kelvin);
    let n = (y - y_locus) / 0.05;

    // Shader values scale up to slider values — see SLIDER_SCALE_*.
    (
        (t * SLIDER_SCALE_TEMPERATURE).clamp(-100.0, 100.0),
        (n * SLIDER_SCALE_TINT).clamp(-100.0, 100.0),
    )
}

/// Invert the daylight locus: given an x chromaticity, the kelvin that put it
/// there. Bisection, because the cubic fits are not analytically invertible.
///
/// `x_locus` decreases monotonically as kelvin rises (hotter is bluer), which is
/// what makes bisection safe here.
fn kelvin_from_locus_x(x: f32) -> f32 {
    let (mut lo, mut hi) = (1800.0_f32, 20000.0_f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if kelvin_to_xy(mid).0 > x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Kelvin to CIE xy. Mirrors `ag_kelvin_to_xy` in `modules.wgsl` — daylight
/// locus above 4000K, Planckian below it where daylight isn't defined.
fn kelvin_to_xy(kelvin: f32) -> (f32, f32) {
    let t = kelvin.clamp(1800.0, 20000.0);
    let inv = 1000.0 / t;

    let x = if t < 4000.0 {
        -0.266_123_9 * inv * inv * inv - 0.234_358_9 * inv * inv + 0.877_695_6 * inv + 0.179_910
    } else if t <= 7000.0 {
        0.244_063 + 0.099_11 * inv + 2.967_8 * inv * inv - 4.607_0 * inv * inv * inv
    } else {
        0.237_040 + 0.247_48 * inv + 1.901_8 * inv * inv - 2.006_4 * inv * inv * inv
    };

    (x, -3.000 * x * x + 2.870 * x - 0.275)
}

/// Detect the illuminant and return both it and the slider values to apply it.
pub fn auto_white_balance(
    image: &DynamicImage,
    mode: DetectMode,
    linearise: bool,
) -> Option<AutoWhiteBalance> {
    let (x, y) = detect_illuminant(image, mode, linearise)?;
    let (temperature, tint) = solve_slider_values(x, y);

    Some(AutoWhiteBalance {
        x,
        y,
        temperature_k: cct_from_xy(x, y),
        temperature,
        tint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, Rgb32FImage};

    fn solid(width: u32, height: u32, colour: [f32; 3]) -> DynamicImage {
        let mut img = Rgb32FImage::new(width, height);
        for pixel in img.pixels_mut() {
            *pixel = Rgb(colour);
        }
        DynamicImage::ImageRgb32F(img)
    }

    /// A frame of coloured texture under a given light.
    ///
    /// Flat images are useless for testing surfaces mode — it weights patches by
    /// chroma variance and covariance precisely to discard them. So vary hue and
    /// brightness across the frame, then multiply by the illuminant, which is
    /// what a cast physically is.
    fn textured(width: u32, height: u32, light: [f32; 3]) -> DynamicImage {
        let mut img = Rgb32FImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let fx = x as f32 / width as f32;
            let fy = y as f32 / height as f32;

            // Coloured patches, correlated across channels the way a real
            // surface is — not noise, which the covariance term rejects.
            let base = 0.25 + 0.5 * ((fx * 9.0).sin() * (fy * 7.0).cos()).abs();
            let surface = [
                base * (0.7 + 0.3 * (fx * 5.0).sin().abs()),
                base * (0.7 + 0.3 * (fy * 4.0).cos().abs()),
                base * (0.7 + 0.3 * ((fx + fy) * 3.0).sin().abs()),
            ];

            *pixel = Rgb([
                surface[0] * light[0],
                surface[1] * light[1],
                surface[2] * light[2],
            ]);
        }
        DynamicImage::ImageRgb32F(img)
    }

    #[test]
    fn neutral_light_needs_almost_no_correction() {
        let img = textured(192, 192, [1.0, 1.0, 1.0]);
        let result = auto_white_balance(&img, DetectMode::Surfaces, false).expect("should detect");
        assert!(
            result.temperature.abs() < 35.0,
            "neutrally lit scene wanted a large correction: {}",
            result.temperature
        );
    }

    #[test]
    fn warm_light_is_detected_and_cooled() {
        // Same scene, lit warm. Should ask to cool it back.
        let neutral = auto_white_balance(
            &textured(192, 192, [1.0, 1.0, 1.0]),
            DetectMode::Surfaces,
            false,
        )
        .expect("should detect");
        let warm = auto_white_balance(
            &textured(192, 192, [1.25, 1.0, 0.7]),
            DetectMode::Surfaces,
            false,
        )
        .expect("should detect");

        assert!(
            warm.temperature < neutral.temperature,
            "warm light should pull temperature down: neutral {} vs warm {}",
            neutral.temperature,
            warm.temperature
        );
    }

    // Edges mode is disabled in the UI - see the note on DetectMode::Edges.
    // Kept and ignored so the failure is reproducible when someone picks it up.
    #[test]
    #[ignore = "edges mode returns implausible illuminants in this pipeline"]
    fn edges_mode_also_responds_to_the_light() {
        let neutral = auto_white_balance(
            &textured(192, 192, [1.0, 1.0, 1.0]),
            DetectMode::Edges,
            false,
        )
        .expect("should detect");
        let warm = auto_white_balance(
            &textured(192, 192, [1.25, 1.0, 0.7]),
            DetectMode::Edges,
            false,
        )
        .expect("should detect");

        assert!(
            warm.temperature < neutral.temperature,
            "edges mode should cool a warm scene too: neutral {} vs warm {}",
            neutral.temperature,
            warm.temperature
        );
    }

    #[test]
    fn a_flat_frame_yields_nothing_to_measure() {
        // Surfaces mode weights by chroma variance, so a frame with none should
        // decline rather than invent an illuminant.
        let img = solid(192, 192, [0.7, 0.45, 0.25]);
        assert!(
            auto_white_balance(&img, DetectMode::Surfaces, false).is_none(),
            "a flat frame has no surfaces to measure"
        );
    }

    #[test]
    fn tiny_images_are_rejected_rather_than_panicking() {
        let img = solid(8, 8, [0.5, 0.5, 0.5]);
        assert!(auto_white_balance(&img, DetectMode::Surfaces, false).is_none());
    }
}

/// Solve white balance from a single colour the user declared neutral.
///
/// The picker's counterpart to whole-image detection: instead of estimating the
/// illuminant from the whole frame, take the clicked colour *as* the
/// illuminant, and solve for the sliders that adapt it to the working white.
///
/// Expects **scene-linear** RGB, 0..1 — the same data auto-WB analyses.
///
/// It has to be that data and not the canvas pixel, and this is where the
/// picker was broken. It used to sample the *processed preview*: an image with
/// the current white balance already applied, along with exposure, curves and
/// everything else. Solving from that yields an illuminant which is then
/// assigned back to the sliders, changing the preview, so the next click sees
/// different pixels and lands somewhere else again. Clicking one spot
/// repeatedly never converged — it chased its own output.
///
/// `white_balance_at` feeds this from the geometry-only cache instead, which no
/// colour slider can touch. Same spot, same answer, every time.
///
/// Returns `None` when the sample is too dark for its chromaticity to mean
/// anything — near black, the ratios are all noise.
pub fn white_balance_from_neutral(r: f32, g: f32, b: f32) -> Option<AutoWhiteBalance> {
    let (r, g, b) = (r.max(0.0), g.max(0.0), b.max(0.0));

    let x_ = SRGB_TO_XYZ[0][0] * r + SRGB_TO_XYZ[0][1] * g + SRGB_TO_XYZ[0][2] * b;
    let y_ = SRGB_TO_XYZ[1][0] * r + SRGB_TO_XYZ[1][1] * g + SRGB_TO_XYZ[1][2] * b;
    let z_ = SRGB_TO_XYZ[2][0] * r + SRGB_TO_XYZ[2][1] * g + SRGB_TO_XYZ[2][2] * b;

    let sum = x_ + y_ + z_;
    // Below this the sample is essentially black and its hue is noise.
    if sum < 1e-4 {
        return None;
    }

    let x = x_ / sum;
    let y = y_ / sum;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }

    let (temperature, tint) = solve_slider_values(x, y);

    Some(AutoWhiteBalance {
        x,
        y,
        temperature_k: cct_from_xy(x, y),
        temperature,
        tint,
    })
}

#[cfg(test)]
mod picker_tests {
    use super::*;

    #[test]
    fn a_neutral_sample_asks_for_almost_no_correction() {
        let r = white_balance_from_neutral(0.5, 0.5, 0.5).expect("mid grey should solve");
        assert!(
            r.temperature.abs() < 15.0 && r.tint.abs() < 15.0,
            "grey wanted a big correction: temp {} tint {}",
            r.temperature,
            r.tint
        );
    }

    #[test]
    fn an_orange_sample_cools_the_picture() {
        let r = white_balance_from_neutral(0.6, 0.4, 0.2).expect("should solve");
        assert!(
            r.temperature < 0.0,
            "orange should cool, got {}",
            r.temperature
        );
    }

    #[test]
    fn black_is_rejected_rather_than_guessed_at() {
        assert!(white_balance_from_neutral(0.0, 0.0, 0.0).is_none());
    }
}

#[cfg(test)]
mod tint_direction_tests {
    use super::*;

    /// Locks the tint sign against the pipeline it feeds.
    ///
    /// This existed as a bug: `ag_apply_tint` in `modules.wgsl` lowered the
    /// illuminant's `y`, which makes the *picture* green, while the doc comment
    /// and RapidRAW's own `tint_mult = (1+t*.25, 1-t*.25, 1+t*.25)` both say
    /// positive tint is magenta. Nothing caught it because nothing tested a
    /// direction — only magnitudes.
    ///
    /// Convention being locked: sample a **green** cast, and the solver must ask
    /// for **positive** tint (push magenta) to cancel it. Magenta in, negative
    /// out. Flip the shader and this fails, which is the point.
    #[test]
    fn a_green_sample_asks_for_magenta() {
        let r = white_balance_from_neutral(0.35, 0.55, 0.35).expect("green should solve");
        assert!(
            r.tint > 0.0,
            "a green cast must ask for magenta (positive tint), got {}",
            r.tint
        );
    }

    #[test]
    fn a_magenta_sample_asks_for_green() {
        let r = white_balance_from_neutral(0.55, 0.35, 0.55).expect("magenta should solve");
        assert!(
            r.tint < 0.0,
            "a magenta cast must ask for green (negative tint), got {}",
            r.tint
        );
    }

    /// The shader and `solve_slider_values` are inverses written by hand, not by
    /// construction — the module doc says so. This asserts the round trip on the
    /// tint axis, so a change to one without the other is caught here rather
    /// than on a photograph.
    #[test]
    fn tint_survives_a_round_trip_through_the_shader_curve() {
        for slider in [-60.0_f32, -20.0, 20.0, 60.0] {
            let n = slider / SLIDER_SCALE_TINT;
            let kelvin = 6500.0_f32;
            let (x, y_locus) = kelvin_to_xy(kelvin);

            // Forward, as modules.wgsl does it: y = y_locus + n * 0.05
            let y = y_locus + n * 0.05;

            let (_, tint_back) = solve_slider_values(x, y);
            assert!(
                (tint_back - slider).abs() < 2.0,
                "tint round trip drifted: put in {slider}, got back {tint_back}"
            );
        }
    }
}

/// Solve white balance from a point the user clicked, in normalised image
/// coordinates (0..1, origin top-left).
///
/// Samples the **geometry-only cached image** — the same pixels auto-WB uses.
/// That cache is keyed on crop and rotation alone, so no colour slider can move
/// it, which is exactly what the picker needs and exactly what sampling the
/// processed preview failed to give.
///
/// Averages an 11x11 patch to survive noise, the way the canvas sampling did.
pub fn white_balance_at(image: &DynamicImage, x: f32, y: f32) -> Option<AutoWhiteBalance> {
    const RADIUS: i64 = 5;

    let rgb = image.to_rgb32f();
    let (width, height) = (rgb.width() as i64, rgb.height() as i64);
    if width == 0 || height == 0 {
        return None;
    }

    let cx = (x.clamp(0.0, 1.0) * (width - 1) as f32).round() as i64;
    let cy = (y.clamp(0.0, 1.0) * (height - 1) as f32).round() as i64;

    let (mut r, mut g, mut b, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut encoded = 0.0f32;
    for row in (cy - RADIUS).max(0)..=(cy + RADIUS).min(height - 1) {
        for col in (cx - RADIUS).max(0)..=(cx + RADIUS).min(width - 1) {
            let p = rgb.get_pixel(col as u32, row as u32);
            // Brightness is judged on the encoded value, before decoding:
            // to_scene_linear(0.0) is ~0.005, not 0, because the contrast boost
            // lifts the floor. A black patch would otherwise pass the darkness
            // check and return a confident answer about noise.
            encoded = encoded.max(p[0]).max(p[1]).max(p[2]);
            // Same decode as the detector: this cache has been through
            // apply_cpu_default_raw_processing and is not scene-linear yet.
            r += to_scene_linear(p[0]);
            g += to_scene_linear(p[1]);
            b += to_scene_linear(p[2]);
            n += 1.0;
        }
    }

    if n == 0.0 || encoded < 0.02 {
        return None;
    }

    white_balance_from_neutral(r / n, g / n, b / n)
}

#[cfg(test)]
mod picker_point_tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    fn flat(colour: [u8; 3]) -> DynamicImage {
        let mut img = RgbImage::new(64, 64);
        for p in img.pixels_mut() {
            *p = Rgb(colour);
        }
        DynamicImage::ImageRgb8(img)
    }

    /// The property the old picker could not hold: the same point must give the
    /// same answer however many times you ask. It sampled the processed preview,
    /// so each click moved the sliders, which moved the pixels, which moved the
    /// next answer.
    #[test]
    fn the_same_point_always_gives_the_same_answer() {
        let img = flat([180, 150, 120]);
        let first = white_balance_at(&img, 0.5, 0.5).expect("should solve");
        for _ in 0..5 {
            let again = white_balance_at(&img, 0.5, 0.5).expect("should solve");
            assert!(
                (again.temperature - first.temperature).abs() < 0.01
                    && (again.tint - first.tint).abs() < 0.01,
                "picker drifted on repeat: {} / {} then {} / {}",
                first.temperature,
                first.tint,
                again.temperature,
                again.tint
            );
        }
    }

    #[test]
    fn an_orange_patch_cools_the_picture() {
        let img = flat([200, 140, 90]);
        let r = white_balance_at(&img, 0.5, 0.5).expect("should solve");
        assert!(
            r.temperature < 0.0,
            "orange should cool, got {}",
            r.temperature
        );
    }

    #[test]
    fn a_black_frame_is_declined() {
        assert!(white_balance_at(&flat([0, 0, 0]), 0.5, 0.5).is_none());
    }
}

#[cfg(test)]
mod neutralisation_tests {
    //! Does the model actually neutralise?
    //!
    //! Measured against darktable on a real file, a spot-white-balanced grey
    //! patch came back `191/196/191` — red and blue equal, so the temperature
    //! axis is right, but green 5 levels high. About 3%, and the same on a
    //! second photo under different light, so it is systematic rather than noise.
    //!
    //! Two candidates: the model (solving the wrong illuminant), or the pipeline
    //! (reading the pixel wrong before the solve). These tests pin the model, in
    //! pure arithmetic with no image and no GPU. If they pass, the residual is
    //! not in the maths and the pipeline is where to look.

    use super::*;

    const XYZ_TO_LMS: [[f32; 3]; 3] = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    const LMS_TO_XYZ: [[f32; 3]; 3] = [
        [0.9869929, -0.1470543, 0.1599627],
        [0.4323053, 0.5183603, 0.0492912],
        [-0.0085287, 0.0400428, 0.9684867],
    ];
    const XYZ_TO_SRGB: [[f32; 3]; 3] = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.969_266, 1.8760108, 0.0415560],
        [0.0556434, -0.2040259, 1.0572252],
    ];

    fn mul3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    fn xy_to_xyz(xy: (f32, f32)) -> [f32; 3] {
        let y = xy.1.max(NORM_MIN);
        [xy.0 / y, 1.0, (1.0 - xy.0 - xy.1) / y]
    }

    /// Mirrors `ag_chromatic_adapt` in modules.wgsl.
    fn adapt(xyz: [f32; 3], from: (f32, f32), to: (f32, f32)) -> [f32; 3] {
        let lf = mul3(&XYZ_TO_LMS, xy_to_xyz(from));
        let lt = mul3(&XYZ_TO_LMS, xy_to_xyz(to));
        let l = mul3(&XYZ_TO_LMS, xyz);
        mul3(
            &LMS_TO_XYZ,
            [
                l[0] * lt[0] / lf[0].max(NORM_MIN),
                l[1] * lt[1] / lf[1].max(NORM_MIN),
                l[2] * lt[2] / lf[2].max(NORM_MIN),
            ],
        )
    }

    /// Mirrors `ag_slider_to_kelvin` then `ag_apply_tint`, taking slider units.
    fn illuminant_from_sliders(temperature: f32, tint: f32) -> (f32, f32) {
        let t = temperature / SLIDER_SCALE_TEMPERATURE;
        let n = tint / SLIDER_SCALE_TINT;
        let kelvin = (6500.0 * (t * 0.28).exp()).clamp(1800.0, 20000.0);
        let (x, y_locus) = kelvin_to_xy(kelvin);
        (x, y_locus + n * 0.05)
    }

    /// The whole claim, end to end and without an image: take a grey card under
    /// some illuminant, solve the sliders from it, apply what the shader would
    /// apply, and the result must be neutral.
    #[test]
    fn a_grey_card_under_any_illuminant_comes_back_neutral() {
        // Real illuminants across the range: tungsten, warm, daylight, shade,
        // plus two deliberately off the locus in either direction.
        let cases = [
            (0.4476, 0.4074),
            (0.4091, 0.3940),
            (0.3457, 0.3585),
            (0.3127, 0.3290),
            (0.2952, 0.3048),
            (0.3457, 0.3300),
            (0.3127, 0.3500),
        ];

        for (ix, iy) in cases {
            let (temperature, tint) = solve_slider_values(ix, iy);

            // Round as the frontend does before handing values to the shader.
            let (temperature, tint) = (temperature.round(), tint.round());
            let assumed = illuminant_from_sliders(temperature, tint);

            // A grey card reflects the illuminant, so the scene pixel is it.
            let patch = xy_to_xyz((ix, iy));
            let adapted = adapt(patch, assumed, (D65_X, D65_Y));
            let rgb = mul3(&XYZ_TO_SRGB, adapted);

            let max = rgb[0].max(rgb[1]).max(rgb[2]);
            let min = rgb[0].min(rgb[1]).min(rgb[2]);
            let cast = (max - min) / max.max(NORM_MIN);

            assert!(
                cast < 0.02,
                "illuminant ({ix}, {iy}) left a {:.1}% cast: rgb {:?}, sliders temp {temperature} tint {tint}",
                cast * 100.0,
                rgb
            );
        }
    }
}
