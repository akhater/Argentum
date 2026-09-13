//! Argentum's own preferences file, with more than one thing in it. Ours.
//!
//! WHY THIS EXISTS
//!
//! `argentum-processing.json` sits beside the profile library because it is
//! ours: one small file of our own rather than keys in their settings, which
//! would be lines of theirs and a merge conflict every time upstream touches
//! that file.
//!
//! It held exactly one key, and the code that wrote it did this:
//!
//! ```ignore
//! let text = serde_json::to_string_pretty(&json!({ "highlightRecovery": on }))?;
//! std::fs::write(settings_path(library), text)
//! ```
//!
//! Which is correct for one setting and silently destroys every other one. The
//! second preference Argentum saved would have erased the first, and the way it
//! showed up would be a user reporting that highlight recovery keeps turning
//! itself back on — a bug with no obvious connection to the export settings that
//! caused it.
//!
//! So reading and writing go through here: load the object, change one key, put
//! it back. Nothing else in the file is touched.
//!
//! A missing, empty or corrupt file reads as "no value" rather than an error.
//! The preference is a convenience; it is not worth failing a startup over, and
//! every caller has a sensible default already.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Where the preferences live.
fn path(library: &Path) -> PathBuf {
    library.join("argentum-processing.json")
}

/// Everything currently stored, or an empty object.
fn read_all(library: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path(library))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            // A file holding a JSON array or a bare string is as unusable as a
            // corrupt one, and starting from empty is what a missing file does.
            _ => None,
        })
        .unwrap_or_default()
}

/// One preference, or `None` if it has never been set.
pub fn get(library: &Path, key: &str) -> Option<Value> {
    read_all(library).remove(key)
}

/// Serialises the read-modify-write below.
///
/// Reading the file, changing one key and writing it back is only safe against
/// losing a neighbour if no one else does it at the same time. Every writer is
/// an `ag` command, and those run on Tauri's multi-threaded runtime: two
/// controls saved a few milliseconds apart would both read the old file, each
/// insert its own key, and the second write would drop the first - the exact
/// failure this module exists to prevent, in a narrower window and therefore
/// harder to reproduce than the one it replaced.
///
/// Renders never take this lock; they read an atomic that `save` has already
/// set, so nothing on a hot path waits for a file.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set one preference, leaving every other key exactly as it was.
pub fn set(library: &Path, key: &str, value: Value) -> Result<(), String> {
    let _serialised = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut all = read_all(library);
    all.insert(key.to_string(), value);

    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&Value::Object(all)).map_err(|e| e.to_string())?;
    std::fs::write(path(library), text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-settings-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_missing_file_has_no_preferences_and_is_not_an_error() {
        let dir = scratch("missing");
        assert_eq!(get(&dir, "anything"), None);
    }

    #[test]
    fn what_is_written_is_what_is_read() {
        let dir = scratch("roundtrip");
        set(&dir, "tiffBitDepth", Value::from(8)).expect("write");
        assert_eq!(get(&dir, "tiffBitDepth"), Some(Value::from(8)));
    }

    /// The whole reason this module exists.
    #[test]
    fn writing_one_preference_leaves_the_others_alone() {
        let dir = scratch("preserve");
        set(&dir, "highlightRecovery", Value::from(false)).expect("write one");
        set(&dir, "tiffBitDepth", Value::from(8)).expect("write two");

        assert_eq!(
            get(&dir, "highlightRecovery"),
            Some(Value::from(false)),
            "saving a second preference erased the first - which is the bug this \
             module was written to make impossible",
        );
        assert_eq!(get(&dir, "tiffBitDepth"), Some(Value::from(8)));
    }

    #[test]
    fn a_preference_can_be_changed_without_disturbing_its_neighbour() {
        let dir = scratch("update");
        set(&dir, "highlightRecovery", Value::from(true)).expect("write");
        set(&dir, "tiffBitDepth", Value::from(16)).expect("write");
        set(&dir, "tiffBitDepth", Value::from(8)).expect("rewrite");

        assert_eq!(get(&dir, "tiffBitDepth"), Some(Value::from(8)));
        assert_eq!(get(&dir, "highlightRecovery"), Some(Value::from(true)));
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_rather_than_failing() {
        let dir = scratch("corrupt");
        std::fs::write(path(&dir), "this is not json").expect("write");
        assert_eq!(get(&dir, "highlightRecovery"), None);

        // And writing over it recovers, rather than failing forever.
        set(&dir, "highlightRecovery", Value::from(true)).expect("write");
        assert_eq!(get(&dir, "highlightRecovery"), Some(Value::from(true)));
    }

    #[test]
    fn an_empty_file_reads_as_no_preferences() {
        let dir = scratch("empty");
        std::fs::write(path(&dir), "").expect("write");
        assert_eq!(get(&dir, "highlightRecovery"), None);
        set(&dir, "tiffBitDepth", Value::from(8)).expect("write");
        assert_eq!(get(&dir, "tiffBitDepth"), Some(Value::from(8)));
    }

    /// Two preferences written at the same moment must both survive.
    ///
    /// Without the lock this fails intermittently: both threads read the file
    /// before either writes, and whichever writes second drops the other's key.
    /// Intermittently is the problem - it would have passed in review and lost
    /// somebody's setting in the field.
    #[test]
    fn two_threads_writing_at_once_do_not_lose_each_other() {
        let dir = scratch("concurrent");

        for round in 0..40 {
            let _ = std::fs::remove_file(path(&dir));
            let a = dir.clone();
            let b = dir.clone();
            let one = std::thread::spawn(move || set(&a, "highlightRecovery", Value::from(true)));
            let two = std::thread::spawn(move || set(&b, "tiffBitDepth", Value::from(8)));
            one.join().expect("thread a").expect("write a");
            two.join().expect("thread b").expect("write b");

            assert_eq!(
                get(&dir, "highlightRecovery"),
                Some(Value::from(true)),
                "round {round}: the export depth's write erased highlight recovery",
            );
            assert_eq!(
                get(&dir, "tiffBitDepth"),
                Some(Value::from(8)),
                "round {round}: highlight recovery's write erased the export depth",
            );
        }
    }

    /// A file holding valid JSON that is not an object - an array, say - must
    /// not make `get` panic or `set` fail.
    #[test]
    fn valid_json_of_the_wrong_shape_is_treated_as_empty() {
        let dir = scratch("wrongshape");
        std::fs::write(path(&dir), "[1, 2, 3]").expect("write");
        assert_eq!(get(&dir, "highlightRecovery"), None);
        set(&dir, "tiffBitDepth", Value::from(16)).expect("write");
        assert_eq!(get(&dir, "tiffBitDepth"), Some(Value::from(16)));
    }
}
