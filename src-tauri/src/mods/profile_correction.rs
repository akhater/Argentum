//! Applying a camera profile after the decode instead of during it.
//!
//! WHY THIS EXISTS
//!
//! The profile used to be applied while the RAW was decoded, by substituting
//! its matrix for rawler's. That is the natural place for it and it does not
//! work here, for a reason that has nothing to do with colour: every other
//! adjustment in this app is applied on the GPU, per frame, so the app has no
//! notion of a setting that needs the file read again. Four attempts to bolt
//! one on each broke something else — a black flash on every switch, a
//! debounced save racing the choice and reverting it, a preview worker finding
//! no image because the decode had cleared it.
//!
//! So: decode once, normally, with rawler's own matrix. Then apply the
//! *difference* between that matrix and the profile's, where every other
//! adjustment already happens. Same arithmetic, different place, and switching
//! becomes instant because nothing has to be read again.
//!
//! THE DIFFERENCE
//!
//! The decode leaves scene-linear sRGB, having applied
//!
//! ```text
//! builtin = pinv(normalize(builtin_xyz2cam · sRGB→XYZ))
//! ```
//!
//! and what we want applied is the same expression with the profile's matrix.
//! Going from one to the other is
//!
//! ```text
//! correction = wanted · inv(builtin)
//! ```
//!
//! which is a single 3x3 on already-decoded pixels. Identity when no profile is
//! chosen, so a photo without one is untouched — not approximately untouched,
//! exactly: the matrix is the identity and the shader multiplies by it.

use std::sync::Mutex;

use super::profile_matrix::{invert, multiply};

/// sRGB primaries to XYZ D65, matching rawler's own constant.
const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

pub const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The matrix rawler used for the photo currently open.
///
/// Recorded during the decode because that is the only moment it is known, and
/// needed later, per frame, to work out what to change it *to*. One photo is
/// open at a time, which is what makes a single slot enough.
static BUILT_IN: Mutex<Option<[f32; 9]>> = Mutex::new(None);

pub fn remember_built_in(matrix: [f32; 9]) {
    if let Ok(mut slot) = BUILT_IN.lock() {
        *slot = Some(matrix);
    }
}

pub fn forget_built_in() {
    if let Ok(mut slot) = BUILT_IN.lock() {
        *slot = None;
    }
}

fn built_in() -> Option<[f32; 9]> {
    BUILT_IN.lock().ok().and_then(|slot| *slot)
}

fn as_rows(flat: &[f32; 9]) -> [[f32; 3]; 3] {
    [
        [flat[0], flat[1], flat[2]],
        [flat[3], flat[4], flat[5]],
        [flat[6], flat[7], flat[8]],
    ]
}

/// Camera RGB to sRGB, the way rawler derives it from an `xyz2cam`.
///
/// Reproduced rather than imported because it is not public, and because the
/// whole correction is only correct if it matches what the decode actually did
/// — `normalize` scaling each row to sum to one is the part that matters, since
/// it is what makes white map to white.
fn cam_to_srgb(xyz2cam: &[f32; 9]) -> Option<[[f32; 3]; 3]> {
    let mut rgb2cam = multiply(&as_rows(xyz2cam), &SRGB_TO_XYZ_D65);
    for row in rgb2cam.iter_mut() {
        let sum: f32 = row.iter().sum();
        if sum.abs() > 1e-9 {
            for cell in row.iter_mut() {
                *cell /= sum;
            }
        }
    }
    invert(&rgb2cam)
}

/// What to multiply decoded pixels by so they look as though the profile had
/// been used instead of rawler's matrix.
pub fn correction_for(profile_xyz2cam: &[f32; 9]) -> Option<[[f32; 3]; 3]> {
    let built_in = built_in()?;
    let from = cam_to_srgb(&built_in)?;
    let to = cam_to_srgb(profile_xyz2cam)?;
    Some(multiply(&to, &invert(&from)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canon EOS 5D Mark II as rawler holds it, and as the published profile
    /// gives it after the ForwardMatrix derivation.
    const BUILT_IN_5D2: [f32; 9] = [
        0.4716, 0.0603, -0.083, -0.7798, 1.5474, 0.248, -0.1496, 0.1937, 0.6651,
    ];

    fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// The same matrix on both sides must change nothing. This is the case that
    /// protects every photo without a profile.
    #[test]
    fn correcting_to_itself_is_the_identity() {
        remember_built_in(BUILT_IN_5D2);
        let c = correction_for(&BUILT_IN_5D2).expect("derivable");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((c[i][j] - want).abs() < 1e-3, "not the identity: {c:?}");
            }
        }
        forget_built_in();
    }

    /// Neutral must stay neutral. Both matrices map white to white by
    /// construction, so the correction between them has to as well — if it does
    /// not, choosing a profile would shift the white balance, which is not what
    /// a profile is for.
    #[test]
    fn neutral_survives_the_correction() {
        remember_built_in(BUILT_IN_5D2);
        // The profile's, derived from its ForwardMatrix.
        let profile = super::super::profile_matrix::xyz2cam_from_forward(&[
            0.6399, 0.1294, 0.1949, 0.2827, 0.6579, 0.0594, 0.0001, 0.0051, 0.8199,
        ])
        .expect("derivable");

        let c = correction_for(&profile).expect("derivable");
        let out = apply(&c, [1.0, 1.0, 1.0]);
        assert!(
            (out[0] - out[1]).abs() < 0.02 && (out[1] - out[2]).abs() < 0.02,
            "white became {out:?}"
        );
        forget_built_in();
    }

    /// And it has to actually do something, or this is an elaborate identity.
    #[test]
    fn a_different_profile_changes_saturated_colour() {
        remember_built_in(BUILT_IN_5D2);
        // Red and blue swapped, the test profile.
        let swapped = super::super::profile_matrix::xyz2cam_from_forward(&[
            0.1949, 0.1294, 0.6399, 0.0594, 0.6579, 0.2827, 0.8199, 0.0051, 0.0001,
        ])
        .expect("derivable");

        let c = correction_for(&swapped).expect("derivable");
        let red = apply(&c, [1.0, 0.2, 0.2]);
        assert!(
            (red[0] - 1.0).abs() > 0.1 || (red[2] - 0.2).abs() > 0.1,
            "a swapped profile left red alone: {red:?}"
        );
        forget_built_in();
    }

    /// With nothing decoded there is nothing to correct against, and that must
    /// be a `None` rather than a wrong answer.
    #[test]
    fn without_a_decode_there_is_no_correction() {
        forget_built_in();
        assert!(correction_for(&BUILT_IN_5D2).is_none());
    }
}

/// The three shader rows for whatever profile this photo asked for.
///
/// Identity whenever there is no profile, no library, or anything at all goes
/// wrong. A photo without a profile must render exactly as it always has, and
/// "exactly" here means multiplied by the identity, not skipped.
pub fn rows_for(js_adjustments: &serde_json::Value) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let m = chosen_correction(js_adjustments).unwrap_or(IDENTITY);
    (
        [m[0][0], m[0][1], m[0][2], 0.0],
        [m[1][0], m[1][1], m[1][2], 0.0],
        [m[2][0], m[2][1], m[2][2], 0.0],
    )
}

fn chosen_correction(js_adjustments: &serde_json::Value) -> Option<[[f32; 3]; 3]> {
    let file = js_adjustments.get("cameraProfile")?.as_str()?.trim();
    if file.is_empty() {
        return None;
    }

    let library = super::profiles::library()?;
    let profile = super::profiles::load(library, file)?;
    let (_, _, forward) = super::decode::daylight_matrix(&profile)?;
    let xyz2cam = super::profile_matrix::xyz2cam_from_forward(&forward?)?;
    correction_for(&xyz2cam)
}
