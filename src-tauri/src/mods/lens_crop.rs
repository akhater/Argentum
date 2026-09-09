//! Choosing the lens calibration that matches the camera.
//!
//! lensfun stores one entry per lens *per body it was measured on*. The EF 50mm
//! f/1.4 USM appears twice in the bundled database: once at `cropfactor 1` and
//! once at `1.611`. Same glass, different measurements.
//!
//! Their matcher scores on the name alone, so both entries score identically and
//! whichever comes last wins. On a full-frame 5D Mark II that produced
//! *"Canon - EF 50mm f/1.4 USM (crop 1.6x)"* — the right lens, calibrated on an
//! APS-C body. It looks like a detection bug and is not one: distortion measured
//! over the middle of the frame under-corrects the corners of a full-frame shot,
//! quietly and in the place it matters most.
//!
//! So: find the camera in the same database, read its crop factor, and prefer
//! the calibration closest to it.

use crate::lens_correction::{Lens, LensDatabase};

/// How close two crop factors must be to count as the same body class.
///
/// lensfun writes 1.611 for Canon APS-C and 1 for full frame, so anything under
/// a tenth is comfortably inside one class and outside the other.
const CROP_TOLERANCE: f32 = 0.1;

/// The crop factor of a camera, found by name in the lensfun database.
///
/// Matching is loose because the EXIF model and lensfun's name rarely agree
/// exactly — "Canon EOS 5D Mark II" against "EOS 5D Mark II". Whichever known
/// camera name appears inside the EXIF string wins, longest first so "5D Mark
/// II" is preferred over a bare "5D".
pub fn camera_crop_factor(db: &LensDatabase, camera_model: &str) -> Option<f32> {
    let needle = camera_model.trim().trim_matches('"').to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }

    // `MultiName.value` is private to their module, so go through the accessor.
    // It returns one name per camera rather than every localisation, which is
    // enough: model names are not translated.
    let mut best: Option<(usize, f32)> = None;
    for camera in &db.cameras {
        let candidate = camera.get_model().trim().to_ascii_lowercase();
        if candidate.is_empty() || !needle.contains(&candidate) {
            continue;
        }
        let len = candidate.len();
        if best.map(|(best_len, _)| len > best_len).unwrap_or(true) {
            best = Some((len, camera.cropfactor));
        }
    }

    best.map(|(_, crop)| crop)
}

/// Of several lenses sharing a name, the one calibrated on this camera.
///
/// Returns `None` when there is nothing to choose between — one candidate, or no
/// crop factor for the camera — leaving their matcher's answer alone. This only
/// ever breaks a tie; it never overrides a decision made on the name.
pub fn pick_for_camera<'a>(
    db: &LensDatabase,
    candidates: &[&'a Lens],
    camera_model: &str,
) -> Option<&'a Lens> {
    if candidates.len() < 2 {
        return None;
    }

    let target = camera_crop_factor(db, camera_model)?;

    let mut best: Option<(f32, &Lens)> = None;
    for lens in candidates {
        let Some(crop) = lens.cropfactor else {
            continue;
        };
        let distance = (crop - target).abs();
        if best.map(|(d, _)| distance < d).unwrap_or(true) {
            best = Some((distance, lens));
        }
    }

    match best {
        // Only worth overriding when something actually fits the body. If the
        // nearest calibration is from a different class of camera entirely, the
        // name match was as good as it gets.
        Some((distance, lens)) if distance <= CROP_TOLERANCE => Some(lens),
        _ => None,
    }
}

/// Find the lens, preferring the calibration measured on a body like this one.
///
/// Runs before their fuzzy matcher and defers to it whenever there is nothing to
/// choose between — one candidate, an unknown camera, or no name match at all.
/// It only ever settles a tie their scoring cannot see.
///
/// The whole matter is decided here rather than inside their function because
/// the fix needs the camera model, which their `find_lens_by_fuzzy_model` never
/// receives. Threading it through would have meant editing several of their
/// lines; this costs four.
pub fn match_for_camera(
    db: &LensDatabase,
    maker: &str,
    model: &str,
    camera_model: &str,
) -> Option<(String, String)> {
    use fuzzy_matcher::FuzzyMatcher;

    let clean_model = model.trim().trim_matches('"');
    if clean_model.is_empty() {
        return None;
    }

    // Nothing to disambiguate without knowing what the photo was taken on.
    camera_crop_factor(db, camera_model)?;

    let clean_maker = maker.trim().trim_matches('"').to_string();
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default().ignore_case();

    let from_maker: Vec<&Lens> = db
        .lenses
        .iter()
        .filter(|lens| lens.get_maker().eq_ignore_ascii_case(&clean_maker))
        .collect();

    if from_maker.is_empty() {
        return None;
    }

    // Score exactly as they do, so this cannot pick a different *lens* — only a
    // different calibration of the same one.
    let scored: Vec<(i64, &Lens)> = from_maker
        .iter()
        .filter_map(|lens| {
            let english = lens.get_full_model_name();
            let canonical = lens.get_canonical_model_name();
            let score = matcher
                .fuzzy_match(&english, clean_model)
                .unwrap_or(0)
                .max(matcher.fuzzy_match(&canonical, clean_model).unwrap_or(0));
            if score <= 0 {
                return None;
            }
            let name = if matcher.fuzzy_match(&canonical, clean_model).unwrap_or(0)
                > matcher.fuzzy_match(&english, clean_model).unwrap_or(0)
            {
                canonical
            } else {
                english
            };
            let penalty = (name.len() as i64 - clean_model.len() as i64).max(0) / 2;
            Some((score - penalty, *lens))
        })
        .collect();

    let best_score = scored.iter().map(|(s, _)| *s).max()?;

    // Duplicate calibrations of one lens score identically, so a small window
    // catches them without pulling in a genuinely different lens.
    let tied: Vec<&Lens> = scored
        .iter()
        .filter(|(s, _)| (*s - best_score).abs() <= 2)
        .map(|(_, l)| *l)
        .collect();

    let chosen = pick_for_camera(db, &tied, camera_model)?;
    Some((chosen.get_maker(), chosen.get_display_name(&from_maker)))
}
