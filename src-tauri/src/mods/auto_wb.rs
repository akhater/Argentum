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

use glam::DVec3;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::image_processing::{PRIMARIES_SRGB, WP_D65, primaries_to_xyz_matrix};
use crate::white_balance::WhiteBalance;

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

/// What the detector found, and the white balance that removes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoWhiteBalance {
    /// Detected illuminant chromaticity (CIE 1931 2° observer).
    pub x: f32,
    pub y: f32,
    /// Correlated colour temperature of that illuminant in kelvin, for the log.
    pub temperature_k: f32,
    /// Kelvin and tint in RapidRAW 1.6.5's units, absolute, as their picker
    /// returns them. The frontend writes it the way their picker does.
    pub white_balance: WhiteBalance,
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

/// The white balance that removes an illuminant seen in the as-shot picture,
/// in RapidRAW 1.6.5's units.
///
/// The image auto-WB looks at has already been balanced by the camera, so what
/// it detects is the light that is *left over*. Their picker has the same
/// problem with a clicked colour and solves it in `pick_white_balance`: take
/// that colour to white, on top of the as-shot balance. An illuminant is just
/// the colour a white object takes under it, so it goes through the same door,
/// and Auto and their picker answer in the same units by construction
/// rather than by two hand-written inverses that had to be kept in step.
pub fn removing_illuminant(x: f64, y: f64, as_shot: WhiteBalance) -> Option<WhiteBalance> {
    if !x.is_finite() || !y.is_finite() || y <= 0.0 {
        return None;
    }
    let xyz = DVec3::new(x / y, 1.0, (1.0 - x - y) / y);
    let rgb = primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65)
        .as_dmat3()
        .inverse()
        * xyz;
    crate::white_balance::pick_white_balance(rgb.to_array(), as_shot)
}

/// Detect the illuminant and return the white balance that removes it.
pub fn auto_white_balance(
    image: &DynamicImage,
    mode: DetectMode,
    linearise: bool,
    as_shot: WhiteBalance,
) -> Option<AutoWhiteBalance> {
    let (x, y) = detect_illuminant(image, mode, linearise)?;
    let white_balance = removing_illuminant(x as f64, y as f64, as_shot)?;

    Some(AutoWhiteBalance {
        x,
        y,
        temperature_k: cct_from_xy(x, y),
        white_balance,
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

    fn mired(wb: WhiteBalance) -> f64 {
        1.0e6 / wb.temperature
    }

    /// The light's colour after their renderer applies this white balance:
    /// the same log-LMS gains `apply_white_balance` uses in the shader.
    fn corrected(light: [f32; 3], as_shot: WhiteBalance, target: WhiteBalance) -> DVec3 {
        let gains = crate::white_balance::adaptation_log_gains(as_shot, target);
        let to_lms = crate::white_balance::rgb_to_lms().as_dmat3();
        let lms = to_lms * DVec3::from_array(light.map(f64::from));
        let scale = DVec3::from_array(gains.map(|g| (g as f64).exp()));
        to_lms.inverse() * (lms * scale)
    }

    fn cast(rgb: DVec3) -> f64 {
        (rgb.x / rgb.y).ln().abs().max((rgb.z / rgb.y).ln().abs())
    }

    #[test]
    fn neutral_light_needs_almost_no_correction() {
        let as_shot = WhiteBalance::reference();
        let img = textured(192, 192, [1.0, 1.0, 1.0]);
        let result =
            auto_white_balance(&img, DetectMode::Surfaces, false, as_shot).expect("should detect");
        assert!(
            (mired(result.white_balance) - mired(as_shot)).abs() < 25.0,
            "a neutrally lit scene moved white balance to {:?}",
            result.white_balance
        );
    }

    #[test]
    fn warm_light_is_detected_and_cooled() {
        let as_shot = WhiteBalance::reference();
        let neutral = auto_white_balance(
            &textured(192, 192, [1.0, 1.0, 1.0]),
            DetectMode::Surfaces,
            false,
            as_shot,
        )
        .expect("should detect");
        let warm = auto_white_balance(
            &textured(192, 192, [1.25, 1.0, 0.7]),
            DetectMode::Surfaces,
            false,
            as_shot,
        )
        .expect("should detect");

        // A lower kelvin target is a cooler picture.
        assert!(
            warm.white_balance.temperature < neutral.white_balance.temperature,
            "warm light should pull the target down: neutral {:?} vs warm {:?}",
            neutral.white_balance,
            warm.white_balance
        );
    }

    /// End to end through their maths: whatever light the detector reports,
    /// the white balance the wand writes turns that light white in their
    /// renderer, on top of whatever the camera chose. How well the detector
    /// judges a synthetic frame is a separate question, asked by the tests
    /// above; this is the one about units.
    #[test]
    fn what_the_wand_writes_neutralises_what_it_detected() {
        let light = [1.25, 1.0, 0.7];
        for as_shot in [
            WhiteBalance::reference(),
            WhiteBalance {
                temperature: 4200.0,
                tint: 3.0,
            },
        ] {
            let result = auto_white_balance(
                &textured(192, 192, light),
                DetectMode::Surfaces,
                false,
                as_shot,
            )
            .expect("should detect");
            let (x, y) = (result.x as f64, result.y as f64);
            let detected = primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65)
                .as_dmat3()
                .inverse()
                * DVec3::new(x / y, 1.0, (1.0 - x - y) / y);
            let after = cast(corrected(
                detected.to_array().map(|c| c as f32),
                as_shot,
                result.white_balance,
            ));
            assert!(
                after < 0.01,
                "the detected light is left at {after:.4} at {as_shot:?}"
            );
        }
    }

    /// A clicked white and a detected illuminant are the same question.
    #[test]
    fn a_grey_card_under_the_light_comes_back_neutral() {
        let as_shot = WhiteBalance {
            temperature: 5000.0,
            tint: 0.0,
        };
        let light = [1.2_f32, 1.0, 0.75];
        let xyz = primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65).as_dmat3()
            * DVec3::from_array(light.map(f64::from));
        let sum = xyz.element_sum();
        let target = removing_illuminant(xyz.x / sum, xyz.y / sum, as_shot).expect("should solve");
        assert!(cast(corrected(light, as_shot, target)) < 0.01);
    }

    // Edges mode is disabled in the UI - see the note on DetectMode::Edges.
    // Kept and ignored so the failure is reproducible when someone picks it up.
    #[test]
    #[ignore = "edges mode returns implausible illuminants in this pipeline"]
    fn edges_mode_also_responds_to_the_light() {
        let as_shot = WhiteBalance::reference();
        let neutral = auto_white_balance(
            &textured(192, 192, [1.0, 1.0, 1.0]),
            DetectMode::Edges,
            false,
            as_shot,
        )
        .expect("should detect");
        let warm = auto_white_balance(
            &textured(192, 192, [1.25, 1.0, 0.7]),
            DetectMode::Edges,
            false,
            as_shot,
        )
        .expect("should detect");
        assert!(warm.white_balance.temperature < neutral.white_balance.temperature);
    }

    #[test]
    fn a_flat_frame_yields_nothing_to_measure() {
        // Surfaces mode weights by chroma variance, so a frame with none should
        // decline rather than invent an illuminant.
        let img = solid(192, 192, [0.7, 0.45, 0.25]);
        assert!(
            auto_white_balance(&img, DetectMode::Surfaces, false, WhiteBalance::reference())
                .is_none(),
            "a flat frame has no surfaces to measure"
        );
    }

    #[test]
    fn tiny_images_are_rejected_rather_than_panicking() {
        let img = solid(8, 8, [0.5, 0.5, 0.5]);
        assert!(
            auto_white_balance(&img, DetectMode::Surfaces, false, WhiteBalance::reference())
                .is_none()
        );
    }
}
