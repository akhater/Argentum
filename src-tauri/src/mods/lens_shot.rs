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

/// What choosing a photo's lens reads of it besides its names.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MatchShot {
    /// The focal length it records (EXIF FocalLength), not its 35 mm equivalent.
    pub focal_mm: Option<f32>,
    /// The crop factor its 35 mm equivalent focal length gives (RAWmakase's
    /// `exif_crop`, `lens/lensfun/matching.rs` @ 1b2687c4).
    pub exif_crop: Option<f32>,
    /// A crop mode's crop factor over the sensor's, as the camera's maker note
    /// records it (`canon_crop_mode`).
    pub crop_mode: Option<f32>,
}

impl MatchShot {
    pub fn from_exif(exif: &HashMap<String, String>) -> Self {
        let focal_mm = recorded(exif, "FocalLength");
        // The EXIF reader copies FocalLength into FocalLengthIn35mmFilm when it
        // reads the first, for the views that show the second. A 35 mm
        // equivalent equal to the focal length says crop factor 1, which is
        // what a full-frame body is anyway and impossible on a smaller one, so it
        // tells nothing either way.
        let exif_crop = focal_mm
            .zip(recorded(exif, "FocalLengthIn35mmFilm"))
            .filter(|(focal, equivalent)| focal != equivalent)
            .map(|(focal, equivalent)| equivalent / focal)
            .filter(|crop| (0.1..=20.).contains(crop));
        let crop_mode =
            recorded(exif, super::canon_crop_mode::KEY).filter(|k| (1.0..=20.).contains(k));
        Self {
            focal_mm,
            exif_crop,
            crop_mode,
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

    /// The crop factor the 35 mm equivalent gives; the EXIF reader's copy of the
    /// focal length into it says nothing.
    #[test]
    fn a_35mm_equivalent_gives_a_crop_factor_unless_it_is_the_focal_length() {
        let crop = |pairs| MatchShot::from_exif(&exif(pairs)).exif_crop;
        assert_eq!(
            crop(&[("FocalLength", "24"), ("FocalLengthIn35mmFilm", "36 mm")]),
            Some(1.5)
        );
        assert_eq!(
            crop(&[("FocalLength", "24"), ("FocalLengthIn35mmFilm", "24")]),
            None
        );
        assert_eq!(crop(&[("FocalLengthIn35mmFilm", "36")]), None);
        assert_eq!(crop(&[("FocalLength", "24")]), None);
        assert_eq!(
            MatchShot::from_exif(&exif(&[("FocalLength", "85 mm")])).focal_mm,
            Some(85.)
        );
    }

    /// Read as the app reads a file: a camera's own 35 mm equivalent survives the
    /// reader's copy of the focal length, which comes first in the file. A JPEG
    /// stands in for the TIFF-based RAWs the same reader opens (NEF, ARW, DNG).
    #[test]
    fn the_exif_reader_keeps_the_35mm_equivalent_a_camera_writes() {
        use little_exif::{exif_tag::ExifTag, filetype::FileExtension, rational::uR64};
        let mut jpeg = Vec::new();
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let mut tags = little_exif::metadata::Metadata::new();
        tags.set_tag(ExifTag::Make("SONY".into()));
        tags.set_tag(ExifTag::FocalLength(vec![uR64 {
            nominator: 24,
            denominator: 1,
        }]));
        tags.set_tag(ExifTag::FocalLengthIn35mmFormat(vec![36]));
        tags.write_to_vec(&mut jpeg, FileExtension::JPEG).unwrap();
        let map = crate::exif_processing::extract_metadata(&jpeg).expect("its EXIF");
        assert_eq!(MatchShot::from_exif(&map).exif_crop, Some(1.5), "{map:?}");
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
