//! What is in the AI models folder, and deleting it. Ours.
//!
//! The list of what can be there is `model_catalog`. This reads the folder
//! against it for Settings > Processing > AI Models, and removes one entry's files on
//! request. It never downloads: each feature fetches its own model again the
//! next time it is used.
//!
//! WHAT IT WILL DELETE
//!
//! Only the names a catalogue entry lists, joined onto the models folder, and
//! only after checking each is a bare name, is not a link, and resolves to
//! something directly inside that folder. The settings page sends an id, never a
//! path, so nothing it says can name a file outside the list.
//!
//! WHEN IT CANNOT
//!
//! A model that is loaded can usually be deleted anyway: ONNX Runtime reads the
//! file into memory, and the feature keeps working until the app closes. Two
//! things cannot go while Argentum runs, and are removed the next time it
//! starts instead, before anything can load them:
//!
//! - anything Windows refuses to delete because it is open, and
//! - the graphics card runtime once this run has pointed ONNX Runtime at it
//!   (see `gpu_runtime::held_until_exit`). Windows might well let that one go,
//!   and then the next AI feature used would crash looking for it.
//!
//! The page says so, and shows the entry as waiting for a restart, rather than
//! reporting a deletion that has not happened.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Manager;

use super::model_catalog::{self, CATALOGUE, GPU_RUNTIME, Model, Stored};

/// Ids waiting to be removed at the next start, one per line.
const PENDING: &str = "argentum-remove-at-start.txt";

/// The folder every AI feature downloads into. RapidRAW's `get_models_dir`,
/// `super_resolution::models_dir` and `gpu_runtime` all name this same one.
///
/// Not created here: looking at the settings page should not make a folder.
pub fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("models"))
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OnDisk {
    Downloaded,
    /// Some of an entry's files and not others: an interrupted download.
    Partial,
    Missing,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub purpose: &'static str,
    pub on_disk: OnDisk,
    /// Size on disk now.
    pub bytes: u64,
    pub download_bytes: Option<u64>,
    /// Deleting it was refused while Argentum runs; it goes at the next start.
    pub removing_at_next_start: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub models: Vec<ModelStatus>,
    /// Everything in the folder, listed or not.
    pub total_bytes: u64,
    /// The part of that no entry claims: a half-finished download, a file an
    /// update renamed, a model a newer version of Argentum added.
    pub other_bytes: u64,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Deleted,
    /// Could not go now; it is removed the next time Argentum starts.
    AtNextStart,
    NotThere,
}

/// A single path component, with nothing that could climb out of the folder.
pub(super) fn is_bare_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name)
}

/// Size of a file, or of a folder and everything under it. Links count as
/// nothing and are not followed.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        0
    } else if meta.is_dir() {
        fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size_of(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// Real folders in `dir` whose names start with `prefix`.
fn folders_starting_with(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(prefix))
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// What of this entry is on disk now.
fn present(dir: &Path, model: &Model) -> Vec<PathBuf> {
    match model.stored {
        Stored::Files(files) => files
            .iter()
            .map(|(name, _)| dir.join(name))
            .filter(|path| fs::symlink_metadata(path).is_ok())
            .collect(),
        Stored::FoldersStartingWith { prefix, .. } => folders_starting_with(dir, prefix),
    }
}

/// Does some entry claim this name in the folder?
fn claimed(name: &str, is_dir: bool) -> bool {
    name == PENDING
        || CATALOGUE.iter().any(|m| match m.stored {
            Stored::Files(files) => files.iter().any(|(file, _)| *file == name),
            Stored::FoldersStartingWith { prefix, .. } => is_dir && name.starts_with(prefix),
        })
}

fn pending(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join(PENDING))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn set_pending(dir: &Path, ids: &[String]) -> io::Result<()> {
    let path = dir.join(PENDING);
    if ids.is_empty() {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        fs::write(path, ids.join("\n") + "\n")
    }
}

/// Every catalogue entry against what is in `dir`.
pub fn list(dir: &Path) -> Listing {
    let waiting: HashSet<String> = pending(dir).into_iter().collect();
    let mut models = Vec::new();
    for model in CATALOGUE {
        let found = present(dir, model);
        let expected = match model.stored {
            Stored::Files(files) => files.len(),
            Stored::FoldersStartingWith { .. } => found.len().max(1),
        };
        let on_disk = match found.len() {
            0 => OnDisk::Missing,
            n if n < expected => OnDisk::Partial,
            _ => OnDisk::Downloaded,
        };
        if on_disk == OnDisk::Missing && !model.listed_when_missing {
            continue;
        }
        models.push(ModelStatus {
            id: model.id,
            label: model.label,
            purpose: model.purpose,
            on_disk,
            bytes: found.iter().map(|p| size_of(p)).sum(),
            download_bytes: model.download_bytes,
            removing_at_next_start: on_disk != OnDisk::Missing && waiting.contains(model.id),
        });
    }

    let mut total_bytes = 0;
    let mut other_bytes = 0;
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let size = size_of(&entry.path());
        total_bytes += size;
        let is_dir = entry
            .file_type()
            .is_ok_and(|t| t.is_dir() && !t.is_symlink());
        let name = entry.file_name();
        if !name.to_str().is_some_and(|n| claimed(n, is_dir)) {
            other_bytes += size;
        }
    }

    Listing {
        models,
        total_bytes,
        other_bytes,
    }
}

/// Windows' ways of saying a file is open somewhere.
fn in_use(e: &io::Error) -> bool {
    if cfg!(windows) {
        // 5: access denied, which is what deleting a loaded DLL returns.
        // 32 and 33: open, or locked, by someone who did not allow deletion.
        matches!(e.raw_os_error(), Some(5 | 32 | 33))
            || matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy
            )
    } else {
        e.kind() == io::ErrorKind::ResourceBusy
    }
}

/// `target` is a bare name directly inside `root`, and not a link.
fn check_inside(root: &Path, target: &Path) -> Result<(), String> {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_bare_name(n))
        .ok_or_else(|| format!("Refusing to delete {}: not a plain name", target.display()))?;
    let meta = fs::symlink_metadata(target).map_err(|e| format!("{name}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{name} is a link, not something Argentum downloaded, so it was left alone"
        ));
    }
    let real = fs::canonicalize(target).map_err(|e| format!("{name}: {e}"))?;
    if real.parent() != Some(root) {
        return Err(format!(
            "Refusing to delete {name}: it is not inside the models folder"
        ));
    }
    Ok(())
}

enum Removal {
    Done,
    Refused,
}

/// Remove what of `model` is on disk. Refused if Windows would not let go of
/// some of it; anything else that goes wrong is an error.
fn remove_now(dir: &Path, model: &Model) -> Result<Removal, String> {
    let targets = present(dir, model);
    if targets.is_empty() {
        return Ok(Removal::Done);
    }
    let root = fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let mut refused = false;
    for target in targets {
        check_inside(&root, &target)?;
        let result = if fs::symlink_metadata(&target).is_ok_and(|m| m.is_dir()) {
            fs::remove_dir_all(&target)
        } else {
            fs::remove_file(&target)
        };
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) if in_use(&e) => refused = true,
            Err(e) => {
                let name = target.file_name().unwrap_or_default().to_string_lossy();
                return Err(format!("Could not delete {name}: {e}"));
            }
        }
    }
    Ok(if refused {
        Removal::Refused
    } else {
        Removal::Done
    })
}

/// Delete one catalogue entry's files.
///
/// `held` says the entry must stay on disk until the app exits whatever
/// Windows would allow; it is then only marked for removal at the next start.
pub fn delete(dir: &Path, id: &str, held: bool) -> Result<Outcome, String> {
    let model = model_catalog::find(id).ok_or_else(|| format!("No AI model called {id}"))?;
    if present(dir, model).is_empty() {
        return Ok(Outcome::NotThere);
    }

    let mut waiting = pending(dir);
    let outcome = if held {
        Removal::Refused
    } else {
        remove_now(dir, model)?
    };
    waiting.retain(|w| w != id);
    if let Removal::Refused = outcome {
        waiting.push(id.to_string());
    }
    set_pending(dir, &waiting).map_err(|e| e.to_string())?;

    Ok(match outcome {
        Removal::Done => Outcome::Deleted,
        Removal::Refused => Outcome::AtNextStart,
    })
}

/// `delete`, for the settings page: asks `gpu_runtime` whether the runtime is
/// held this run.
pub fn delete_for_app(app: &tauri::AppHandle, id: &str) -> Result<Outcome, String> {
    let held = id == GPU_RUNTIME && super::gpu_runtime::held_until_exit();
    delete(&models_dir(app)?, id, held)
}

/// Finish deletions that had to wait for a restart, and remove what a removed
/// feature left behind (`model_catalog::RETIRED`).
///
/// Called from `startup::init` before the graphics card runtime is pinned and
/// before any feature can load a model, which is the one moment nothing in
/// the folder is in use. Anything still refused stays on the list for next time.
pub fn finish_pending_removals(dir: &Path) {
    remove_retired(dir);
    let waiting = pending(dir);
    if waiting.is_empty() {
        return;
    }
    let mut still = Vec::new();
    for id in waiting {
        let Some(model) = model_catalog::find(&id) else {
            continue;
        };
        match remove_now(dir, model) {
            Ok(Removal::Done) => log::info!("[models] removed {id}, as asked last run"),
            Ok(Removal::Refused) => still.push(id),
            Err(e) => log::warn!("[models] could not remove {id}: {e}"),
        }
    }
    if let Err(e) = set_pending(dir, &still) {
        log::warn!("[models] could not update the removal list: {e}");
    }
}

/// Delete the files of features that are gone. Only plain files directly in
/// the folder, never through a link; a failure is logged and tried again at
/// the next start.
fn remove_retired(dir: &Path) {
    let Ok(root) = fs::canonicalize(dir) else {
        return;
    };
    for name in model_catalog::RETIRED {
        let path = dir.join(name);
        if fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let removed = check_inside(&root, &path)
            .and_then(|()| fs::remove_file(&path).map_err(|e| e.to_string()));
        match removed {
            Ok(()) => log::info!("[models] removed {name}: nothing uses it any more"),
            Err(e) => log::warn!("[models] could not remove {name}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: usize) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    fn status<'a>(listing: &'a Listing, id: &str) -> Option<&'a ModelStatus> {
        listing.models.iter().find(|m| m.id == id)
    }

    #[test]
    fn bare_names_only() {
        assert!(is_bare_name("u2net.onnx"));
        assert!(is_bare_name("onnxruntime-directml-"));
        for bad in [
            "",
            ".",
            "..",
            "../u2net.onnx",
            "a/b",
            "a\\b",
            "C:u2net.onnx",
            "nul\0",
        ] {
            assert!(!is_bare_name(bad), "{bad:?} passed");
        }
    }

    #[test]
    fn an_empty_or_missing_folder_lists_everything_as_not_downloaded() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [tmp.path().to_path_buf(), tmp.path().join("models")] {
            let listing = list(&dir);
            assert_eq!(listing.total_bytes, 0);
            assert!(listing.models.iter().all(|m| m.on_disk == OnDisk::Missing));
            assert!(status(&listing, "sky").is_some());
            // Only listed where it exists: it never does off Windows.
            assert!(status(&listing, GPU_RUNTIME).is_none());
        }
    }

    #[test]
    fn sizes_and_states_come_from_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "u2net.onnx", 1000);
        write(dir, "sam_vit_b_01ec64_encoder.onnx", 300);
        write(dir, "onnxruntime-directml-1.22.0/onnxruntime.dll", 50);
        write(dir, "onnxruntime-directml-1.21.0/onnxruntime.dll", 20);
        write(dir, ".u2net.onnx.download", 7);
        write(dir, "skyseg-u2net.onnx", 5);

        let listing = list(dir);
        let foreground = status(&listing, "foreground").unwrap();
        assert_eq!(
            (foreground.on_disk, foreground.bytes),
            (OnDisk::Downloaded, 1000)
        );
        let subject = status(&listing, "subject").unwrap();
        assert_eq!((subject.on_disk, subject.bytes), (OnDisk::Partial, 300));
        let runtime = status(&listing, GPU_RUNTIME).unwrap();
        assert_eq!((runtime.on_disk, runtime.bytes), (OnDisk::Downloaded, 70));
        assert_eq!(status(&listing, "sky").unwrap().on_disk, OnDisk::Missing);
        assert_eq!(listing.other_bytes, 12);
        assert_eq!(listing.total_bytes, 1000 + 300 + 70 + 12);
    }

    #[test]
    fn deleting_removes_that_entry_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "clip_model.onnx", 10);
        write(dir, "clip_tokenizer.json", 10);
        write(dir, "u2net.onnx", 10);
        write(dir, "unrelated.txt", 10);

        assert_eq!(delete(dir, "tagging", false), Ok(Outcome::Deleted));
        assert!(!dir.join("clip_model.onnx").exists());
        assert!(!dir.join("clip_tokenizer.json").exists());
        assert!(dir.join("u2net.onnx").exists());
        assert!(dir.join("unrelated.txt").exists());

        assert_eq!(delete(dir, "tagging", false), Ok(Outcome::NotThere));
        assert!(delete(dir, "../unrelated.txt", false).is_err());
        assert!(delete(dir, "unrelated.txt", false).is_err());
    }

    #[test]
    fn the_runtime_goes_with_every_version_and_only_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "onnxruntime-directml-1.22.0/onnxruntime.dll", 10);
        write(dir, "onnxruntime-directml-1.21.0/onnxruntime.dll", 10);
        write(dir, "onnxruntime-directml-notes.txt", 10);
        write(dir, "onnxruntime-other/onnxruntime.dll", 10);

        assert_eq!(delete(dir, GPU_RUNTIME, false), Ok(Outcome::Deleted));
        assert!(!dir.join("onnxruntime-directml-1.22.0").exists());
        assert!(!dir.join("onnxruntime-directml-1.21.0").exists());
        assert!(dir.join("onnxruntime-directml-notes.txt").exists());
        assert!(dir.join("onnxruntime-other/onnxruntime.dll").exists());
    }

    /// The runtime after this run has pointed ONNX Runtime at it: left in
    /// place, shown as waiting, and removed by the next start.
    #[test]
    fn a_held_entry_waits_for_the_next_start() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "onnxruntime-directml-1.22.0/onnxruntime.dll", 10);

        assert_eq!(delete(dir, GPU_RUNTIME, true), Ok(Outcome::AtNextStart));
        assert!(
            dir.join("onnxruntime-directml-1.22.0/onnxruntime.dll")
                .exists()
        );
        let listing = list(dir);
        assert!(
            status(&listing, GPU_RUNTIME)
                .unwrap()
                .removing_at_next_start
        );
        // The note to itself is not a model, and not "other files" either.
        assert_eq!(listing.other_bytes, 0);

        finish_pending_removals(dir);
        assert!(!dir.join("onnxruntime-directml-1.22.0").exists());
        assert!(!dir.join(PENDING).exists());
        assert!(status(&list(dir), GPU_RUNTIME).is_none());
    }

    /// What a removed feature downloaded goes at the next start, with no
    /// entry to ask for it; the models beside it stay.
    #[test]
    fn a_retired_model_goes_at_the_next_start() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let retired = model_catalog::RETIRED[0];
        write(dir, retired, 10);
        write(dir, "u2net.onnx", 10);
        // Until then nothing lists it: it is part of "other files".
        assert_eq!(list(dir).other_bytes, 10);

        finish_pending_removals(dir);
        assert!(!dir.join(retired).exists());
        assert!(dir.join("u2net.onnx").exists());
        assert_eq!(list(dir).other_bytes, 0);
    }

    /// Windows refuses to delete a file someone holds open without sharing
    /// deletion. That is reported as waiting for a restart, not as an error and
    /// not as done.
    #[cfg(windows)]
    #[test]
    fn a_file_windows_will_not_release_waits_for_the_next_start() {
        use std::os::windows::fs::OpenOptionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "lama_fp16.onnx", 10);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ, and not FILE_SHARE_DELETE
            .open(dir.join("lama_fp16.onnx"))
            .unwrap();

        assert_eq!(delete(dir, "inpainting", false), Ok(Outcome::AtNextStart));
        assert!(dir.join("lama_fp16.onnx").exists());
        assert!(
            status(&list(dir), "inpainting")
                .unwrap()
                .removing_at_next_start
        );

        drop(held);
        finish_pending_removals(dir);
        assert!(!dir.join("lama_fp16.onnx").exists());
        assert!(!dir.join(PENDING).exists());
    }

    /// Deleting again once it is possible clears it from the waiting list.
    #[test]
    fn a_later_deletion_clears_the_waiting_list() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "u2net.onnx", 10);
        assert_eq!(delete(dir, "foreground", true), Ok(Outcome::AtNextStart));
        assert_eq!(delete(dir, "foreground", false), Ok(Outcome::Deleted));
        assert!(!dir.join(PENDING).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed_out_of_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(outside.path(), "precious.onnx", 10);
        std::os::unix::fs::symlink(outside.path().join("precious.onnx"), dir.join("u2net.onnx"))
            .unwrap();

        assert!(delete(dir, "foreground", false).is_err());
        assert!(outside.path().join("precious.onnx").exists());
    }
}
