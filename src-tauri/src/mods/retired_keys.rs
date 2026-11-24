//! Stored keys retired on purpose, and the removal of them on load. Ours.
//!
//! `AppSettings` and `ImageMetadata` keep every top-level key they do not know,
//! so that a file a NEWER release wrote survives a save (`extra`). A key this
//! release retired is also unknown to them, so each load calls [`strip`] to
//! take the retired ones out: a deleted reader resets its stored value, and a
//! name reused later does not come back with its old value. The list is
//! defined once, here; the compatibility corpus (mods/compat_corpus.rs) reads
//! the same one to know which keys it may not find again.

use serde_json::{Map, Value};

/// The two files of a release folder.
pub const SETTINGS: &str = "settings.json";
pub const EDIT: &str = "photo.agdata";

/// A stored key today's code no longer keeps, on purpose: the corpus does not
/// count it lost, and a top-level one is removed when the file is loaded, so a
/// retired key never lives on among the keys kept for a newer release. Its path is a JSON
/// Pointer into the file, `*` standing for any one key or index
/// (`/masks/*/adjustments/oldKey`).
// `reason` and `pr` are for whoever reads the list and for the corpus tests.
#[cfg_attr(not(test), allow(dead_code))]
pub struct Dropped {
    pub file: &'static str,
    pub path: &'static str,
    pub reason: &'static str,
    pub pr: &'static str,
}

pub const DROPPED: &[Dropped] = &[
    Dropped {
        file: SETTINGS,
        path: "/enableBaselineExposure",
        reason: "no control ever wrote it; Argentum adds no baseline stops to Exposure (its reader returned 0)",
        pr: "#258",
    },
    Dropped {
        file: SETTINGS,
        path: "/baselineExposure",
        reason: "no control ever wrote it; Argentum adds no baseline stops to Exposure (its reader returned 0)",
        pr: "#258",
    },
    Dropped {
        file: SETTINGS,
        path: "/compensateHighlightPreservation",
        reason: "no control ever wrote it; DNG Standard handles held-back highlights in its own curve",
        pr: "#258",
    },
    Dropped {
        file: SETTINGS,
        path: "/requireMatchingExif",
        reason: "no control ever wrote it; grouping no longer reads it (always off)",
        pr: "#258",
    },
    Dropped {
        file: SETTINGS,
        path: "/groupEditedFiles",
        reason: "no control ever wrote it; grouping no longer reads it (edited files always group)",
        pr: "#258",
    },
    Dropped {
        file: SETTINGS,
        path: "/workspace/leftTopHeight",
        reason: "the Classic layout stored it; the Modern window keeps only the two column widths",
        pr: "#276",
    },
    Dropped {
        file: SETTINGS,
        path: "/workspace/rightTopHeight",
        reason: "the Classic layout stored it; the Modern window keeps only the two column widths",
        pr: "#276",
    },
    Dropped {
        file: SETTINGS,
        path: "/workspace/panelLayout",
        reason: "the Classic layout stored it; the Modern window keeps only the two column widths",
        pr: "#276",
    },
    Dropped {
        file: SETTINGS,
        path: "/workspace/activePanels",
        reason: "the Classic layout stored it; the Modern window keeps only the two column widths",
        pr: "#276",
    },
    Dropped {
        file: SETTINGS,
        path: "/workspace/panelSwitcherPlacement",
        reason: "the Classic layout stored it; the Modern window keeps only the two column widths",
        pr: "#276",
    },
    Dropped {
        file: SETTINGS,
        path: "/lastImportSettings",
        reason: "the Import dialog stored it; Import is gone, Shoebox does the importing",
        pr: "#296",
    },
    Dropped {
        file: SETTINGS,
        path: "/imageCacheSize",
        reason: "the decoded-photo cache is limited by size now; the stored count is converted once to decodedPhotoCacheMb",
        pr: "#487",
    },
];

/// Removes from `extra`, the unknown top-level keys of `file` (`SETTINGS` or
/// `EDIT`), every one in [`DROPPED`]. Returns whether any was there.
pub fn strip(extra: &mut Map<String, Value>, file: &str) -> bool {
    let before = extra.len();
    extra.retain(|key, _| {
        !DROPPED
            .iter()
            .any(|d| d.file == file && d.path.strip_prefix('/').is_some_and(|rest| rest == key))
    });
    extra.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_retired_top_level_keys_of_that_file_are_removed() {
        let Value::Object(mut extra) = serde_json::json!({
            "baselineExposure": 1, "agFuture": { "x": 1 }, "workspace": 2
        }) else {
            unreachable!()
        };
        assert!(!strip(&mut extra, EDIT));
        assert_eq!(extra.len(), 3);
        assert!(strip(&mut extra, SETTINGS));
        let mut kept: Vec<&String> = extra.keys().collect();
        kept.sort();
        assert_eq!(kept, ["agFuture", "workspace"]);
        assert!(!strip(&mut extra, SETTINGS));
    }
}
