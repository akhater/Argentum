//! What is in a folder: its folders, for the tree, and its photos, for the
//! grid. Both only read names and one stat per entry; neither opens a file.
//!
//! Photos are handed over a part at a time, so a folder of thousands on a
//! slow disk shows its first photos at once. A newer listing stops an older
//! one at its next entry: changing folder never waits for the last one.

use std::fs::{DirEntry, ReadDir};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rayon::prelude::*;

use super::contract::{BrowseError, BrowseErrorKind, Folder, ListingId, Photo};

/// Photos in one part, at most.
pub const PART_PHOTOS: usize = 256;
/// How long a part waits to fill before it goes anyway.
pub const PART_WAIT: Duration = Duration::from_millis(100);

/// What the editor opens besides camera RAWs, by extension.
const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];

/// A camera RAW the engine's decoder takes, by its extension.
pub fn is_raw(path: &Path) -> bool {
    extension_in(
        path,
        argentum_engine::rawler::decoders::supported_extensions(),
    )
}

/// A photo the grid shows: a RAW, or a JPEG, PNG or TIFF. Hidden files are
/// not.
pub fn is_photo(path: &Path) -> bool {
    !is_hidden_name(path) && (is_raw(path) || extension_in(path, &IMAGE_EXTENSIONS))
}

fn extension_in(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| list.iter().any(|s| s.eq_ignore_ascii_case(ext)))
}

fn is_hidden_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// Hidden by its name, or on Windows by its attributes (`$Recycle.Bin`,
/// `System Volume Information`).
fn is_hidden(entry: &DirEntry) -> bool {
    if is_hidden_name(&entry.path()) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(meta) = entry.metadata() {
            return meta.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    false
}

fn is_folder(entry: &DirEntry) -> bool {
    entry.file_type().is_ok_and(|t| t.is_dir())
}

fn read_dir(dir: &Path) -> Result<ReadDir, BrowseError> {
    std::fs::read_dir(dir).map_err(|e| BrowseError::io(dir, &e))
}

/// Whether `dir` holds a folder that is not hidden. Stops at the first.
fn has_folders(dir: &Path) -> bool {
    read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| is_folder(&e) && !is_hidden(&e))
    })
}

/// A folder as the tree shows it.
pub fn folder(path: &Path) -> Folder {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    Folder {
        name,
        path: path.to_string_lossy().into_owned(),
        has_folders: has_folders(path),
    }
}

/// The folders in `dir`, by name, hidden ones left out.
pub fn folders(dir: &Path) -> Result<Vec<Folder>, BrowseError> {
    let paths: Vec<_> = read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| is_folder(e) && !is_hidden(e))
        .map(|e| e.path())
        .collect();
    // One look inside each, side by side: on a folder of dated folders this
    // is most of the time.
    let mut found: Vec<Folder> = paths.par_iter().map(|p| folder(p)).collect();
    found.sort_by_cached_key(|f| f.name.to_lowercase());
    Ok(found)
}

/// The photo `entry` is, if it is one: one stat (free on Windows, where the
/// directory listing carries it) and the sidecar's existence.
fn photo(entry: &DirEntry) -> Option<Photo> {
    let path = entry.path();
    if !is_photo(&path) {
        return None;
    }
    let meta = entry.metadata().ok().filter(|m| m.is_file())?;
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    Some(Photo {
        name: entry.file_name().to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        bytes: meta.len(),
        modified_ms,
        edited: crate::exif_processing::get_primary_sidecar_path(&path).exists(),
    })
}

/// Hands out listing ids; the latest one is the only one still running.
#[derive(Default)]
pub struct Listings {
    latest: Arc<AtomicU32>,
}

impl Listings {
    /// Open `dir` for listing. Fails at once when it cannot be read; the
    /// photos come from `stream`.
    pub fn start(&self, dir: &Path) -> Result<Listing, BrowseError> {
        if dir.is_file() {
            return Err(BrowseError::new(
                BrowseErrorKind::NotAFolder,
                format!("{} is a file", dir.display()),
            ));
        }
        let entries = read_dir(dir)?;
        let id = ListingId(self.latest.fetch_add(1, Ordering::SeqCst) + 1);
        Ok(Listing {
            id,
            entries,
            latest: self.latest.clone(),
        })
    }
}

/// One folder being listed.
pub struct Listing {
    pub id: ListingId,
    entries: ReadDir,
    latest: Arc<AtomicU32>,
}

impl Listing {
    /// Hand the photos to `part`, `PART_PHOTOS` at a time or whatever came
    /// in `PART_WAIT`, the last part marked done. Stops, without a last part,
    /// once a newer listing has started. Entries that cannot be read are
    /// passed over.
    pub fn stream(self, mut part: impl FnMut(Vec<Photo>, bool)) {
        let Listing {
            id,
            entries,
            latest,
        } = self;
        let is_latest = || latest.load(Ordering::SeqCst) == id.0;
        let mut photos = Vec::new();
        let mut since = Instant::now();
        for entry in entries.filter_map(Result::ok) {
            if !is_latest() {
                return;
            }
            photos.extend(photo(&entry));
            if photos.len() >= PART_PHOTOS || (!photos.is_empty() && since.elapsed() >= PART_WAIT) {
                part(std::mem::take(&mut photos), false);
                since = Instant::now();
            }
        }
        if is_latest() {
            part(photos, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    fn names(photos: &[Photo]) -> Vec<String> {
        let mut n: Vec<_> = photos.iter().map(|p| p.name.clone()).collect();
        n.sort();
        n
    }

    #[test]
    fn photos_are_raws_and_images_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "a.CR3",
            "b.nef",
            "c.JPG",
            "d.png",
            "e.tif",
            ".hidden.CR2",
            "notes.txt",
            "a.CR3.agdata",
            "video.mp4",
        ] {
            touch(dir.path(), name);
        }
        std::fs::create_dir(dir.path().join("f.cr2")).unwrap();
        let listings = Listings::default();
        let mut got = Vec::new();
        listings
            .start(dir.path())
            .unwrap()
            .stream(|part, _| got.extend(part));
        assert_eq!(names(&got), ["a.CR3", "b.nef", "c.JPG", "d.png", "e.tif"]);
    }

    #[test]
    fn a_photo_says_whether_it_has_a_sidecar_and_its_stat() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.CR3");
        touch(dir.path(), "a.CR3.agdata");
        touch(dir.path(), "b.CR3");
        // A virtual copy's sidecar is not the photo's own.
        touch(dir.path(), "b.CR3.1.agdata");
        let mut got = Vec::new();
        Listings::default()
            .start(dir.path())
            .unwrap()
            .stream(|part, _| got.extend(part));
        got.sort_by(|a, b| a.name.cmp(&b.name));
        assert!(got[0].edited);
        assert!(!got[1].edited);
        assert_eq!(got[0].bytes, 1);
        assert!(got[0].modified_ms > 1_600_000_000_000);
        assert!(Path::new(&got[0].path).is_file());
    }

    #[test]
    fn a_big_folder_comes_in_parts_the_last_one_done() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(PART_PHOTOS * 2 + 10) {
            touch(dir.path(), &format!("IMG_{i:04}.CR3"));
        }
        let mut parts = Vec::new();
        Listings::default()
            .start(dir.path())
            .unwrap()
            .stream(|part, done| parts.push((part.len(), done)));
        assert_eq!(
            parts.iter().map(|p| p.0).sum::<usize>(),
            PART_PHOTOS * 2 + 10
        );
        assert!(parts.iter().all(|p| p.0 <= PART_PHOTOS));
        assert_eq!(parts.iter().filter(|p| p.1).count(), 1);
        assert!(parts.last().unwrap().1);
    }

    #[test]
    fn a_newer_listing_stops_an_older_one() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(PART_PHOTOS * 3) {
            touch(dir.path(), &format!("IMG_{i:04}.CR3"));
        }
        let listings = Listings::default();
        let first = listings.start(dir.path()).unwrap();
        let mut parts = 0;
        let mut done = false;
        let mut newer = None;
        first.stream(|_, d| {
            parts += 1;
            done |= d;
            // The user opens another folder while the first is listed.
            newer.get_or_insert_with(|| listings.start(dir.path()).unwrap());
        });
        assert_eq!(
            parts, 1,
            "the older listing went on after a newer one started"
        );
        assert!(!done);
        let newer = newer.unwrap();
        assert_eq!(newer.id, ListingId(2));
        let mut count = 0;
        newer.stream(|p, _| count += p.len());
        assert_eq!(count, PART_PHOTOS * 3);
    }

    #[test]
    fn a_missing_folder_or_a_file_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.CR3");
        let listings = Listings::default();
        let missing = listings.start(&dir.path().join("gone")).err().unwrap();
        assert_eq!(missing.kind, BrowseErrorKind::NotFound);
        let file = listings.start(&dir.path().join("a.CR3")).err().unwrap();
        assert_eq!(file.kind, BrowseErrorKind::NotAFolder);
    }

    #[test]
    fn folders_come_by_name_and_say_whether_they_hold_folders() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b", "A", "c", ".git"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        std::fs::create_dir(dir.path().join("A").join("inner")).unwrap();
        std::fs::create_dir(dir.path().join("c").join(".hidden")).unwrap();
        touch(dir.path(), "file.CR3");
        let got = folders(dir.path()).unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|f| (f.name.as_str(), f.has_folders))
            .collect();
        assert_eq!(shown, [("A", true), ("b", false), ("c", false)]);
        assert_eq!(
            folders(&dir.path().join("gone")).unwrap_err().kind,
            BrowseErrorKind::NotFound
        );
    }
}
