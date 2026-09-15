//! Everything Argentum does to a RAW as it is decoded. One anchor.
//!
//! `raw_processing.rs` calls `on_raw_decoded` once, immediately after rawler
//! hands back the decoded image and before anything is done with it. That call
//! is the entire upstream cost of every decode-level fix we will ever make.
//!
//! The alternative — a call per fix — is one line of their file each time, in
//! the function every RAW passes through. This way the file that changes is
//! ours.
//!
//! ORDER IS THE CONTRACT
//!
//! Steps run top to bottom as written here. Anything that corrects *levels* has
//! to come before anything that reads pixel values, because the levels decide
//! what those values mean.

use rawler::rawimage::RawImage;

/// Called once per decoded RAW, before its pixels are used for anything.
///
/// `file_bytes` is the original file, because most of what has to be corrected
/// at this stage is written in the maker's own metadata rather than in anything
/// rawler exposes.
pub fn on_raw_decoded(raw: &mut RawImage, file_bytes: &[u8], photo_path: Option<&str>) {
    // Canon sRAW / mRAW: black already subtracted, white level in the file.
    // Must be first — it decides what every pixel value means.
    super::sraw_levels::fix(raw, file_bytes);

    // Canon bodies older than ColorData keep their white balance somewhere
    // rawler looks for it but cannot reach. Before the highlights, which read
    // pixels, and before the matrix, which multiplies whatever they are.
    super::canon_old_wb::fix(raw, file_bytes);

    // Put back the channels the sensor could not record. After the levels,
    // because it is the levels that say which values are at the ceiling, and
    // before anything reads a pixel as colour.
    super::highlights::recover(raw);

    // Remember the matrix rawler will use, so the GPU can correct off it.
    apply_camera_profile(raw, photo_path);
}

/// Record the matrix rawler is about to use, for the profile correction.
///
/// The profile itself is no longer applied here. It used to be — substituting
/// its matrix for rawler's during the decode — and that is the natural place
/// for it, but not in this app: every other adjustment is applied per frame on
/// the GPU, so there is no mechanism for a setting that needs the file read
/// again. Four attempts to add one each broke something else: a black flash on
/// every switch, a debounced save racing the choice and reverting it, a preview
/// worker finding no image because the decode had cleared it.
///
/// darktable and RawTherapee both apply the input profile in the pipeline
/// rather than at load, so this is the ordinary way round and the previous
/// arrangement was the unusual one.
///
/// What is left here is the one thing only the decode knows: which matrix was
/// used, so the correction can be worked out later. See
/// mods/profile_correction.rs.
fn apply_camera_profile(raw: &mut RawImage, photo_path: Option<&str>) {
    // The matrix rawler will *actually* develop with, not the first one in the
    // map.
    //
    // `color_matrix` is a HashMap, so `.values().next()` is whichever entry the
    // hasher happens to yield — and for a camera carrying two illuminants that
    // is not reliably the one used. `develop_intermediate` looks for D65 and
    // falls back to the first entry only if there is none, so recording
    // anything else means the correction inverts a transform that was never
    // applied. Silent, and wrong in colour rather than in behaviour.
    //
    // Kept deliberately identical to rawler's rule rather than merely similar:
    // this value is only meaningful if it is the same matrix.
    let built_in = raw
        .color_matrix
        .iter()
        .find(|(illuminant, _)| **illuminant == rawler::imgop::xyz::Illuminant::D65)
        .or_else(|| raw.color_matrix.iter().next())
        .map(|(_, m)| m)
        .filter(|m| m.len() >= 9)
        .map(|m| {
            let mut flat = [0.0f32; 9];
            flat.copy_from_slice(&m[..9]);
            flat
        });

    match built_in {
        Some(matrix) => super::profile_correction::remember_built_in(photo_path, matrix),
        // Nothing to correct against; a profile will simply not apply.
        None => super::profile_correction::forget_built_in(),
    }
}

/// Roughly how warm an EXIF light source is, in kelvin.
///
/// Only the daylight-ish ones a profile actually uses, and only well enough to
/// rank them: the answer here decides which of two matrices is nearer D65, not
/// anything that is then computed with.
fn kelvin_of(illuminant: u16) -> Option<f32> {
    Some(match illuminant {
        17 => 2856.0,         // Standard A, tungsten
        18 => 4874.0,         // B
        19 => 6774.0,         // C
        20 => 5503.0,         // D55
        21 => 6504.0,         // D65
        22 => 7504.0,         // D75
        23 => 5003.0,         // D50
        24 => 3200.0,         // ISO studio tungsten
        1 | 9 | 10 => 6504.0, // daylight, fine weather, cloudy: treat as D65
        _ => return None,
    })
}

/// The profile's matrices for the illuminant closest to daylight.
///
/// A dual-illuminant profile has a pair for each: the colour matrix and the
/// forward matrix belong together and must not be taken from different
/// illuminants. Returns both, with the illuminant that chose them.
pub type Matrices = (u16, [f32; 9], Option<[f32; 9]>);

pub fn daylight_matrix(profile: &super::dcp::Profile) -> Option<Matrices> {
    const TARGET: f32 = 6504.0;

    let mut best: Option<(f32, Matrices)> = None;
    let mut consider = |illuminant: u16, colour: [f32; 9], forward: Option<[f32; 9]>| {
        // An unknown illuminant is still usable — better a matrix measured on
        // this body than none — but anything named beats it.
        let distance = kelvin_of(illuminant).map_or(f32::MAX / 2.0, |k| (k - TARGET).abs());
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, (illuminant, colour, forward)));
        }
    };

    consider(
        profile.illuminant1,
        profile.colour_matrix1,
        profile.forward_matrix1,
    );
    if let (Some(illuminant), Some(colour)) = (profile.illuminant2, profile.colour_matrix2) {
        consider(illuminant, colour, profile.forward_matrix2);
    }

    best.map(|(_, matrices)| matrices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::dcp::Profile;

    fn profile(i1: u16, m1: f32, i2: Option<u16>, m2: Option<f32>) -> Profile {
        Profile {
            name: None,
            camera: None,
            illuminant1: i1,
            illuminant2: i2,
            colour_matrix1: [m1; 9],
            colour_matrix2: m2.map(|v| [v; 9]),
            // Marked so a test can tell which illuminant's pair came back.
            forward_matrix1: Some([m1 + 100.0; 9]),
            forward_matrix2: m2.map(|v| [v + 100.0; 9]),
        }
    }

    /// The case that shipped wrong: a real Canon profile carries illuminant A
    /// first and D50 second, and A is tungsten. Picking the first one means
    /// rendering daylight through a tungsten characterisation.
    #[test]
    fn it_prefers_daylight_over_tungsten() {
        let p = profile(17, 1.0, Some(23), Some(2.0));
        let (illuminant, colour, forward) = daylight_matrix(&p).expect("a matrix");
        assert_eq!(illuminant, 23, "picked the tungsten matrix");
        assert_eq!(colour[0], 2.0);
        assert_eq!(
            forward.expect("a forward matrix")[0],
            102.0,
            "matrices came from different illuminants"
        );
    }

    /// Order in the file must not decide it.
    #[test]
    fn the_order_in_the_file_does_not_matter() {
        let p = profile(23, 2.0, Some(17), Some(1.0));
        assert_eq!(daylight_matrix(&p).expect("a matrix").0, 23);
    }

    #[test]
    fn d65_beats_d50_when_both_are_there() {
        let p = profile(23, 1.0, Some(21), Some(2.0));
        assert_eq!(daylight_matrix(&p).expect("a matrix").0, 21);
    }

    /// A single-matrix profile is used whatever its illuminant — a matrix
    /// measured on this body beats a generic one.
    #[test]
    fn one_matrix_is_always_used() {
        let p = profile(17, 1.0, None, None);
        assert_eq!(daylight_matrix(&p).expect("a matrix").0, 17);
    }

    /// An unnamed illuminant is a last resort, never a preference.
    #[test]
    fn a_named_illuminant_wins_over_an_unknown_one() {
        let p = profile(0, 1.0, Some(23), Some(2.0));
        assert_eq!(daylight_matrix(&p).expect("a matrix").0, 23);
    }
}
