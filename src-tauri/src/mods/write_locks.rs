//! One writer at a time for the files the library reads, changes and writes
//! back. Ours, after RapidRAW.
//!
//! Each write is atomic (`mods/atomic_write.rs`), so no file is ever left half
//! written; but two read-change-write passes that overlap each read the old
//! file, and the second write drops the first one's change. These locks make
//! the passes take turns:
//!
//! - [`lock_sidecar`] / [`update_sidecar`]: one pass at a time per `.agdata`
//!   (a rating, a tag, the editor's save and its History all write the same
//!   file). Striped: 64 locks shared by path hash, so the lock table stays
//!   bounded however many photos a library has.
//! - [`lock_xmp`]: one XMP write at a time. A RAW and its JPEG share
//!   `IMG.xmp`, and a paste writes them from parallel threads.
//! - [`lock_albums`]: one change at a time to the album file.
//! - [`apply_changes`]: long work runs outside a lock, on a snapshot.
//!
//! Lock order, where two are held: sidecar, then XMP; the album lock is never
//! held with another.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::image_processing::ImageMetadata;

// RapidRAW 922fb901: src-tauri/src/exif_processing.rs (SIDECAR_LOCKS, lock_sidecar)
const SIDECAR_STRIPES: usize = 64;
static SIDECAR_LOCKS: [Mutex<()>; SIDECAR_STRIPES] = [const { Mutex::new(()) }; SIDECAR_STRIPES];

/// Held while one pass reads, changes and writes `sidecar`.
pub fn lock_sidecar(sidecar: &Path) -> MutexGuard<'static, ()> {
    let mut hasher = DefaultHasher::new();
    sidecar.hash(&mut hasher);
    SIDECAR_LOCKS[hasher.finish() as usize % SIDECAR_STRIPES]
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Reads `sidecar`, applies `update` and writes it back, with no other pass on
/// the same sidecar in between. Returns what was written.
// RapidRAW 922fb901: src-tauri/src/exif_processing.rs (update_sidecar)
pub fn update_sidecar(
    sidecar: &Path,
    source: &Path,
    update: impl FnOnce(&mut ImageMetadata),
) -> std::io::Result<ImageMetadata> {
    let _sidecar = lock_sidecar(sidecar);
    let mut metadata = crate::exif_processing::load_sidecar(sidecar);
    update(&mut metadata);
    crate::exif_processing::write_sidecar(sidecar, source, &metadata)?;
    Ok(metadata)
}

/// Writes into `current` the settings that `after` changed from `before`: work too
/// long to hold a sidecar's lock (an analysis of the photo) runs on the edit as
/// read, and only what it changed lands on the edit as it is now. Ours.
pub fn apply_changes(
    current: &mut serde_json::Value,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    let (Some(current), Some(before), Some(after)) = (
        current.as_object_mut(),
        before.as_object(),
        after.as_object(),
    ) else {
        return;
    };
    for (key, value) in after {
        if before.get(key) != Some(value) {
            current.insert(key.clone(), value.clone());
        }
    }
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        current.remove(key);
    }
}

// RapidRAW ad1dc9b9: src-tauri/src/metadata/xmp.rs (XMP_LOCK)
static XMP_LOCK: Mutex<()> = Mutex::new(());

/// Held while one pass reads, changes and writes an XMP.
pub fn lock_xmp() -> MutexGuard<'static, ()> {
    XMP_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

// RapidRAW ad1dc9b9: src-tauri/src/library/albums.rs (ALBUMS_LOCK)
static ALBUMS_LOCK: Mutex<()> = Mutex::new(());

/// Held while one pass reads, changes and writes the album file.
pub fn lock_albums() -> MutexGuard<'static, ()> {
    ALBUMS_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::{sidecar_for, sync_metadata_to_xmp};

    #[test]
    fn work_done_on_a_snapshot_keeps_a_change_made_meanwhile() {
        let before = serde_json::json!({ "exposure": 0.0, "contrast": 0.0, "vignette": 5 });
        let after = serde_json::json!({ "exposure": 0.7, "contrast": 0.0 });
        let mut current = serde_json::json!({ "exposure": 0.0, "contrast": 30, "vignette": 5 });
        apply_changes(&mut current, &before, &after);
        assert_eq!(
            current,
            serde_json::json!({ "exposure": 0.7, "contrast": 30 })
        );
    }

    #[test]
    fn changes_to_one_sidecar_made_at_once_are_all_kept() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("IMG.CR2");
        std::fs::write(&photo, "raw").unwrap();
        let sidecar = sidecar_for(&photo, None);

        std::thread::scope(|scope| {
            for thread in 0..8 {
                let (photo, sidecar) = (&photo, &sidecar);
                scope.spawn(move || {
                    for tag in 0..20 {
                        update_sidecar(sidecar, photo, |metadata| {
                            metadata
                                .tags
                                .get_or_insert_with(Vec::new)
                                .push(format!("{thread}-{tag}"));
                        })
                        .unwrap();
                    }
                });
            }
        });

        let kept = crate::exif_processing::load_sidecar(&sidecar);
        assert_eq!(kept.tags.map(|tags| tags.len()), Some(160));
    }

    #[test]
    fn an_xmp_written_by_a_raw_and_its_jpeg_at_once_stays_whole() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("IMG.CR2");
        let jpeg = dir.path().join("IMG.jpg");
        let xmp = dir.path().join("IMG.xmp");
        for round in 0..30 {
            let _ = std::fs::remove_file(&xmp);
            std::thread::scope(|scope| {
                for thread in 0..8 {
                    let photo = if thread % 2 == 0 { &raw } else { &jpeg };
                    scope.spawn(move || {
                        let metadata = ImageMetadata {
                            rating: 1 + (thread % 5) as u8,
                            tags: Some(vec![format!("t{thread}")]),
                            ..ImageMetadata::default()
                        };
                        sync_metadata_to_xmp(photo, &sidecar_for(photo, None), &metadata, true);
                    });
                }
            });
            let content = std::fs::read_to_string(&xmp).unwrap();
            assert_eq!(
                content.matches("<xmp:Rating>").count(),
                1,
                "round {round}: {content}"
            );
            assert_eq!(
                content.matches("<dc:subject>").count(),
                1,
                "round {round}: {content}"
            );
        }
    }
}
