//! Reading a photo's sidecar, told apart from failing to read it. Ours.
//!
//! WHY THIS EXISTS
//!
//! A sidecar that is there but cannot be read for a moment (a network share
//! dropping out, another program holding the file without read sharing) was
//! read as an empty edit, and the next rating, tag or save of the edit wrote
//! that empty edit over the real one, or deleted the file when nothing was
//! left in it. So a read tells three cases apart:
//!
//! - no sidecar: an empty edit, as for a photo never edited;
//! - one that reads: read as far as it goes (`mods/salvage.rs`); bytes that are
//!   not UTF-8 are copied aside and read with the bad bytes replaced;
//! - one that is there but does not read: an error. Every pass that writes the
//!   sidecar back returns it before writing anything; code that only shows the
//!   edit shows an empty one and writes nothing (`shown_empty`).

use std::io;
use std::path::Path;

use crate::image_processing::ImageMetadata;

/// The text of `sidecar`: `None` when there is none, an error when it is there
/// but could not be read.
pub fn read_text(sidecar: &Path) -> io::Result<Option<String>> {
    #[cfg(test)]
    if let Some(error) = injected::failure(sidecar) {
        return Err(error);
    }
    let bytes = match std::fs::read(sidecar) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Some(text)),
        Err(e) => {
            crate::mods::salvage::keep_copy(sidecar, &e);
            Ok(Some(String::from_utf8_lossy(e.as_bytes()).into_owned()))
        }
    }
}

/// What code that only shows a photo's edit shows when its sidecar could not
/// be read: an empty edit. Nothing may be written back from it.
pub fn shown_empty(sidecar: &Path, error: &io::Error) -> ImageMetadata {
    log::warn!(
        "{} could not be read ({error}); shown as unedited, left as it is",
        sidecar.display()
    );
    ImageMetadata::default()
}

/// Tests make a sidecar fail to read, on this thread, while a guard lives.
#[cfg(test)]
pub mod injected {
    use std::cell::RefCell;
    use std::io;
    use std::path::{Path, PathBuf};

    thread_local! {
        static FAILING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    }

    pub struct Failing(PathBuf);

    impl Drop for Failing {
        fn drop(&mut self) {
            FAILING.with(|failing| failing.borrow_mut().retain(|p| p != &self.0));
        }
    }

    /// Reads of `path` on this thread fail with `PermissionDenied`, as a file
    /// another program holds without read sharing does, until the guard drops.
    pub fn fail_reads_of(path: &Path) -> Failing {
        FAILING.with(|failing| failing.borrow_mut().push(path.to_path_buf()));
        Failing(path.to_path_buf())
    }

    pub(super) fn failure(path: &Path) -> Option<io::Error> {
        FAILING
            .with(|failing| failing.borrow().iter().any(|p| p == path))
            .then(|| io::Error::new(io::ErrorKind::PermissionDenied, "held by another program"))
    }
}

#[cfg(test)]
mod tests {
    use super::injected::fail_reads_of;
    use crate::exif_processing::{load_sidecar, try_load_sidecar};
    use crate::file_management::sidecar_for;
    use crate::mods::write_locks::update_sidecar;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    const EDIT: &str = r#"{ "version": 1, "rating": 3, "tags": ["user:cat"], "adjustments": { "exposure": 1.5 } }"#;

    fn photo_with(dir: &Path, sidecar_text: &[u8]) -> (PathBuf, PathBuf) {
        let photo = dir.join("IMG_0001.jpg");
        std::fs::write(&photo, "jpeg").unwrap();
        let sidecar = sidecar_for(&photo, None);
        std::fs::write(&sidecar, sidecar_text).unwrap();
        (photo, sidecar)
    }

    #[test]
    fn a_rating_change_leaves_a_sidecar_that_does_not_read_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let (photo, sidecar) = photo_with(dir.path(), EDIT.as_bytes());
        let _failing = fail_reads_of(&sidecar);

        let result = update_sidecar(&sidecar, &photo, |m| m.rating = 5);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), EDIT.as_bytes());
    }

    #[test]
    fn rating_zero_on_a_sidecar_that_does_not_read_does_not_delete_it() {
        let dir = tempfile::tempdir().unwrap();
        let only_a_rating = r#"{ "version": 1, "rating": 2 }"#;
        let (photo, sidecar) = photo_with(dir.path(), only_a_rating.as_bytes());
        let _failing = fail_reads_of(&sidecar);

        assert!(update_sidecar(&sidecar, &photo, |m| m.rating = 0).is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), only_a_rating.as_bytes());
    }

    #[test]
    fn the_editor_save_leaves_a_sidecar_that_does_not_read_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let (photo, sidecar) = photo_with(dir.path(), EDIT.as_bytes());
        let _failing = fail_reads_of(&sidecar);

        let result =
            crate::file_management::save_edit(&photo, &sidecar, json!({ "exposure": 0.2 }), None);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), EDIT.as_bytes());
    }

    #[test]
    fn the_history_save_leaves_a_sidecar_that_does_not_read_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let (photo, sidecar) = photo_with(dir.path(), EDIT.as_bytes());
        let _failing = fail_reads_of(&sidecar);

        assert!(crate::mods::history::write(&photo, &sidecar, None).is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), EDIT.as_bytes());
    }

    #[test]
    fn the_grid_shows_an_empty_edit_and_writes_nothing_back_even_with_xmp_sync() {
        let dir = tempfile::tempdir().unwrap();
        let (photo, sidecar) = photo_with(dir.path(), EDIT.as_bytes());
        // An XMP with a rating the sidecar lacks is what makes the grid write.
        std::fs::write(
            dir.path().join("IMG_0001.xmp"),
            r#"<x:xmpmeta><rdf:Description xmp:Rating="4"/></x:xmpmeta>"#,
        )
        .unwrap();
        let _failing = fail_reads_of(&sidecar);

        assert_eq!(load_sidecar(&sidecar).rating, 0);
        let shown = crate::file_management::resolve_image_metadata(
            &photo,
            &sidecar,
            true,
            &crate::app_settings::AppSettings::default(),
        );

        assert_eq!(shown.rating, 0);
        assert_eq!(std::fs::read(&sidecar).unwrap(), EDIT.as_bytes());
    }

    #[test]
    fn no_sidecar_is_an_empty_edit_and_the_first_save_makes_one() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("IMG_0002.jpg");
        std::fs::write(&photo, "jpeg").unwrap();
        let sidecar = sidecar_for(&photo, None);

        assert_eq!(try_load_sidecar(&sidecar).unwrap().rating, 0);
        update_sidecar(&sidecar, &photo, |m| m.rating = 3).unwrap();
        assert_eq!(load_sidecar(&sidecar).rating, 3);
    }

    #[test]
    fn a_sidecar_with_bytes_that_are_not_utf8_is_read_and_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let mut text = br#"{ "version": 1, "rating": 4, "tags": ["user:caf"#.to_vec();
        text.push(0xff);
        text.extend_from_slice(br#""], "adjustments": { "exposure": 1.5 } }"#);
        let (_, sidecar) = photo_with(dir.path(), &text);

        let read = try_load_sidecar(&sidecar).unwrap();

        assert_eq!(read.rating, 4);
        assert_eq!(read.adjustments["exposure"], 1.5);
        let kept = crate::mods::salvage::backup_path(&sidecar);
        assert_eq!(std::fs::read(kept).unwrap(), text);
    }
}
