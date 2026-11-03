//! A RAW's camera colour the way the DNG spec builds it. Ours, from RapidRAW 3f5345a5 ("DNG-style camera colour with
//! dual-illuminant matrices").
//!
//! WHAT CHANGES
//!
//! rawler develops a RAW with its D65 matrix, scaled so the as-shot white comes
//! out white, which is exact only for daylight. The DNG way inverts the
//! camera's matrix, takes the as-shot white in XYZ and adapts it to D65 with
//! Bradford; a camera with both an A and a D65 matrix (most in rawler's data)
//! gets the two mixed for the shot's light, so tungsten and stage light lose
//! the magenta the D65 matrix gave them. The maths is RapidRAW's,
//! `white_balance::camera_to_srgb`.
//!
//! WHERE: AFTER THE DECODE, LIKE A PROFILE
//!
//! RapidRAW swaps the matrix in its decode. Here the decode keeps rawler's, and
//! the difference is one 3x3 on the decoded pixels in the slot a camera
//! profile uses (profile_correction.rs, whose notes say why: the decode is
//! shared by every edit of a file and kept in memory, and switching must not
//! read the file again). So
//!
//! ```text
//! correction = dng · diag(1 / wb) · inv(built_in)
//! ```
//!
//! where `built_in` is what rawler applied after the white balance `wb`, and
//! `dng` is RapidRAW's camera to sRGB for the as-shot white. Both map the
//! as-shot white to the same grey, so a neutral keeps its brightness exactly
//! and only colours move.
//!
//! WHICH PHOTOS
//!
//! Every edit, whenever it was made. From 26.44.1 to 26.45.x only new edits
//! took it, marked `agDngColour: true` (`RETIRED_KEY`), and older ones kept
//! rawler's colour; that mark is ignored now and goes on the next save.
//!
//! Only a RAW developed through a 3x3 matrix: a mosaic, or a linear RAW (a
//! Canon sRAW, a linear DNG) whose calibration the decode applied. A
//! monochrome or four-colour sensor keeps rawler's colour, and a chosen
//! camera profile replaces the matrix as before.

use super::profile_matrix::{invert, multiply};
use crate::white_balance::{WhiteBalance, camera_to_srgb};

/// The mark 26.44.1 to 26.45.x stored in a new edit. No render reads it: a
/// stored one counts as untouched (`image_processing::is_default_adjustments`)
/// and the editor drops it on load (adjustments.ts `normalizeLoadedAdjustments`).
pub const RETIRED_KEY: &str = "agDngColour";

/// What the correction needs from the decode, beside the matrix it used.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// The camera's illuminant A matrix, when it also has the D65 one that
    /// rawler developed with.
    pub xyz_to_camera_a: Option<[f32; 9]>,
    /// The white balance the decode applied, red, green and blue.
    pub wb: [f32; 3],
}

/// The inputs for a RAW the DNG way applies to: one with 3x3 matrices that the
/// decode developed through them (`calibrated`): a mosaic, or a linear RAW
/// such as a Canon sRAW or a linear DNG unless Linear RAW Processing skips
/// the calibration.
pub fn inputs_of(raw: &rawler::rawimage::RawImage, calibrated: bool) -> Option<Inputs> {
    use rawler::imgop::xyz::Illuminant;
    use rawler::rawimage::RawPhotometricInterpretation;

    let mosaic = raw.cpp == 1 && matches!(raw.photometric, RawPhotometricInterpretation::Cfa(_));
    let linear = raw.cpp == 3 && matches!(raw.photometric, RawPhotometricInterpretation::LinearRaw);
    if !calibrated || !(mosaic || linear) {
        return None;
    }
    let three_by_three = |m: &Vec<f32>| -> Option<[f32; 9]> { m.as_slice().try_into().ok() };
    // Kept to rawler's rule, as profile_correction's built-in matrix is: D65 when
    // there is one, otherwise the first.
    let developed_with = raw
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw.color_matrix.values().next())?;
    three_by_three(developed_with)?;
    let has_d65 = raw.color_matrix.contains_key(&Illuminant::D65);
    let xyz_to_camera_a = match raw.color_matrix.get(&Illuminant::A) {
        Some(a) if has_d65 => Some(three_by_three(a)?),
        _ => None,
    };
    Some(Inputs {
        xyz_to_camera_a,
        wb: [raw.wb_coeffs[0], raw.wb_coeffs[1], raw.wb_coeffs[2]],
    })
}

/// What to multiply decoded pixels by for the DNG way: `built_in` is the matrix
/// rawler developed with, `developed` its camera to sRGB as the decode applied
/// it (profile_correction.rs `cam_to_srgb`).
pub fn correction(
    built_in: &[f32; 9],
    developed: &[[f32; 3]; 3],
    inputs: &Inputs,
) -> Option<[[f32; 3]; 3]> {
    let wb = if inputs.wb.iter().all(|c| c.is_finite() && *c > 0.0) {
        inputs.wb
    } else {
        [1.0; 3]
    };
    let neutral = wb.map(|c| 1.0 / c);
    let xyz_to_camera = match &inputs.xyz_to_camera_a {
        Some(a) => WhiteBalance::dual_illuminant_camera_neutral(a, built_in, &neutral)?.1,
        None => built_in.to_vec(),
    };
    let primaries = camera_to_srgb(&xyz_to_camera, &neutral)?;
    let [red, green, blue] = <[[f32; 3]; 3]>::try_from(primaries).ok()?;
    let mut dng = [[0.0f32; 3]; 3];
    for (row, out) in dng.iter_mut().enumerate() {
        *out = [red[row], green[row], blue[row]];
    }
    let unbalance = [
        [1.0 / wb[0], 0.0, 0.0],
        [0.0, 1.0 / wb[1], 0.0],
        [0.0, 0.0, 1.0 / wb[2]],
    ];
    let m = multiply(&multiply(&dng, &unbalance), &invert(developed)?);
    m.iter().flatten().all(|v| v.is_finite()).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::profile_matrix::as_rows;
    use argentum_engine::colour::SRGB_TO_XYZ_D65;

    /// Canon EOS 5D Mark II in rawler's data, D65 and A.
    const D65_5D2: [f32; 9] = [
        0.4716, 0.0603, -0.0830, -0.7798, 1.5474, 0.2480, -0.1496, 0.1937, 0.6651,
    ];
    const A_5D2: [f32; 9] = [
        0.5309, -0.0229, -0.0336, -0.6241, 1.3265, 0.3337, -0.0817, 0.1215, 0.6664,
    ];

    /// rawler's camera to sRGB, as profile_correction.rs works it out.
    fn developed(xyz2cam: &[f32; 9]) -> [[f32; 3]; 3] {
        let mut rgb2cam = multiply(&as_rows(xyz2cam), &SRGB_TO_XYZ_D65);
        for row in rgb2cam.iter_mut() {
            let sum: f32 = row.iter().sum();
            for cell in row.iter_mut() {
                *cell /= sum;
            }
        }
        invert(&rgb2cam).unwrap()
    }

    fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    }

    /// Both ways turn the as-shot white into the same grey, so a neutral keeps
    /// its brightness exactly, in daylight and under tungsten.
    #[test]
    fn a_neutral_stays_where_it_was() {
        let built = developed(&D65_5D2);
        for wb in [[2.1f32, 1.0, 1.5], [1.3, 1.0, 2.6], [2.6, 1.0, 1.2]] {
            let inputs = Inputs {
                xyz_to_camera_a: Some(A_5D2),
                wb,
            };
            let c = correction(&D65_5D2, &built, &inputs).unwrap();
            for grey in [0.18f32, 1.0, 4.0] {
                let out = apply(&c, [grey; 3]);
                for v in out {
                    assert!(
                        (v - grey).abs() < 2e-4 * grey.max(1.0),
                        "{wb:?} {grey}: {out:?}"
                    );
                }
            }
        }
    }

    /// Under tungsten the mixed matrix moves colours; the D65 one alone, under
    /// daylight-like white, moves them much less.
    #[test]
    fn tungsten_moves_colour_more_than_daylight() {
        let built = developed(&D65_5D2);
        let off_identity = |wb: [f32; 3]| {
            let c = correction(
                &D65_5D2,
                &built,
                &Inputs {
                    xyz_to_camera_a: Some(A_5D2),
                    wb,
                },
            )
            .unwrap();
            (0..3)
                .flat_map(|r| (0..3).map(move |k| (r, k)))
                .map(|(r, k)| (c[r][k] - if r == k { 1.0 } else { 0.0 }).abs())
                .fold(0.0f32, f32::max)
        };
        let daylight = off_identity([2.1, 1.0, 1.5]);
        let tungsten = off_identity([1.3, 1.0, 2.6]);
        assert!(
            tungsten > daylight,
            "tungsten {tungsten} daylight {daylight}"
        );
        assert!(tungsten > 0.01, "tungsten {tungsten}");
    }

    #[test]
    fn a_white_balance_that_cannot_be_read_is_taken_as_none() {
        let built = developed(&D65_5D2);
        let nan = Inputs {
            xyz_to_camera_a: None,
            wb: [f32::NAN, 1.0, 1.0],
        };
        assert!(correction(&D65_5D2, &built, &nan).is_some());
    }

    /// A photo reset in the editor (its starting values) or by the backend
    /// (`default_adjustments`), with no rating, flag, tags or snapshots, has
    /// no sidecar, whether or not its stored edit still carries the mark.
    #[test]
    fn a_reset_photo_has_no_sidecar_with_or_without_the_retired_mark() {
        use crate::image_processing::{ImageMetadata, default_adjustments, is_default_adjustments};
        use serde_json::{Value, json};

        assert!(is_default_adjustments(&default_adjustments()));
        let editor = Value::Object(crate::mods::compat_corpus::starting_values());
        assert!(is_default_adjustments(&editor));

        let with = |edit: &Value, mark: Value| {
            let mut edit = edit.clone();
            edit[RETIRED_KEY] = mark;
            edit
        };
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("IMG_0001.CR2");
        let sidecar = crate::file_management::sidecar_for(&source, None);
        for edit in [
            default_adjustments(),
            editor.clone(),
            with(&default_adjustments(), json!(true)),
            with(&editor, json!(true)),
            with(&editor, json!(false)),
        ] {
            std::fs::write(&sidecar, "{}").unwrap();
            let metadata = ImageMetadata {
                adjustments: edit.clone(),
                ..Default::default()
            };
            crate::exif_processing::write_sidecar(&sidecar, &source, &metadata).unwrap();
            assert!(!sidecar.exists(), "kept for {edit}");
        }

        // An edit is still an edit, mark or not.
        let edited = with(
            &json!({ "baselineExposure": true, "exposure": 0.5 }),
            json!(true),
        );
        assert!(!is_default_adjustments(&edited));
    }
}
