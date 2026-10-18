//! Finding photos for the new interface beyond browsing (`browse/`): the
//! photo the app was started with, and a file dialog for one. Neither opens
//! a file.

use std::path::Path;

use argentum_engine::rawler::decoders::supported_extensions;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// The photo the app was started with: the first argument that names a file.
/// Flags, and their values that are not files, are passed over.
pub fn launched_with(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|a| !a.starts_with('-') && Path::new(a).is_file())
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
}
