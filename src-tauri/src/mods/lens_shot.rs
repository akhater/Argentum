//! How a photo was shot, as lens correction reads it from the EXIF map. Ours.
//!
//! One reader for the screen (`get_lens_distortion_params`) and the save
//! (`resolve_lens_params_in_adjustments`), so a photo keeps its correction
//! when it is opened again.

use std::collections::HashMap;

/// The focal length of a photo that records none.
const FOCAL_MM_NOT_RECORDED: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LensShot {
    pub focal_mm: f32,
    pub f_number: Option<f32>,
    pub distance_m: Option<f32>,
}

impl LensShot {
    pub fn from_exif(exif: &HashMap<String, String>) -> Self {
        Self {
            focal_mm: recorded(exif, "FocalLength")
                .or_else(|| recorded(exif, "FocalLengthIn35mmFilm"))
                .unwrap_or(FOCAL_MM_NOT_RECORDED),
            // The list holds ApertureValue as an f-number (mods/apex_aperture.rs).
            f_number: recorded(exif, "FNumber").or_else(|| recorded(exif, "ApertureValue")),
            distance_m: recorded(exif, "SubjectDistance"),
        }
    }
}

/// The number in an EXIF value as the map holds it: `f/5.6`, `5.6`, `85 mm`,
/// `3.2 m`. None for anything else and for 0, which EXIF uses for "unknown".
fn recorded(exif: &HashMap<String, String>, key: &str) -> Option<f32> {
    let text = exif.get(key)?.trim();
    let number = text.strip_prefix("f/").unwrap_or(text);
    let value = number.split_whitespace().next()?.parse::<f32>().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::resolve_lens_params_in_adjustments;
    use crate::lens_correction::{LensDatabase, resolve_lens_params};
    use serde_json::json;

    fn exif(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn close(actual: Option<f32>, expected: f32) -> bool {
        actual.is_some_and(|a| (a - expected).abs() < 0.01)
    }

    #[test]
    fn an_f_number_reads_as_the_map_prints_it_or_bare() {
        assert_eq!(
            LensShot::from_exif(&exif(&[("FNumber", "f/5.6")])).f_number,
            Some(5.6)
        );
        assert_eq!(
            LensShot::from_exif(&exif(&[("FNumber", "5.6")])).f_number,
            Some(5.6)
        );
    }

    #[test]
    fn what_a_photo_does_not_record_stays_unrecorded() {
        assert_eq!(
            LensShot::from_exif(&exif(&[])),
            LensShot {
                focal_mm: FOCAL_MM_NOT_RECORDED,
                f_number: None,
                distance_m: None,
            }
        );
    }

    /// A manual lens records 0 or 0/0, and EXIF's distance is 0 when unknown.
    #[test]
    fn zero_and_words_are_not_recorded_values() {
        for empty in ["f/0", "0", "f/NaN", "f/inf"] {
            let shot = LensShot::from_exif(&exif(&[("FNumber", empty)]));
            assert_eq!(shot.f_number, None, "{empty}");
        }
        for empty in ["0", "unknown", "infinity"] {
            let shot = LensShot::from_exif(&exif(&[("SubjectDistance", empty)]));
            assert_eq!(shot.distance_m, None, "{empty}");
        }
    }

    #[test]
    fn the_f_number_wins_over_the_apex_value() {
        let shot = LensShot::from_exif(&exif(&[("ApertureValue", "f/8"), ("FNumber", "f/16")]));
        assert_eq!(shot.f_number, Some(16.0));
    }

    /// The list holds ApertureValue as the f-number it stands for.
    #[test]
    fn the_aperture_value_alone_reads_as_listed() {
        let of = |av| LensShot::from_exif(&exif(&[("ApertureValue", av)])).f_number;
        assert!(close(of("f/16"), 16.0), "{:?}", of("f/16"));
        assert!(close(of("f/5.6"), 5.6), "{:?}", of("f/5.6"));
        assert_eq!(of("f/0"), None);
    }

    #[test]
    fn distance_and_focal_read_with_or_without_their_unit() {
        let shot = |pairs| LensShot::from_exif(&exif(pairs));
        assert_eq!(shot(&[("SubjectDistance", "3.2 m")]).distance_m, Some(3.2));
        assert_eq!(shot(&[("SubjectDistance", "3.2")]).distance_m, Some(3.2));
        assert_eq!(shot(&[("FocalLength", "85 mm")]).focal_mm, 85.0);
        assert_eq!(shot(&[("FocalLength", "85")]).focal_mm, 85.0);
        assert_eq!(shot(&[("FocalLengthIn35mmFilm", "85")]).focal_mm, 85.0);
    }

    /// Wide open is dark in the corners; stopped down, and close, less so.
    fn lens_database() -> LensDatabase {
        quick_xml::de::from_str(
            r#"<lensdatabase><lens>
                <maker>Test</maker><model>Prime 50</model><mount>Test</mount>
                <calibration>
                    <vignetting model="pa" focal="50" aperture="1.4" distance="1000" k1="-0.9" k2="0" k3="0"/>
                    <vignetting model="pa" focal="50" aperture="5.6" distance="1" k1="-0.4" k2="0" k3="0"/>
                    <vignetting model="pa" focal="50" aperture="5.6" distance="1000" k1="-0.3" k2="0" k3="0"/>
                    <vignetting model="pa" focal="50" aperture="8" distance="1000" k1="-0.2" k2="0" k3="0"/>
                    <vignetting model="pa" focal="50" aperture="16" distance="1000" k1="-0.1" k2="0" k3="0"/>
                </calibration>
            </lens></lensdatabase>"#,
        )
        .expect("a lens database")
    }

    fn vignette_on_screen(pairs: &[(&str, &str)]) -> f64 {
        let params = resolve_lens_params(&lens_database(), "Test", "Prime 50", &exif(pairs));
        serde_json::to_value(params.expect("a profile")).unwrap()["vig_k1"]
            .as_f64()
            .unwrap()
    }

    fn vignette_saved(pairs: &[(&str, &str)]) -> f64 {
        let mut edit = json!({ "lensMaker": "Test", "lensModel": "Prime 50" });
        resolve_lens_params_in_adjustments(&mut edit, &Some(exif(pairs)), Some(&lens_database()));
        edit["lensDistortionParams"]["vig_k1"].as_f64().unwrap()
    }

    #[test]
    fn the_screen_and_the_save_pick_the_profile_of_the_real_aperture() {
        let shot = [
            ("FNumber", "f/16"),
            ("ApertureValue", "f/8"),
            ("FocalLength", "50"),
        ];
        assert!((vignette_on_screen(&shot) - -0.1).abs() < 1e-6);
        assert!((vignette_saved(&shot) - -0.1).abs() < 1e-6);
    }

    /// The default is far away; 0 would pick the closest profile.
    #[test]
    fn a_photo_with_no_distance_gets_the_default_not_zero() {
        let lens = &lens_database().lenses[0];
        let at = |distance| {
            let params = lens.get_distortion_params(50.0, Some(5.6), distance);
            serde_json::to_value(params.unwrap()).unwrap()["vig_k1"]
                .as_f64()
                .unwrap()
        };
        assert!((at(Some(0.0)) - -0.4).abs() < 1e-6);
        let shot = [
            ("FNumber", "f/5.6"),
            ("FocalLength", "50"),
            ("SubjectDistance", "0"),
        ];
        assert!((vignette_on_screen(&shot) - at(None)).abs() < 1e-6);
        assert!((vignette_saved(&shot) - at(None)).abs() < 1e-6);
    }
}
