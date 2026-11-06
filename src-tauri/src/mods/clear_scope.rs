//! What Settings > Data's "Clear sidecars" and the tagging page's "Clear tags"
//! buttons may touch, and how cleared files leave. Ours.
//!
//! RapidRAW (`src-tauri/src/file_management.rs` `clear_all_sidecars`,
//! `src-tauri/src/tagging.rs` `clear_all_tags`, @ 1.6.6) walked every root
//! folder, which was safe when roots were folders added by hand; Argentum's
//! roots are whole disks, so the buttons now take the open folder, and this
//! refuses a disk itself in case anything still sends one. A cleared sidecar
//! goes to the Recycle Bin or Trash and nowhere else: unlike a library delete
//! (`file_ops::trash_or_delete`), a file the bin refuses stays where it is.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Why a clear did not run, or did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum ClearRefused {
    /// The folder is not there.
    NoFolder(String),
    /// The folder is a whole disk or share: `C:\`, `/`, `\\server\share`.
    DriveRoot(String),
    /// The bin took `moved` files and refused `stayed`, which are untouched.
    SomeStayed { moved: usize, stayed: usize },
}

impl fmt::Display for ClearRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFolder(path) => write!(f, "Folder does not exist: {path}"),
            Self::DriveRoot(path) => write!(
                f,
                "{path} is a whole disk. Open the folder to clear, not the disk itself."
            ),
            Self::SomeStayed { moved, stayed } => write!(
                f,
                "Moved {moved} files to the Recycle Bin; {stayed} could not be moved and were left as they are."
            ),
        }
    }
}

impl From<ClearRefused> for String {
    fn from(refused: ClearRefused) -> Self {
        refused.to_string()
    }
}

/// `path` as a folder a clear may walk: it exists and is not a disk's root.
pub fn folder_to_clear(path: &str) -> Result<&Path, ClearRefused> {
    let canonical = fs::canonicalize(path).map_err(|_| ClearRefused::NoFolder(path.into()))?;
    if canonical.parent().is_none() {
        return Err(ClearRefused::DriveRoot(path.into()));
    }
    Ok(Path::new(path))
}

/// Moves `files` to the bin with `trash`, all at once and then, if that
/// fails, one at a time: the bin says nothing of which file it refused, so
/// what is still there afterwards is what failed. Never deletes anything
/// itself. Gives the number moved.
pub fn move_to_bin(
    files: &[PathBuf],
    trash: impl Fn(&[PathBuf]) -> Result<(), String>,
) -> Result<usize, ClearRefused> {
    if files.is_empty() || trash(files).is_ok() {
        return Ok(files.len());
    }
    let mut stayed = 0;
    for file in files.iter().filter(|file| file.exists()) {
        if let Err(e) = trash(std::slice::from_ref(file)) {
            log::warn!("Left {}: the bin refused it: {e}", file.display());
            stayed += 1;
        }
    }
    match stayed {
        0 => Ok(files.len()),
        stayed => Err(ClearRefused::SomeStayed {
            moved: files.len() - stayed,
            stayed,
        }),
    }
}

/// The system's Recycle Bin or Trash, for [`move_to_bin`].
pub fn system_bin(files: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(files).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::clear_sidecars_in;
    use std::cell::RefCell;

    fn disk_root() -> PathBuf {
        std::env::temp_dir()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf()
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn no_bin(_: &[PathBuf]) -> Result<(), String> {
        panic!("nothing may be cleared")
    }

    #[test]
    fn a_disk_root_is_refused() {
        let root = text(&disk_root());
        assert_eq!(
            folder_to_clear(&root),
            Err(ClearRefused::DriveRoot(root.clone()))
        );
    }

    #[test]
    fn a_folder_on_the_disk_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = text(dir.path());
        assert_eq!(folder_to_clear(&folder), Ok(Path::new(&folder)));
    }

    #[test]
    fn a_folder_that_is_not_there_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let gone = text(&dir.path().join("gone"));
        assert_eq!(
            folder_to_clear(&gone),
            Err(ClearRefused::NoFolder(gone.clone()))
        );
    }

    /// `C:\Photos\..` is `C:\` all the same.
    #[test]
    fn a_path_that_climbs_to_the_disk_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut climb = dir.path().to_path_buf();
        for _ in dir.path().ancestors().skip(1) {
            climb.push("..");
        }
        let climb = text(&climb);
        assert_eq!(
            folder_to_clear(&climb),
            Err(ClearRefused::DriveRoot(climb.clone()))
        );
    }

    #[test]
    fn clear_sidecars_refuses_a_disk_root_before_touching_anything() {
        let refused = clear_sidecars_in(&text(&disk_root()), no_bin);
        assert!(
            matches!(refused, Err(ClearRefused::DriveRoot(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn clear_sidecars_bins_only_the_open_folders_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let open = dir.path().join("Trip");
        let inside = [
            open.join("IMG.CR2.agdata"),
            open.join("IMG.CR2.agexif"),
            open.join("Day 2").join("IMG2.NEF.agdata"),
        ];
        let outside = dir.path().join("Other").join("IMG3.ARW.agdata");
        let photo = open.join("IMG.CR2");
        for file in inside.iter().chain([&outside, &photo]) {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "{}").unwrap();
        }
        let binned = RefCell::new(Vec::new());

        let moved = clear_sidecars_in(&text(&open), |files| {
            binned.borrow_mut().extend_from_slice(files);
            Ok(())
        });

        let mut binned = binned.into_inner();
        binned.sort();
        let mut expected = inside.to_vec();
        expected.sort();
        assert_eq!(moved, Ok(3));
        assert_eq!(binned, expected);
        // Only the bin removes them: nothing was deleted on the side.
        assert!(inside.iter().all(|file| file.exists()));
        assert!(outside.exists() && photo.exists());
    }

    #[test]
    fn a_file_the_bin_refuses_stays_and_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = ["a", "b", "c"]
            .map(|name| dir.path().join(format!("{name}.agdata")))
            .into();
        for file in &files {
            fs::write(file, "{}").unwrap();
        }
        let stuck = files[1].clone();

        let moved = move_to_bin(&files, |batch| {
            if batch.contains(&stuck) {
                return Err("refused".into());
            }
            batch.iter().for_each(|file| fs::remove_file(file).unwrap());
            Ok(())
        });

        assert_eq!(
            moved,
            Err(ClearRefused::SomeStayed {
                moved: 2,
                stayed: 1
            })
        );
        assert!(stuck.exists());
        assert!(!files[0].exists() && !files[2].exists());
    }

    #[test]
    fn nothing_to_clear_never_calls_the_bin() {
        assert_eq!(move_to_bin(&[], no_bin), Ok(0));
    }
}
