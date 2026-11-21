//! History: each photo's undo list, kept in its sidecar so it survives switching
//! photos and restarting Argentum. Ours.
//!
//! The model is darktable's: its XMP sidecar holds the history stack
//! (`darktable:history`) and how much of it is applied (`darktable:history_end`),
//! so undone steps are still there to redo when the photo opens again
//! (src/common/exif.cc @ f973fb0f, `_set_xmp_dt_history`). RAWmakase keeps one in
//! its catalog the same way (crates/rawmakase-catalog/src/catalog/develop_history.rs
//! @ bf5346d: a name and a full state per step, large settings stored once). Here
//! a step holds its name and only what it changed, each value under its place in
//! the edit as a JSON Pointer (RFC 6901, `"/masks/0/opacity": 50`), so 50 steps
//! weigh less than one copy of the edit.
//! src/argentum/history/historyRecord.ts writes and reads the steps; this side
//! keeps them.
//!
//! A record belongs to the edit saved beside it when it was written, and is only
//! good for that edit: it carries the edit's fingerprint, and once the edit has
//! changed (a paste, a reset, another app) `read` gives none, so the editor
//! starts a fresh History. Step 0 holds what the first state had that the edit
//! has not, and the editor also checks that replaying the steps lands on the
//! edit. A History never changes the edit.
//!
//! Kept as `agHistory` beside the edit (ImageMetadata in image_processing.rs), as
//! Snapshots are, never inside it: Copy settings, presets and sync carry the edit
//! alone. Written through their write_sidecar, which keeps no sidecar that holds
//! only defaults, so History alone never gives a photo opened and left unedited a
//! sidecar.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A step of the History: its name and what it changed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryStep {
    /// As the History tab names it.
    pub name: String,
    /// Each value the step set, under its place in the edit.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    #[cfg_attr(
        test,
        ts(as = "Option<crate::mods::ag_contract::EditValues>", optional)
    )]
    pub set: serde_json::Map<String, Value>,
    /// The places it took out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub unset: Vec<String>,
}

/// A photo's History as its sidecar keeps it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HistoryRecord {
    /// The step the edit is at, darktable's history_end. Those after it were
    /// undone and can be redone.
    pub end: usize,
    /// Oldest first. Step 0 is the state the History starts from.
    pub steps: Vec<HistoryStep>,
    /// The fingerprint of the edit saved beside it when it was written. Set
    /// here, never by the editor.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[cfg_attr(test, ts(as = "Option<String>", optional))]
    pub edit: String,
}

/// `agHistory` as read from a sidecar. One this version cannot read (damaged, or
/// written by a later one) counts as none: failing here would fail the whole
/// sidecar, and their readers take a sidecar they cannot read for an unedited photo.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<HistoryRecord>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value::<HistoryRecord>(value)
        .ok()
        .filter(|record| record.end < record.steps.len()))
}

/// A text that changes whenever the edit does: FNV-1a over its values, an
/// object's keys in order, numbers as numbers (1 and 1.0 are one). Written here:
/// none of the sources fingerprints an edit, they keep every state whole.
pub(crate) fn fingerprint(edit: &Value) -> String {
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn walk(hash: &mut u64, value: &Value) {
        match value {
            Value::Null => feed(hash, b"n"),
            Value::Bool(b) => feed(hash, if *b { b"t" } else { b"f" }),
            Value::Number(n) => {
                feed(hash, b"#");
                feed(
                    hash,
                    &n.as_f64().unwrap_or(f64::NAN).to_bits().to_le_bytes(),
                );
            }
            Value::String(text) => {
                feed(hash, &(text.len() as u64).to_le_bytes());
                feed(hash, text.as_bytes());
            }
            Value::Array(items) => {
                feed(hash, b"[");
                feed(hash, &(items.len() as u64).to_le_bytes());
                items.iter().for_each(|item| walk(hash, item));
            }
            Value::Object(map) => {
                feed(hash, b"{");
                feed(hash, &(map.len() as u64).to_le_bytes());
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for key in keys {
                    feed(hash, &(key.len() as u64).to_le_bytes());
                    feed(hash, key.as_bytes());
                    walk(hash, &map[key]);
                }
            }
        }
    }
    let mut hash = 0xcbf2_9ce4_8422_2325;
    walk(&mut hash, edit);
    format!("{hash:016x}")
}

/// The photo's History, none if its sidecar has none or its edit has changed
/// since the History was written.
pub fn read(sidecar: &Path) -> Option<HistoryRecord> {
    let metadata = crate::exif_processing::load_sidecar(sidecar);
    let edit = fingerprint(&metadata.adjustments);
    metadata.ag_history.filter(|record| record.edit == edit)
}

/// Keep `record` as the photo's History, for the edit its sidecar holds now, the
/// rest of the sidecar as it is; none takes it out. A sidecar left holding only
/// defaults is not kept.
///
/// Like their save of the edit, this reads the sidecar and writes it all back, so
/// the two must not run at once: the editor writes a History well after its own
/// save of the edit (historyKeeper.ts says when), and the sidecar's lock
/// (mods/write_locks.rs) makes them take turns if they meet.
pub fn write(source: &Path, sidecar: &Path, record: Option<HistoryRecord>) -> Result<(), String> {
    if record.as_ref().is_some_and(|r| r.end >= r.steps.len()) {
        return Err("History: its end is past its last step".to_string());
    }
    let _sidecar = crate::mods::write_locks::lock_sidecar(sidecar);
    let mut metadata = crate::exif_processing::load_sidecar(sidecar);
    let edit = fingerprint(&metadata.adjustments);
    let record = record.map(|r| HistoryRecord { edit, ..r });
    if metadata.ag_history == record {
        return Ok(());
    }
    metadata.ag_history = record;
    crate::exif_processing::write_sidecar(sidecar, source, &metadata).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What the editor writes for an edit taken to Exposure 1, then a mask's
    /// opacity to 50, both undone (historyRecord.test.ts makes the same file).
    fn record() -> HistoryRecord {
        serde_json::from_str(include_str!(
            "../../../src/argentum/history/record.fixture.json"
        ))
        .unwrap()
    }

    fn saved(sidecar: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(sidecar).unwrap()).unwrap()
    }

    #[test]
    fn the_editors_record_reads_and_writes_back_unchanged() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../src/argentum/history/record.fixture.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(record()).unwrap(), fixture);
        assert_eq!(record().steps[2].set["/masks/0/opacity"], json!(50));
    }

    #[test]
    fn history_is_kept_beside_the_edit() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("DSC0001.ARW");
        let sidecar = crate::file_management::sidecar_for(&source, None);
        std::fs::write(
            &sidecar,
            r#"{ "version": 1, "rating": 4, "adjustments": { "exposure": 0.5 }, "tags": ["kept"] }"#,
        )
        .unwrap();

        assert_eq!(read(&sidecar), None);
        write(&source, &sidecar, Some(record())).unwrap();
        let kept = read(&sidecar).unwrap();
        assert_eq!((kept.end, &kept.steps), (record().end, &record().steps));

        let saved = saved(&sidecar);
        assert_eq!(saved["adjustments"], json!({ "exposure": 0.5 }));
        assert_eq!(saved["rating"], json!(4));
        assert_eq!(saved["tags"], json!(["kept"]));
        assert!(saved["adjustments"].get("agHistory").is_none());

        write(&source, &sidecar, None).unwrap();
        assert!(self::saved(&sidecar).get("agHistory").is_none());
        assert_eq!(
            self::saved(&sidecar)["adjustments"],
            json!({ "exposure": 0.5 })
        );
    }

    #[test]
    fn history_is_only_good_for_the_edit_it_was_written_with() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("DSC0005.ARW");
        let sidecar = crate::file_management::sidecar_for(&source, None);
        let rewrite = |change: &dyn Fn(&mut crate::image_processing::ImageMetadata)| {
            let mut metadata = crate::exif_processing::load_sidecar(&sidecar);
            change(&mut metadata);
            std::fs::write(&sidecar, serde_json::to_string_pretty(&metadata).unwrap()).unwrap();
        };
        std::fs::write(
            &sidecar,
            r#"{ "version": 1, "rating": 0, "adjustments": { "exposure": 0.5, "contrast": 10 } }"#,
        )
        .unwrap();
        write(&source, &sidecar, Some(record())).unwrap();

        // Written back as it is, or rated: the same edit.
        rewrite(&|m| m.rating = 3);
        assert!(read(&sidecar).is_some());
        // Pasted over, or edited in another app: another edit.
        rewrite(&|m| m.adjustments["exposure"] = json!(0.75));
        assert_eq!(read(&sidecar), None);
    }

    #[test]
    fn the_fingerprint_follows_the_values_not_how_they_are_written() {
        let a = fingerprint(&json!({ "exposure": 1, "hsl": { "reds": { "hue": 0.5 } } }));
        let same = fingerprint(&json!({ "hsl": { "reds": { "hue": 0.5 } }, "exposure": 1.0 }));
        let other = fingerprint(&json!({ "exposure": 1, "hsl": { "reds": { "hue": 0.25 } } }));
        assert_eq!(a, same);
        assert_ne!(a, other);
        assert_ne!(
            fingerprint(&json!(["ab", "c"])),
            fingerprint(&json!(["a", "bc"]))
        );
    }

    #[test]
    fn history_alone_gives_an_unedited_photo_no_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("DSC0002.ARW");
        let sidecar = crate::file_management::sidecar_for(&source, None);

        // Steps made and all undone: the edit is the photo's defaults again.
        write(&source, &sidecar, Some(record())).unwrap();
        assert!(!sidecar.exists());

        // A sidecar holding only defaults goes, History or not.
        let defaults = crate::image_processing::ImageMetadata {
            adjustments: crate::image_processing::default_adjustments(),
            ..Default::default()
        };
        std::fs::write(&sidecar, serde_json::to_string(&defaults).unwrap()).unwrap();
        write(&source, &sidecar, Some(record())).unwrap();
        assert!(!sidecar.exists());
    }

    #[test]
    fn a_record_this_version_cannot_read_leaves_the_edit_readable() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = crate::file_management::sidecar_for(&dir.path().join("DSC0003.ARW"), None);
        for broken in [
            json!("not a record"),
            json!({ "end": 3, "steps": [{ "name": "Initial State" }] }),
            json!({ "end": 0, "steps": [{ "name": 7 }] }),
        ] {
            let text = json!({ "version": 1, "rating": 2, "adjustments": { "exposure": 1.5 }, "agHistory": broken });
            std::fs::write(&sidecar, text.to_string()).unwrap();
            let metadata = crate::exif_processing::load_sidecar(&sidecar);
            assert_eq!(metadata.adjustments, json!({ "exposure": 1.5 }));
            assert_eq!(metadata.rating, 2);
            assert_eq!(metadata.ag_history, None);
        }
    }

    #[test]
    fn an_end_past_the_last_step_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("DSC0004.ARW");
        let sidecar = crate::file_management::sidecar_for(&source, None);
        let mut wrong = record();
        wrong.end = wrong.steps.len();
        assert!(write(&source, &sidecar, Some(wrong)).is_err());
    }
}
