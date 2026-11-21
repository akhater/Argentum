//! Which highlight rule a photo renders with, and the mark that says so.
//!
//! Edits saved before this rule rebuild clipped highlights on the mosaic
//! (`highlights::recover` and `settle_blown`); edits made since take the pull
//! after demosaic (`highlight_pull.rs`). An edit keeps the look it was made
//! with.
//!
//! WHY THE MARK IS THE PHOTO'S, NOT THE EDIT'S
//!
//! The rule runs in the decode, and the decode is the file's: cached in
//! memory under the file and its decode settings (`cache_utils::
//! DecodeFingerprint`), shared by the editor, the thumbnail, the export and
//! every virtual copy. So the mark sits at the top of the photo's own sidecar,
//! beside its rating, not in its adjustments: copy settings, presets, sync and
//! Reset never carry it to another photo, and the decode reads it from the
//! file it is decoding. A photo with no sidecar has no edit to keep, and takes
//! the pull.
//!
//! Every write of a photo's own sidecar stamps it from what the sidecar on
//! disk said before (`stamped`), so a photo never changes rule while its
//! sidecar exists. One that is written back to nothing at all (no edit, no
//! rating, no tags) loses its sidecar, as before, and starts again as new.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The rule an edit renders its clipped highlights with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    /// The rule before the pull: rebuilt on the mosaic from the photo's highlight colour.
    /// Never written: a sidecar without the mark means this.
    Rebuild,
    /// RAWmakase's pull towards the other channels, after demosaic.
    Pull,
}

/// The key the mark is stored under, at the top of a sidecar.
pub const KEY: &str = "agHighlights";

/// The rule of the photo at `photo` (a file, or a virtual copy's path): its
/// own sidecar's. The pull when it has none or nothing says which photo.
pub fn rule_for(photo: Option<&str>) -> Rule {
    let Some(photo) = photo else {
        return Rule::Pull;
    };
    let (source, _) = crate::file_management::split_virtual_path(photo);
    rule_of_sidecar(&crate::file_management::sidecar_for(
        Path::new(source),
        None,
    ))
}

/// The rule a photo's own sidecar at `sidecar` says. One that does not read
/// was still an edit, saved before or by something else, and keeps today's.
pub fn rule_of_sidecar(sidecar: &Path) -> Rule {
    let text = match std::fs::read_to_string(sidecar) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Rule::Pull,
        Err(_) => return Rule::Rebuild,
    };
    // Most sidecars predate the mark, and some hold a long History: the key
    // is looked for before anything is parsed.
    if !text.contains(KEY) {
        return Rule::Rebuild;
    }
    #[derive(Deserialize)]
    struct Marked {
        #[serde(rename = "agHighlights")]
        mark: Option<serde_json::Value>,
    }
    match serde_json::from_str::<Marked>(&text) {
        Ok(Marked {
            mark: Some(serde_json::Value::String(mark)),
        }) if mark == "pull" => Rule::Pull,
        _ => Rule::Rebuild,
    }
}

/// The mark as `ImageMetadata` reads it: the pull's, or none for anything
/// else, so a value this version does not know cannot stop the sidecar
/// reading.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Rule>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok((value.as_ref().and_then(|v| v.as_str()) == Some("pull")).then_some(Rule::Pull))
}

/// The mark a write of a photo's own sidecar at `sidecar` carries: the
/// pull's when the sidecar there now says the pull (or there is none), and
/// none otherwise, whatever the edit being written came with.
pub fn stamped(sidecar: &Path) -> Option<Rule> {
    (rule_of_sidecar(sidecar) == Rule::Pull).then_some(Rule::Pull)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-hl-rule-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_photo_with_no_sidecar_takes_the_pull() {
        let dir = scratch("none");
        let photo = dir.join("IMG_1.CR3");
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Rule::Pull);
        assert_eq!(rule_for(None), Rule::Pull);
    }

    #[test]
    fn a_sidecar_saved_before_keeps_the_rebuild() {
        let dir = scratch("old");
        let photo = dir.join("IMG_2.CR3");
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        std::fs::write(
            &sidecar,
            r#"{"version":1,"rating":3,"adjustments":{"exposure":1}}"#,
        )
        .unwrap();
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Rule::Rebuild);
        assert_eq!(stamped(&sidecar), None);
        // Unreadable is an edit too.
        std::fs::write(&sidecar, "{ not json").unwrap();
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Rule::Rebuild);
    }

    #[test]
    fn a_marked_sidecar_takes_the_pull_and_its_virtual_copies_follow_the_photo() {
        let dir = scratch("marked");
        let photo = dir.join("IMG_3.CR3");
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        std::fs::write(
            &sidecar,
            r#"{"version":1,"rating":0,"adjustments":{},"agHighlights":"pull"}"#,
        )
        .unwrap();
        let path = photo.to_string_lossy().to_string();
        assert_eq!(rule_for(Some(&path)), Rule::Pull);
        assert_eq!(stamped(&sidecar), Some(Rule::Pull));
        let copy = format!("{path}{}abc123", crate::file_management::VIRTUAL_COPY);
        assert_eq!(rule_for(Some(&copy)), Rule::Pull);
        // A value this version does not know keeps today's.
        std::fs::write(&sidecar, r#"{"version":1,"agHighlights":"later"}"#).unwrap();
        assert_eq!(rule_for(Some(&path)), Rule::Rebuild);
    }

    /// Through `write_sidecar`, which every save goes through: a new photo's
    /// sidecar is stamped, an older one is not, whatever the edit written
    /// carries, and a virtual copy's never is.
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
        assert_eq!(mark(&own), "pull", "a new photo");
        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(mark(&own), "pull", "and it stays");

        std::fs::write(&own, r#"{"version":1,"rating":2,"adjustments":{}}"#).unwrap();
        let carried = ImageMetadata {
            ag_highlights: Some(Rule::Pull),
            ..edit.clone()
        };
        crate::exif_processing::write_sidecar(&own, &photo, &carried).unwrap();
        assert!(mark(&own).is_null(), "an older edit keeps its rule");
        assert_eq!(rule_for(Some(&photo.to_string_lossy())), Rule::Rebuild);

        let copy = crate::file_management::sidecar_for(&photo, Some("abc123"));
        crate::exif_processing::write_sidecar(&copy, &photo, &carried).unwrap();
        assert!(mark(&copy).is_null(), "a virtual copy's sidecar");
    }

    #[test]
    fn the_mark_is_stored_as_the_word_pull() {
        assert_eq!(serde_json::to_value(Rule::Pull).unwrap(), "pull");
    }
}
