//! Snapshots: named states of a photo's edit. Ours.
//!
//! The model is RAWmakase's (src/app/snapshots.rs @ 7c3c81a, Lightroom's
//! panel): + keeps the photo as it is now under a name, a click puts it back
//! as a History step, and each one can be updated, renamed or deleted.
//! RAWmakase keeps them in its catalog; Argentum has none, so they live in the
//! photo's sidecar, as `agSnapshots` beside the edit (ImageMetadata in
//! image_processing.rs), not inside it. Inside, an undo would take a snapshot
//! back out, and every save of the edit rewrites it; beside it, the edit's own
//! save (save_metadata_and_update_thumbnail) reads the sidecar and keeps them.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    /// When it was made or last updated, in milliseconds since 1970.
    pub at: u64,
    /// The whole edit as it was.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub adjustments: Value,
}

/// The photo's snapshots, none if it has no sidecar or none were made.
pub fn read(sidecar: &Path) -> Vec<Snapshot> {
    crate::exif_processing::load_sidecar(sidecar)
        .ag_snapshots
        .unwrap_or_default()
}

/// Keep `list` as the photo's snapshots, leaving the rest of the sidecar as
/// it is. An empty list takes the field out.
pub fn write(sidecar: &Path, source: &Path, list: Vec<Snapshot>) -> Result<(), String> {
    crate::mods::write_locks::update_sidecar(sidecar, source, |metadata| {
        metadata.ag_snapshots = (!list.is_empty()).then_some(list);
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot(name: &str, exposure: f64) -> Snapshot {
        Snapshot {
            id: format!("id-{name}"),
            name: name.to_string(),
            at: 1_760_000_000_000,
            adjustments: json!({ "exposure": exposure }),
        }
    }

    #[test]
    fn snapshots_are_kept_beside_the_edit() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("DSC0001.ARW");
        let sidecar = dir.path().join("DSC0001.ARW.rrdata");
        std::fs::write(
            &sidecar,
            r#"{ "version": 1, "rating": 4, "adjustments": { "exposure": 1.5 }, "tags": ["kept"] }"#,
        )
        .unwrap();

        assert!(read(&sidecar).is_empty());
        let list = vec![snapshot("First pass", 0.5), snapshot("Warm", 1.0)];
        write(&sidecar, &photo, list.clone()).unwrap();
        assert_eq!(read(&sidecar), list);

        // The edit and the rest of the sidecar are as they were.
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
        assert_eq!(saved["adjustments"]["exposure"], json!(1.5));
        assert_eq!(saved["rating"], json!(4));
        assert_eq!(saved["tags"], json!(["kept"]));
        assert!(saved["adjustments"].get("agSnapshots").is_none());
    }

    #[test]
    fn deleting_the_last_one_takes_the_field_out() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("DSC0002.ARW");
        let sidecar = dir.path().join("DSC0002.ARW.rrdata");
        write(&sidecar, &photo, vec![snapshot("Only", 0.0)]).unwrap();
        write(&sidecar, &photo, Vec::new()).unwrap();
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
        assert!(saved.get("agSnapshots").is_none());
        assert!(read(&sidecar).is_empty());
    }

    #[test]
    fn a_snapshot_save_beside_the_editors_never_undoes_its_edit() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("DSC0003.ARW");
        std::fs::write(&photo, "raw").unwrap();
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        const LAST: u32 = 300;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                for i in 0..=LAST {
                    write(&sidecar, &photo, vec![snapshot("Loop", f64::from(i))]).unwrap();
                }
            });
            scope.spawn(|| {
                for i in 0..=LAST {
                    crate::mods::write_locks::update_sidecar(&sidecar, &photo, |m| {
                        m.adjustments = json!({ "exposure": i });
                    })
                    .unwrap();
                }
            });
        });

        let kept = crate::exif_processing::load_sidecar(&sidecar);
        assert_eq!(kept.adjustments["exposure"], json!(LAST));
        assert_eq!(kept.ag_snapshots.map(|list| list.len()), Some(1));
    }

    #[test]
    fn a_save_that_fails_leaves_the_old_sidecar_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("DSC0004.ARW");
        std::fs::write(&photo, "raw").unwrap();
        let sidecar = crate::file_management::sidecar_for(&photo, None);
        write(&sidecar, &photo, vec![snapshot("Kept", 1.0)]).unwrap();
        let before = std::fs::read(&sidecar).unwrap();

        // A file nobody may replace (Windows) or a folder nobody may add to
        // (elsewhere).
        let locked = if cfg!(windows) {
            sidecar.as_path()
        } else {
            dir.path()
        };
        let mut permissions = std::fs::metadata(locked).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(locked, permissions).unwrap();

        let result = write(&sidecar, &photo, vec![snapshot("New", 2.0)]);

        let mut permissions = std::fs::metadata(locked).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(locked, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    }
}
