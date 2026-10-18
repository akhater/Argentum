//! Finding photos for the new interface: the RAWs in a folder, and a file
//! dialog for one. Both only read names; neither opens a file.

use std::io;
use std::path::{Path, PathBuf};

use argentum_engine::rawler::decoders::supported_extensions;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// The RAW files in a photo's folder, or in `path` when it is a folder,
/// sorted by name.
pub fn folder(path: &Path) -> io::Result<Vec<String>> {
    let path = std::path::absolute(path)?;
    let dir = if path.is_dir() {
        path.as_path()
    } else {
        path.parent().unwrap_or(&path)
    };
    let entries = std::fs::read_dir(dir)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", dir.display())))?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_raw(p))
        .collect();
    found.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    Ok(found
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// The photo the app was started with: the first argument that names a file.
/// Flags, and their values that are not files, are passed over.
pub fn launched_with(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|a| !a.starts_with('-') && Path::new(a).is_file())
}

/// A file the engine's decoder takes, by its extension; hidden files are not.
fn is_raw(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'));
    let raw = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            supported_extensions()
                .iter()
                .any(|s| s.eq_ignore_ascii_case(ext))
        });
    raw && !hidden
}

/// A file dialog for one RAW, in `from`'s folder when given. None when it is
/// closed without a choice.
pub async fn pick(app: &AppHandle, from: Option<String>) -> Option<String> {
    let extensions: Vec<String> = supported_extensions()
        .iter()
        .map(|e| e.to_lowercase())
        .collect();
    let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let mut dialog = app.dialog().file().add_filter("RAW", &extensions);
    if let Some(dir) = from.as_deref().map(Path::new).and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx.await.ok().flatten()?;
    Some(picked.as_path()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_photo_started_with_is_the_first_argument_naming_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a b.CR3");
        std::fs::write(&photo, b"").unwrap();
        let photo = photo.to_string_lossy().into_owned();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            launched_with(args(&["--format", "jpg", &photo])),
            Some(photo.clone())
        );
        assert_eq!(launched_with(args(&["missing.cr3", "--x"])), None);
        assert_eq!(launched_with(args(&[])), None);
    }

    #[test]
    fn a_folder_lists_its_raws_by_name_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "b.NEF",
            "A.cr3",
            "c.jpg",
            ".hidden.CR2",
            "notes.agdata",
            "d.dng",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        std::fs::create_dir(dir.path().join("e.cr2")).unwrap();
        let names = |list: Vec<String>| -> Vec<String> {
            list.iter()
                .map(|p| {
                    Path::new(p)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        };
        let want = ["A.cr3", "b.NEF", "d.dng"];
        assert_eq!(names(folder(&dir.path().join("b.NEF")).unwrap()), want);
        assert_eq!(names(folder(dir.path()).unwrap()), want);
        assert!(folder(&dir.path().join("missing").join("x.cr3")).is_err());
    }
}
