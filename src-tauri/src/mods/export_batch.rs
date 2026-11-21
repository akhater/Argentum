//! What each photo in an export batch gets: its own edits, its copy number
//! and the folders its name asks for.
//!
//! - The editor's live edits belong to the exact photo or virtual copy that is
//!   open (`IMG.CR3` or `IMG.CR3?vc=a1b2c3`). Compared without the copy id, a
//!   copy exported next to its open original took the original's edits, and
//!   an open copy lost its own.
//! - Virtual copies are numbered per photo in batch order, `_VC01`, `_VC02`.
//!   The number came from the digits of the copy id, or from any `_vc` in a
//!   file name, so a plain `trip_vc2.jpg` got a suffix too.
//! - A file name template with folders (`{YYYY}/{MM}/{original_filename}`)
//!   writes into those folders, which nothing created.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::export_processing::ExportAdjustmentsMode;
use crate::file_management::split_virtual_path;

/// Whether `path` is the photo or copy open in the editor, copy id included.
// RapidRAW 06ffd3ca: src-tauri/src/export_processing.rs
pub fn is_open_edit(path: &str, open_path: Option<&str>) -> bool {
    open_path == Some(path)
}

/// Whether `path` is the open edit under `mode`; a preset applied to the whole
/// batch never uses the editor's live edits.
pub(crate) fn is_open_edit_in(path: &str, mode: &ExportAdjustmentsMode) -> bool {
    match mode {
        ExportAdjustmentsMode::UseSidecars { active_path, .. } => {
            is_open_edit(path, active_path.as_deref())
        }
        ExportAdjustmentsMode::GlobalOverride(_) => false,
    }
}

/// The edits `path` is exported with: the editor's live ones when it is the
/// open edit, otherwise `saved` (its sidecar's), or the batch override.
pub(crate) fn adjustments_for(path: &str, mode: &ExportAdjustmentsMode, saved: Value) -> Value {
    match mode {
        ExportAdjustmentsMode::UseSidecars {
            active_adjustments: Some(live),
            ..
        } if is_open_edit_in(path, mode) => live.clone(),
        ExportAdjustmentsMode::UseSidecars { .. } => saved,
        ExportAdjustmentsMode::GlobalOverride(adjustments) => adjustments.clone(),
    }
}

/// Each path's copy number: `None` for a photo, 1, 2... for its virtual
/// copies in batch order.
// RapidRAW 5e0a4b7a: src-tauri/src/export/mod.rs
pub fn copy_numbers(paths: &[String]) -> Vec<Option<usize>> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    paths
        .iter()
        .map(|path| match split_virtual_path(path) {
            (photo, Some(_)) => {
                let count = counts.entry(photo).or_default();
                *count += 1;
                Some(*count)
            }
            (_, None) => None,
        })
        .collect()
}

/// `stem` with a copy's `_VC01` suffix.
pub fn with_copy_suffix(stem: String, copy_number: Option<usize>) -> String {
    match copy_number {
        Some(number) => format!("{stem}_VC{number:02}"),
        None => stem,
    }
}

/// Creates the folders `output` is written into.
// RapidRAW 7d94545a: src-tauri/src/export_processing.rs
pub fn create_output_folders(output: &Path) -> Result<(), String> {
    match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create folder '{}': {}", parent.display(), e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open(path: &str, live: Value) -> ExportAdjustmentsMode {
        ExportAdjustmentsMode::UseSidecars {
            active_path: Some(path.to_string()),
            active_adjustments: Some(live),
        }
    }

    /// The edits each photo of `batch` is exported with, given what each
    /// one's sidecar holds.
    fn exported(batch: &[(&str, Value)], mode: &ExportAdjustmentsMode) -> Vec<Value> {
        batch
            .iter()
            .map(|(path, saved)| adjustments_for(path, mode, saved.clone()))
            .collect()
    }

    #[test]
    fn a_copy_exported_with_its_open_original_keeps_its_own_edits() {
        let batch = [
            ("C:/p/IMG.CR3", json!({"exposure": 0.0})),
            ("C:/p/IMG.CR3?vc=a1b2c3", json!({"exposure": -1.0})),
        ];
        let live = json!({"exposure": 2.0});
        assert_eq!(
            exported(&batch, &open("C:/p/IMG.CR3", live.clone())),
            [live, json!({"exposure": -1.0})]
        );
    }

    #[test]
    fn an_open_copy_is_exported_with_its_live_edits() {
        let batch = [
            ("C:/p/IMG.CR3", json!({"exposure": 0.0})),
            ("C:/p/IMG.CR3?vc=a1b2c3", json!({"exposure": -1.0})),
            ("C:/p/IMG.CR3?vc=d4e5f6", json!({"exposure": 1.0})),
        ];
        let live = json!({"exposure": -3.0});
        assert_eq!(
            exported(&batch, &open("C:/p/IMG.CR3?vc=a1b2c3", live.clone())),
            [json!({"exposure": 0.0}), live, json!({"exposure": 1.0})]
        );
    }

    #[test]
    fn a_batch_override_applies_to_every_photo() {
        let preset = json!({"contrast": 10});
        let mode = ExportAdjustmentsMode::GlobalOverride(preset.clone());
        let batch = [
            ("C:/p/IMG.CR3", json!({})),
            ("C:/p/IMG.CR3?vc=1", json!({})),
        ];
        assert_eq!(exported(&batch, &mode), [preset.clone(), preset]);
        assert!(!is_open_edit_in("C:/p/IMG.CR3", &mode));
    }

    #[test]
    fn copies_are_numbered_per_photo_in_batch_order() {
        let paths: Vec<String> = [
            "C:/p/a.CR3?vc=9f8e7d",
            "C:/p/a.CR3",
            "C:/p/b.CR3?vc=123456",
            "C:/p/a.CR3?vc=000001",
            "C:/p/trip_vc2.jpg",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            copy_numbers(&paths),
            [Some(1), None, Some(1), Some(2), None]
        );
    }

    #[test]
    fn a_copy_takes_a_two_digit_suffix() {
        assert_eq!(with_copy_suffix("IMG_1".into(), Some(1)), "IMG_1_VC01");
        assert_eq!(with_copy_suffix("IMG_1".into(), Some(12)), "IMG_1_VC12");
        assert_eq!(with_copy_suffix("trip_vc2".into(), None), "trip_vc2");
    }

    #[test]
    fn a_template_can_name_the_second() {
        use chrono::TimeZone;
        let taken = chrono::Local
            .with_ymd_and_hms(2026, 11, 21, 9, 5, 7)
            .unwrap()
            .with_timezone(&chrono::Utc);
        let name = crate::file_management::generate_filename_from_template(
            "{YYYY}/{MM}/{hh}{mm}{ss}_{original_filename}",
            Path::new("C:/p/IMG_1.CR3"),
            1,
            1,
            &taken,
        );
        assert_eq!(name, "2026/11/090507_IMG_1");
    }

    #[test]
    fn a_name_with_folders_gets_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("2026").join("11").join("IMG_1.jpg");
        create_output_folders(&output).unwrap();
        assert!(output.parent().unwrap().is_dir());
        create_output_folders(Path::new("IMG_1.jpg")).unwrap();
    }
}
