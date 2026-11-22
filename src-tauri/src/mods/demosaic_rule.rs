//! Which demosaic a photo's decode uses, and the mark that says so. Ours.
//!
//! Edits saved before RapidRAW 1.7.0's demosaic was taken keep rawler's: PPG
//! on Bayer sensors, rawler's own on X-Trans. Edits made since, and photos
//! with no edit, take RapidRAW's (`argentum_engine::rr_demosaic`): RCD with
//! residual interpolation on Bayer, Markesteijn's 3-pass on X-Trans. An edit
//! keeps the look it was made with.
//!
//! The mark is the photo's, at the top of its own sidecar, as the highlight
//! rule's is and for the same reasons (`highlight_rule.rs`): the decode is
//! the file's, shared by its virtual copies, the thumbnail and the export;
//! copy settings, presets and Reset never carry it; every write of the
//! photo's own sidecar stamps it from what the sidecar on disk said before.

use std::path::Path;

use rawler::RawImageData;
use rawler::rawimage::RawImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::sidecar_mark::{self, Mark};

/// The demosaic a photo's decode uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Demosaic {
    /// rawler's: PPG on Bayer, its own on X-Trans. Never written: a sidecar
    /// without the mark means this.
    #[serde(rename = "rawler")]
    Rawler,
    /// RapidRAW 1.7.0's (`argentum_engine::rr_demosaic`).
    #[serde(rename = "rapidraw")]
    RapidRaw,
}

/// The key the mark is stored under, at the top of a sidecar.
pub const KEY: &str = "agDemosaic";

/// The word the mark is written as.
const WORD: &str = "rapidraw";

/// The demosaic of the photo at `photo` (a file, or a virtual copy's path):
/// its own sidecar's. RapidRAW's when it has none or nothing says which photo.
pub fn rule_for(photo: Option<&str>) -> Demosaic {
    match photo {
        Some(photo) => rule_of_sidecar(&sidecar_mark::own_sidecar(photo)),
        None => Demosaic::RapidRaw,
    }
}

/// The demosaic a photo's own sidecar at `sidecar` says.
pub fn rule_of_sidecar(sidecar: &Path) -> Demosaic {
    match sidecar_mark::read(sidecar, KEY) {
        Mark::NoSidecar => Demosaic::RapidRaw,
        Mark::Word(word) if word == WORD => Demosaic::RapidRaw,
        _ => Demosaic::Rawler,
    }
}

/// The mark as `ImageMetadata` reads it: RapidRAW's, or none for anything
/// else, so a value this version does not know cannot stop the sidecar
/// reading.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Demosaic>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok((value.as_ref().and_then(|v| v.as_str()) == Some(WORD)).then_some(Demosaic::RapidRaw))
}

/// The mark a write of a photo's own sidecar at `sidecar` carries:
/// RapidRAW's when the sidecar there now says it (or there is none), and
/// none otherwise, whatever the edit being written came with.
pub fn stamped(sidecar: &Path) -> Option<Demosaic> {
    (rule_of_sidecar(sidecar) == Demosaic::RapidRaw).then_some(Demosaic::RapidRaw)
}

/// Demosaic `raw` with RapidRAW's, in place: its samples become camera RGB
/// over its active area, three floats a pixel, in the scale and space
/// rawler's own demosaic hands to calibration (black at 0, `white` at
/// 1 / (`u32::MAX` - black), not white balanced). The caller develops it
/// without rawler's rescale, which this has done. False, with `raw` as it
/// was, when the demosaic cannot take the mosaic (`Mosaic::new`): rawler's
/// then demosaics it.
///
/// `white` is the photo's own white level: the app has put `u32::MAX` in
/// `raw` by now (`raw_processing::develop_decoded`).
pub fn demosaic_in_place(raw: &mut RawImage, white: f32) -> bool {
    // White balanced before demosaic, as RapidRAW does (residual
    // interpolation fits red and blue to green); green at 1, the scale its
    // constants are for. Taken off again after, for rawler's calibration.
    let coeffs = if raw.wb_coeffs[0].is_nan() {
        [1.0; 4]
    } else {
        raw.wb_coeffs
    };
    let wb = [coeffs[0] / coeffs[1], 1.0, coeffs[2] / coeffs[1]];
    let Some(mosaic) = argentum_engine::rr_demosaic::Mosaic::new(raw, wb, white) else {
        return false;
    };
    let domain = mosaic.domain();
    let started = std::time::Instant::now();
    let mut data = mosaic.develop().into_flattened();
    log::info!(
        "RapidRAW {:?} demosaic of {}x{} took {:?}",
        mosaic.pattern(),
        domain.d.w,
        domain.d.h,
        started.elapsed()
    );
    // rawler's scale: (sample - black) / (u32::MAX - black).
    let black = raw.blacklevel.as_bayer_array()[0];
    let to_rawler = (white - black).max(1.0) / (u32::MAX as f32 - black);
    let gains = wb.map(|g| to_rawler / g);
    data.par_chunks_exact_mut(3).for_each(|px| {
        for (v, gain) in px.iter_mut().zip(gains) {
            *v *= gain;
        }
    });
    raw.data = RawImageData::Float(data);
    raw.cpp = 3;
    raw.width = domain.d.w;
    raw.height = domain.d.h;
    // `active_area` stays: rawler crops the default crop against it, and the
    // picture is now exactly that area, as its own demosaic leaves it.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-demosaic-rule-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_photo_with_no_sidecar_takes_rapidraws() {
        let dir = scratch("none");
        let photo = dir.join("IMG_1.CR3");
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Demosaic::RapidRaw);
        assert_eq!(rule_for(None), Demosaic::RapidRaw);
    }

    #[test]
    fn a_sidecar_saved_before_keeps_rawlers() {
        let dir = scratch("old");
        let photo = dir.join("IMG_2.CR3");
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        // Saved since the highlight pull, before this.
        std::fs::write(
            &sidecar,
            r#"{"version":1,"rating":3,"adjustments":{"exposure":1},"agHighlights":"pull"}"#,
        )
        .unwrap();
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Demosaic::Rawler);
        assert_eq!(stamped(&sidecar), None);
        std::fs::write(&sidecar, "{ not json").unwrap();
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Demosaic::Rawler);
    }

    #[test]
    fn a_marked_sidecar_takes_rapidraws_and_its_virtual_copies_follow_the_photo() {
        let dir = scratch("marked");
        let photo = dir.join("IMG_3.CR3");
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        std::fs::write(&sidecar, r#"{"version":1,"agDemosaic":"rapidraw"}"#).unwrap();
        let path = photo.to_string_lossy().to_string();
        assert_eq!(rule_for(Some(&path)), Demosaic::RapidRaw);
        assert_eq!(stamped(&sidecar), Some(Demosaic::RapidRaw));
        let copy = format!("{path}{}abc123", crate::file_management::VIRTUAL_COPY);
        assert_eq!(rule_for(Some(&copy)), Demosaic::RapidRaw);
        std::fs::write(&sidecar, r#"{"version":1,"agDemosaic":"later"}"#).unwrap();
        assert_eq!(rule_for(Some(&path)), Demosaic::Rawler);
    }

    /// Through `write_sidecar`: a new photo's sidecar is stamped, an older
    /// one is not, whatever the edit written carries, and a virtual copy's
    /// never is.
    #[test]
    fn a_save_stamps_from_the_sidecar_it_replaces() {
        use crate::image_processing::ImageMetadata;
        let dir = scratch("save");
        let photo = dir.join("IMG_4.CR3");
        let own = crate::file_management::sidecar_for(&photo, None);
        let edit = ImageMetadata {
            rating: 2,
            adjustments: serde_json::json!({ "exposure": 0.5 }),
            ..Default::default()
        };
        let mark = |sidecar: &Path| {
            let text = std::fs::read_to_string(sidecar).unwrap();
            serde_json::from_str::<serde_json::Value>(&text).unwrap()[KEY].clone()
        };

        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(mark(&own), WORD, "a new photo");
        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(mark(&own), WORD, "and it stays");

        std::fs::write(&own, r#"{"version":1,"rating":2,"adjustments":{}}"#).unwrap();
        let carried = ImageMetadata {
            ag_demosaic: Some(Demosaic::RapidRaw),
            ..edit.clone()
        };
        crate::exif_processing::write_sidecar(&own, &photo, &carried).unwrap();
        assert!(mark(&own).is_null(), "an older edit keeps its demosaic");
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Demosaic::Rawler);

        let copy = crate::file_management::sidecar_for(&photo, Some("abc123"));
        crate::exif_processing::write_sidecar(&copy, &photo, &carried).unwrap();
        assert!(mark(&copy).is_null(), "a virtual copy's sidecar");
    }

    #[test]
    fn the_mark_is_stored_as_its_word() {
        assert_eq!(serde_json::to_value(Demosaic::RapidRaw).unwrap(), WORD);
    }
}
