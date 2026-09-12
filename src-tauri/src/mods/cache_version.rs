//! Throw away cached thumbnails when our decode changes.
//!
//! THE PROBLEM
//!
//! Thumbnails are cached to disk under a hash of the photo's path, its
//! modification time and the adjustments applied to it — see
//! `compute_thumbnail_cache_hash` in `file_management.rs`. Nothing in that hash
//! describes *how the photo was decoded*.
//!
//! That is fine for RapidRAW, whose decode never changes. It is wrong for us:
//! every fix in `mods/` changes what a correct thumbnail looks like, and the
//! hash does not move, so the app finds the old file and never regenerates it.
//! The library keeps showing pictures rendered by code we have already deleted,
//! while the editor shows the new rendering — the same photo, two colours,
//! depending on where you look at it.
//!
//! It cost real time: the sRAW levels fix landed and the library still showed
//! the green cast, which reads exactly like the fix not working.
//!
//! WHY NOT JUST DELETE THEM BY HAND
//!
//! Because remembering is not a mechanism. It has to happen because the code
//! changed, not because someone thought of it.
//!
//! WHY NOT PUT THE VERSION IN THE HASH
//!
//! That is the better fix and it is one line — in `file_management.rs`, which
//! is at 30 of its 30 approved lines, all of them the `.agdata` rename. The
//! rule is that a budget is never raised, so this does the same job from our
//! own side: same effect, nothing of theirs touched.
//!
//! Deleting the files is enough on its own. The thumbnails are served over
//! `http://asset.localhost/...` with no cache headers, and the webview was
//! measured re-reading a file whose bytes had changed underneath it, so there
//! is no second copy hiding in the webview to worry about.

use std::path::{Path, PathBuf};

/// Bump this whenever a change in `mods/` alters how a photo renders.
///
/// Not the app version: this tracks the *pixels*, and most releases do not
/// change them. A new number means every cached thumbnail is regenerated once,
/// on the next start.
///
/// 1. sRAW black and white levels (`mods/sraw_levels.rs`), and the removal of
///    the D50/D65 matrix correction that had been compensating for them.
pub const PIPELINE: u32 = 2;

/// Name of the stamp left beside the thumbnails recording what made them.
const STAMP: &str = "argentum-pipeline";

/// Clear the thumbnail cache if it was written by a different decode.
///
/// Takes the app cache directory, which is where `resolve_thumbnail_cache_dir`
/// puts `thumbnails/`. Silent and best-effort: a cache that cannot be read or
/// cleared is not a reason to stop the app starting, it just means some
/// thumbnails stay stale until the user regenerates them.
pub fn clear_thumbnails_if_pipeline_changed(cache_dir: &Path) {
    let stamp = cache_dir.join(STAMP);
    let current = PIPELINE.to_string();

    if std::fs::read_to_string(&stamp).is_ok_and(|found| found.trim() == current) {
        return;
    }

    let thumbnails = cache_dir.join("thumbnails");
    let removed = remove_cached_images(&thumbnails);
    if removed > 0 {
        log::info!("decode pipeline is now v{PIPELINE}; dropped {removed} stale thumbnails");
    }

    let _ = std::fs::create_dir_all(cache_dir);
    let _ = std::fs::write(&stamp, current);
}

/// Delete the cached images, and only those.
///
/// Deliberately not `remove_dir_all` on a path assembled from a handle: this
/// removes files it recognises and leaves anything else alone, so a wrong path
/// or a future neighbour file cannot turn into data loss.
fn remove_cached_images(thumbnails: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(thumbnails) else {
        return 0;
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if is_cached_thumbnail(&path) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A file this cache wrote: `<hash>_small.jpg` or `<hash>_medium.jpg`.
fn is_cached_thumbnail(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with("_small.jpg") || name.ends_with("_medium.jpg")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-cache-version-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("thumbnails")).expect("scratch dir");
        dir
    }

    fn touch(path: &Path, name: &str) {
        std::fs::write(path.join(name), b"x").expect("write");
    }

    /// A cache with no stamp is from before this existed, so it must go.
    #[test]
    fn an_unstamped_cache_is_cleared() {
        let dir = scratch("unstamped");
        touch(&dir.join("thumbnails"), "abc_small.jpg");
        touch(&dir.join("thumbnails"), "abc_medium.jpg");

        clear_thumbnails_if_pipeline_changed(&dir);

        assert!(!dir.join("thumbnails/abc_small.jpg").exists());
        assert!(!dir.join("thumbnails/abc_medium.jpg").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join(STAMP)).unwrap(),
            PIPELINE.to_string()
        );
    }

    /// Once stamped, a second start must leave the cache alone — otherwise
    /// every launch throws away work.
    #[test]
    fn a_matching_stamp_keeps_the_cache() {
        let dir = scratch("matching");
        std::fs::write(dir.join(STAMP), PIPELINE.to_string()).expect("stamp");
        touch(&dir.join("thumbnails"), "abc_small.jpg");

        clear_thumbnails_if_pipeline_changed(&dir);

        assert!(dir.join("thumbnails/abc_small.jpg").exists());
    }

    /// An older stamp means the decode moved on.
    #[test]
    fn an_older_stamp_clears_the_cache() {
        let dir = scratch("older");
        std::fs::write(dir.join(STAMP), "0").expect("stamp");
        touch(&dir.join("thumbnails"), "abc_medium.jpg");

        clear_thumbnails_if_pipeline_changed(&dir);

        assert!(!dir.join("thumbnails/abc_medium.jpg").exists());
    }

    /// Nothing but thumbnails is touched, whatever else lives in there.
    #[test]
    fn leaves_everything_else_alone() {
        let dir = scratch("bystanders");
        let thumbs = dir.join("thumbnails");
        touch(&thumbs, "abc_small.jpg");
        touch(&thumbs, "notes.txt");
        touch(&thumbs, "photo.jpg");
        std::fs::create_dir_all(thumbs.join("subdir")).expect("subdir");

        clear_thumbnails_if_pipeline_changed(&dir);

        assert!(!thumbs.join("abc_small.jpg").exists());
        assert!(thumbs.join("notes.txt").exists(), "unrelated file removed");
        assert!(
            thumbs.join("photo.jpg").exists(),
            "a real photo was removed"
        );
        assert!(thumbs.join("subdir").is_dir(), "a directory was removed");
    }

    /// A cache directory that does not exist yet is not an error, and still
    /// gets stamped so the next start is a no-op.
    #[test]
    fn a_missing_cache_is_fine() {
        let dir = std::env::temp_dir().join("argentum-cache-version-missing");
        let _ = std::fs::remove_dir_all(&dir);

        clear_thumbnails_if_pipeline_changed(&dir);

        assert_eq!(
            std::fs::read_to_string(dir.join(STAMP)).unwrap(),
            PIPELINE.to_string()
        );
    }
}
