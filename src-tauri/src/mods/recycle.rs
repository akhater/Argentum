//! A library delete goes to the Recycle Bin or Trash, and never past it unasked.
//! Ours.
//!
//! RapidRAW deleted a file for good, without a word, wherever the bin refused it
//! (a network share, a card, a path the system will not trash). Here a refused
//! file stays where it is: [`recycle`] reports it, the interface says that these
//! files cannot go to the bin, and only a yes there reaches
//! [`remove_permanently`]. A batch is decided file by file: what the bin takes
//! goes, and the files it refuses are asked about, so one stubborn file does not
//! decide for the rest.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;

/// What a delete did, as the interface hears it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeleteOutcome {
    /// Everything went to the bin.
    Done,
    /// The bin took `recycled` files and refused `files`, which are untouched.
    CannotRecycle { files: Vec<String>, recycled: usize },
}

/// Moves `files` to the bin with `trash`, all at once and then, if that fails,
/// one at a time: the bin does not say which file it refused, so what is still
/// there afterwards is what it refused. Never deletes anything itself.
pub fn recycle(
    files: &[PathBuf],
    trash: impl Fn(&[PathBuf]) -> Result<(), String>,
) -> DeleteOutcome {
    if files.is_empty() || trash(files).is_ok() {
        return DeleteOutcome::Done;
    }
    let mut refused = Vec::new();
    for file in files.iter().filter(|file| file.exists()) {
        if let Err(e) = trash(std::slice::from_ref(file)) {
            log::warn!("The bin refused {}: {e}", file.display());
            refused.push(file.to_string_lossy().into_owned());
        }
    }
    if refused.is_empty() {
        return DeleteOutcome::Done;
    }
    DeleteOutcome::CannotRecycle {
        recycled: files.len().saturating_sub(refused.len()),
        files: refused,
    }
}

/// [`recycle`] with the system's bin.
pub fn recycle_with_system_bin(files: &[PathBuf]) -> DeleteOutcome {
    recycle(files, crate::mods::clear_scope::system_bin)
}

/// Deletes `files` for good: only after the person said yes to it.
pub fn remove_permanently(files: &[PathBuf]) -> Result<(), String> {
    let mut failed = 0;
    for file in files {
        let removed = if file.is_dir() {
            fs::remove_dir_all(file)
        } else if file.is_file() {
            fs::remove_file(file)
        } else {
            continue;
        };
        if let Err(e) = removed {
            log::warn!("Failed to delete {}: {e}", file.display());
            failed += 1;
        }
    }
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} files could not be deleted.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bin that takes any file but those named in `refuse`; like the real one,
    /// a batch holding a refused file fails whole, and a file it takes is gone.
    fn bin(refuse: &'static [&'static str]) -> impl Fn(&[PathBuf]) -> Result<(), String> {
        move |files| {
            let refused = |f: &PathBuf| refuse.iter().any(|r| f.ends_with(r));
            if files.iter().any(refused) {
                return Err("no bin here".into());
            }
            files.iter().for_each(|f| fs::remove_file(f).unwrap());
            Ok(())
        }
    }

    fn folder_with(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let files = names
            .iter()
            .map(|name| {
                let path = dir.path().join(name);
                fs::write(&path, name).unwrap();
                path
            })
            .collect();
        (dir, files)
    }

    #[test]
    fn a_bin_that_works_takes_everything() {
        let (_dir, files) = folder_with(&["a.cr2", "a.cr2.agdata"]);
        assert_eq!(recycle(&files, bin(&[])), DeleteOutcome::Done);
        assert!(files.iter().all(|f| !f.exists()));
    }

    #[test]
    fn a_bin_that_refuses_everything_deletes_nothing() {
        let (_dir, files) = folder_with(&["a.cr2", "a.cr2.agdata"]);
        let always_fails = |_: &[PathBuf]| Err("no bin here".to_string());
        let outcome = recycle(&files, always_fails);
        let DeleteOutcome::CannotRecycle {
            files: refused,
            recycled,
        } = outcome
        else {
            panic!("expected CannotRecycle");
        };
        assert_eq!((refused.len(), recycled), (2, 0));
        assert!(files.iter().all(|f| f.exists()));
    }

    #[test]
    fn a_mixed_batch_recycles_what_it_can_and_reports_the_rest() {
        let (_dir, files) = folder_with(&["a.cr2", "b.cr2", "b.cr2.agdata"]);
        let outcome = recycle(&files, bin(&["b.cr2"]));
        let DeleteOutcome::CannotRecycle {
            files: refused,
            recycled,
        } = outcome
        else {
            panic!("expected CannotRecycle");
        };
        assert_eq!(refused, vec![files[1].to_string_lossy().into_owned()]);
        assert_eq!(recycled, 2);
        assert!(!files[0].exists() && files[1].exists() && !files[2].exists());
    }

    #[test]
    fn nothing_to_delete_is_done() {
        assert_eq!(recycle(&[], bin(&[])), DeleteOutcome::Done);
    }

    #[test]
    fn only_the_explicit_command_deletes_for_good() {
        let (dir, files) = folder_with(&["a.cr2"]);
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("x"), "x").unwrap();
        let all = [files[0].clone(), sub.clone()];
        remove_permanently(&all).unwrap();
        assert!(!all[0].exists() && !sub.exists());
        // A file already gone is no failure.
        remove_permanently(&all).unwrap();
    }
}
