//! Moving a photo with its files from one folder to another. Ours.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A move worked out before any file is touched: each file's destination,
/// and each photo's new path.
pub(crate) struct MovePlan {
    /// A file, where it goes, and whether it stays where it is too (an XMP
    /// another photo still uses).
    pub(crate) operations: Vec<(PathBuf, PathBuf, bool)>,
    pub(crate) renames: HashMap<String, String>,
}

impl MovePlan {
    /// Moves the files in order, and stops at the first that fails. Returns
    /// the photos that did move, old path to new, whether or not it stopped.
    pub(crate) fn carry_out(self) -> (HashMap<String, String>, Result<(), String>) {
        let mut moved = HashSet::new();
        let mut result = Ok(());
        for (source, dest, keep_source) in self.operations {
            if let Err(e) = move_one_file(&source, &dest, keep_source) {
                result = Err(e);
                break;
            }
            moved.insert(source);
        }
        let mut renames = self.renames;
        renames.retain(|photo, _| moved.contains(Path::new(photo)));
        (renames, result)
    }
}

fn move_one_file(source: &Path, dest: &Path, keep_source: bool) -> Result<(), String> {
    if keep_source {
        fs::copy(source, dest)
            .map_err(|e| format!("Move failed during copy for {}: {}", source.display(), e))?;
        return Ok(());
    }
    if fs::rename(source, dest).is_err() {
        fs::copy(source, dest)
            .map_err(|e| format!("Move failed during copy for {}: {}", source.display(), e))?;

        if let Err(e) = fs::remove_file(source) {
            log::warn!(
                "Moved file successfully, but failed to delete original {}: {}",
                source.display(),
                e
            );
        }
    }
    Ok(())
}
