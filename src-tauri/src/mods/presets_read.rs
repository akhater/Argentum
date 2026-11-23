//! Reading the saved presets, told apart from failing to read them. Ours.
//!
//! WHY THIS EXISTS
//!
//! A presets file that was there but did not read (held by another program, cut
//! short, a field in a shape a newer version wrote) was shown as an empty list,
//! and the next preset added or renamed saved that list over the file: every
//! saved preset gone, with nothing said. So a read tells three cases apart:
//!
//! - no file, or one with nothing in it: an empty list;
//! - one that reads: the list;
//! - one that is there but does not read: an error, and the bytes are copied
//!   aside (`mods/salvage.rs`). Every pass that writes the list back (import,
//!   community presets) returns the error before writing, and the interface
//!   saves nothing until a later read succeeds.

use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;

/// The list stored at `path`.
pub fn read_list<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("The presets could not be read: {e}")),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    let error = match serde_json::from_slice(&bytes) {
        Ok(list) => return Ok(list),
        Err(error) => error,
    };
    crate::mods::salvage::keep_copy(path, &error);
    Err(format!(
        "The presets could not be read ({error}); the file is left as it is."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::salvage::backup_path;

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Item {
        name: String,
    }

    fn file(text: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn an_unparseable_file_is_an_error_left_as_it_is_with_a_copy_kept() {
        let text = br#"[{ "name": "Warm" }, { "name": 5 }]"#;
        let (_dir, path) = file(text);

        assert!(read_list::<Item>(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), text);
        assert_eq!(std::fs::read(backup_path(&path)).unwrap(), text);
    }

    #[test]
    fn a_file_cut_short_is_an_error() {
        let (_dir, path) = file(br#"[{ "name": "Wa"#);
        assert!(read_list::<Item>(&path).is_err());
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_not_an_empty_list() {
        // A folder where the file should be fails to read with something other than NotFound.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        std::fs::create_dir(&path).unwrap();
        assert!(read_list::<Item>(&path).is_err());
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let list = read_list::<Item>(&dir.path().join("presets.json")).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn an_empty_file_is_an_empty_list() {
        let (_dir, path) = file(b"  \n");
        assert!(read_list::<Item>(&path).unwrap().is_empty());
    }

    #[test]
    fn a_file_that_reads_gives_its_list_and_no_copy() {
        let (_dir, path) = file(br#"[{ "name": "Warm" }]"#);
        assert_eq!(
            read_list::<Item>(&path).unwrap(),
            vec![Item {
                name: "Warm".into()
            }]
        );
        assert!(!backup_path(&path).exists());
    }
}
