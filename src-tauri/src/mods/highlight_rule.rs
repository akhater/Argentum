//! Which decode rule a photo renders with, and the mark that says so.
//!
//! Edits saved before the pull rebuild clipped highlights on the mosaic
//! (`highlights::recover` and `settle_blown`); edits made since take the pull
//! after demosaic (`highlight_pull.rs`). Edits made since the shadow rule also
//! keep the colour of pixels below black at very high ISO (rawler fork, from
//! RAWmakase 23d6c1c4), where those before clip each channel at zero. Edits
//! made since the demosaic rule are demosaiced by RapidRAW 1.7.0's
//! (`rr_demosaic.rs`), where those before take rawler's. An edit keeps the
//! look it was made with.
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
//! the latest rule.
//!
//! Every write of a photo's own sidecar stamps it from what the sidecar on
//! disk said before (`stamped`), so a photo never changes rule while its
//! sidecar exists. One that is written back to nothing at all (no edit, no
//! rating, no tags) loses its sidecar, as before, and starts again as new.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The rule an edit renders its clipped highlights and its deep shadows with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rule {
    /// The rule before the pull: rebuilt on the mosaic from the photo's highlight colour.
    /// Never written: a sidecar without the mark means this.
    #[serde(rename = "rebuild")]
    Rebuild,
    /// RAWmakase's pull towards the other channels, after demosaic.
    #[serde(rename = "pull")]
    Pull,
    /// The pull, and pixels below black keep their own colour instead of each
    /// channel being clipped at zero (RAWmakase 23d6c1c4, in the rawler fork).
    #[serde(rename = "pull-shadows")]
    PullShadows,
    /// The pull and the shadows, demosaiced by RapidRAW 1.7.0's RCD and
    /// residual interpolation, or Markesteijn on X-Trans (`rr_demosaic.rs`).
    #[serde(rename = "pull-shadows-demosaic")]
    PullShadowsDemosaic,
}

impl Rule {
    /// The rule of a photo with no edit yet.
    pub const LATEST: Rule = Rule::PullShadowsDemosaic;

    /// Clipped highlights are pulled after demosaic.
    pub fn pulls(self) -> bool {
        matches!(
            self,
            Rule::Pull | Rule::PullShadows | Rule::PullShadowsDemosaic
        )
    }

    /// Pixels below black keep their colour.
    pub fn keeps_shadow_colour(self) -> bool {
        matches!(self, Rule::PullShadows | Rule::PullShadowsDemosaic)
    }

    /// The full-size picture is demosaiced by RapidRAW 1.7.0's.
    pub fn demosaics_rapidraw(self) -> bool {
        self == Rule::PullShadowsDemosaic
    }

    /// The rule a stored mark names; none for a word this version does not know.
    fn of_mark(mark: &str) -> Option<Rule> {
        match mark {
            "pull" => Some(Rule::Pull),
            "pull-shadows" => Some(Rule::PullShadows),
            "pull-shadows-demosaic" => Some(Rule::PullShadowsDemosaic),
            _ => None,
        }
    }
}

/// The key the mark is stored under, at the top of a sidecar.
pub const KEY: &str = "agHighlights";

/// The rule of the photo at `photo` (a file, or a virtual copy's path): its
/// own sidecar's. The latest when it has none or nothing says which photo.
pub fn rule_for(photo: Option<&str>) -> Rule {
    let Some(photo) = photo else {
        return Rule::LATEST;
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Rule::LATEST,
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
        }) => Rule::of_mark(&mark).unwrap_or(Rule::Rebuild),
        _ => Rule::Rebuild,
    }
}

/// The mark as `ImageMetadata` reads it: a rule this version knows, or none
/// for anything else, so a value it does not know cannot stop the sidecar
/// reading.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Rule>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value
        .as_ref()
        .and_then(|v| v.as_str())
        .and_then(Rule::of_mark))
}

/// The mark a write of a photo's own sidecar at `sidecar` carries: the rule
/// the sidecar there now says (the latest when there is none), and none for
/// the rebuild, whatever the edit being written came with.
pub fn stamped(sidecar: &Path) -> Option<Rule> {
    Some(rule_of_sidecar(sidecar)).filter(|rule| *rule != Rule::Rebuild)
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
    fn a_photo_with_no_sidecar_takes_the_latest_rule() {
        let dir = scratch("none");
        let photo = dir.join("IMG_1.CR3");
        assert_eq!(
            rule_for(Some(&photo.to_string_lossy())),
            Rule::PullShadowsDemosaic
        );
        assert_eq!(rule_for(None), Rule::PullShadowsDemosaic);
        assert!(
            Rule::LATEST.pulls()
                && Rule::LATEST.keeps_shadow_colour()
                && Rule::LATEST.demosaics_rapidraw()
        );
        assert!(
            Rule::PullShadows.pulls()
                && Rule::PullShadows.keeps_shadow_colour()
                && !Rule::PullShadows.demosaics_rapidraw()
        );
        assert!(Rule::Pull.pulls() && !Rule::Pull.keeps_shadow_colour());
        assert!(!Rule::Rebuild.pulls() && !Rule::Rebuild.keeps_shadow_colour());
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
        // An edit made since the shadow rule keeps it.
        std::fs::write(
            &sidecar,
            r#"{"version":1,"rating":0,"adjustments":{},"agHighlights":"pull-shadows"}"#,
        )
        .unwrap();
        assert_eq!(rule_for(Some(&path)), Rule::PullShadows);
        assert_eq!(stamped(&sidecar), Some(Rule::PullShadows));
        assert_eq!(rule_for(Some(&copy)), Rule::PullShadows);
        // And one made since the demosaic rule keeps that.
        std::fs::write(
            &sidecar,
            r#"{"version":1,"agHighlights":"pull-shadows-demosaic"}"#,
        )
        .unwrap();
        assert_eq!(rule_for(Some(&path)), Rule::PullShadowsDemosaic);
        assert_eq!(stamped(&sidecar), Some(Rule::PullShadowsDemosaic));
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
        assert_eq!(mark(&own), "pull-shadows-demosaic", "a new photo");
        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(mark(&own), "pull-shadows-demosaic", "and it stays");

        std::fs::write(&own, r#"{"version":1,"agHighlights":"pull-shadows"}"#).unwrap();
        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(
            mark(&own),
            "pull-shadows",
            "an edit made before the demosaic rule keeps rawler's"
        );

        std::fs::write(&own, r#"{"version":1,"agHighlights":"pull"}"#).unwrap();
        crate::exif_processing::write_sidecar(&own, &photo, &edit).unwrap();
        assert_eq!(mark(&own), "pull", "an edit made with the pull keeps it");

        std::fs::write(&own, r#"{"version":1,"rating":2,"adjustments":{}}"#).unwrap();
        let carried = ImageMetadata {
            ag_highlights: Some(Rule::PullShadowsDemosaic),
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
    fn the_marks_are_stored_as_the_words_read_back() {
        for rule in [Rule::Pull, Rule::PullShadows, Rule::PullShadowsDemosaic] {
            let word = serde_json::to_value(rule).unwrap();
            assert_eq!(Rule::of_mark(word.as_str().unwrap()), Some(rule));
        }
        assert_eq!(serde_json::to_value(Rule::Pull).unwrap(), "pull");
        assert_eq!(
            serde_json::to_value(Rule::PullShadows).unwrap(),
            "pull-shadows"
        );
        assert_eq!(
            serde_json::to_value(Rule::PullShadowsDemosaic).unwrap(),
            "pull-shadows-demosaic"
        );
    }
}
