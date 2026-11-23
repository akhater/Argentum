//! Moving a photo with its files from one folder to another. Ours.
//!
//! The files that hold the user's work (edit sidecars, the EXIF cache, the XMP,
//! look files) reach the destination first and the photo last. A photo is
//! therefore either still in its folder with everything, or in the new one with
//! its files beside it. If any file of a photo fails, what already arrived goes
//! back (or, for a copy, is deleted) and the photo stays whole where it was.
//!
//! A rename stays a rename. When it fails (another drive) the file is copied
//! under a temporary name in the destination folder and renamed to its real name
//! once complete, and the originals are deleted only when every file of that
//! photo is at the destination. A failed copy leaves no partial file.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The three disk steps a move is made of, so a test can make one fail.
pub(crate) trait Disk {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn remove(&self, file: &Path) -> io::Result<()>;
}

pub(crate) struct RealDisk;

impl Disk for RealDisk {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
        fs::copy(from, to)
    }
    fn remove(&self, file: &Path) -> io::Result<()> {
        fs::remove_file(file)
    }
}

/// A file, where it goes, and whether it stays where it is too (an XMP
/// another photo still uses).
pub(crate) type Step = (PathBuf, PathBuf, bool);

/// One photo and every file that travels with it, the photo included.
pub(crate) struct PhotoMove {
    pub(crate) photo: PathBuf,
    pub(crate) files: Vec<Step>,
}

/// A move worked out before any file is touched: each photo's files and
/// destinations, and each photo's new path.
pub(crate) struct MovePlan {
    pub(crate) photos: Vec<PhotoMove>,
    pub(crate) renames: HashMap<String, String>,
}

impl MovePlan {
    /// Moves the photos one after the other and stops at the first that
    /// fails, that photo being left whole where it was. Returns the photos
    /// that did move, old path to new, whether or not it stopped.
    pub(crate) fn carry_out(self) -> (HashMap<String, String>, Result<(), String>) {
        self.carry_out_on(&RealDisk)
    }

    pub(crate) fn carry_out_on(
        self,
        disk: &dyn Disk,
    ) -> (HashMap<String, String>, Result<(), String>) {
        let mut renames = self.renames;
        let mut moved = Vec::new();
        let mut result = Ok(());
        for photo in &self.photos {
            if let Err(e) = move_photo(disk, photo) {
                result = Err(e);
                break;
            }
            moved.push(photo.photo.to_string_lossy().into_owned());
        }
        renames.retain(|from, _| moved.contains(from));
        (renames, result)
    }
}

/// What a file did on its way, so it can be undone.
enum Arrived {
    /// Renamed: it is only at the destination.
    Renamed { from: PathBuf, to: PathBuf },
    /// Copied: it is at both until the move is complete.
    Copied { from: PathBuf, to: PathBuf },
    /// Copied to stay: the original is never removed.
    Kept { to: PathBuf },
}

fn move_photo(disk: &dyn Disk, photo: &PhotoMove) -> Result<(), String> {
    // No save of this photo's edits (or of a virtual copy's) in between: one
    // made while the photo is still being copied would be deleted with the
    // original, or overwritten by an undo. Held until the originals are gone
    // or the undo is done.
    let _edits = crate::mods::write_locks::lock_sidecars(
        photo
            .files
            .iter()
            .map(|(from, _, _)| from.as_path())
            .filter(|from| from.extension().is_some_and(|e| e == "agdata")),
    );
    // A partial file an earlier, killed move left is ours: copy_whole would
    // overwrite it, but it is not left behind if this move renames instead.
    for (_, to, _) in &photo.files {
        let _ = fs::remove_file(partial_name(to));
    }
    // Everything but the photo, then the photo.
    let own = photo
        .files
        .iter()
        .filter(|(from, _, _)| *from != photo.photo);
    let last = photo
        .files
        .iter()
        .filter(|(from, _, _)| *from == photo.photo);
    let mut arrived = Vec::new();
    for (from, to, keep_source) in own.chain(last) {
        match arrive(disk, from, to, *keep_source) {
            Ok(step) => arrived.push(step),
            Err(e) => return Err(undo(disk, arrived, e)),
        }
    }
    // Every file is complete at the destination: the originals can go, the
    // photo's last.
    for step in arrived {
        if let Arrived::Copied { from, .. } = step
            && let Err(e) = disk.remove(&from)
        {
            log::warn!(
                "Moved file successfully, but failed to delete original {}: {}",
                from.display(),
                e
            );
        }
    }
    Ok(())
}

fn arrive(disk: &dyn Disk, from: &Path, to: &Path, keep_source: bool) -> Result<Arrived, String> {
    if keep_source {
        copy_whole(disk, from, to)?;
        return Ok(Arrived::Kept {
            to: to.to_path_buf(),
        });
    }
    if disk.rename(from, to).is_ok() {
        return Ok(Arrived::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }
    copy_whole(disk, from, to)?;
    Ok(Arrived::Copied {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
    })
}

/// The temporary name a copy to `to` is written under.
fn partial_name(to: &Path) -> PathBuf {
    let mut name: OsString = to.file_name().unwrap_or_default().to_os_string();
    name.push(".argentum-partial");
    to.with_file_name(name)
}

/// A copy that reaches `to` only when complete: written under a temporary
/// name beside it, then renamed. A failure leaves nothing behind.
fn copy_whole(disk: &dyn Disk, from: &Path, to: &Path) -> Result<(), String> {
    let partial = partial_name(to);
    let done = disk
        .copy(from, &partial)
        .map_err(|e| format!("Move failed during copy for {}: {}", from.display(), e))
        .and_then(|_| {
            disk.rename(&partial, to)
                .map_err(|e| format!("Move failed during copy for {}: {}", from.display(), e))
        });
    if done.is_err() {
        let _ = disk.remove(&partial);
    }
    done
}

/// Puts back what arrived, last first. Returns the error to report: the
/// failure itself, and any file that could not be put back.
fn undo(disk: &dyn Disk, arrived: Vec<Arrived>, error: String) -> String {
    let mut stuck = Vec::new();
    for step in arrived.into_iter().rev() {
        let (undone, at) = match step {
            Arrived::Renamed { from, to } => {
                let back = disk.rename(&to, &from).or_else(|_| {
                    copy_whole(disk, &to, &from)
                        .map_err(io::Error::other)
                        .and_then(|()| disk.remove(&to))
                });
                (back, to)
            }
            Arrived::Copied { to, .. } | Arrived::Kept { to } => (disk.remove(&to), to),
        };
        if undone.is_err() {
            stuck.push(at.display().to_string());
        }
    }
    if stuck.is_empty() {
        error
    } else {
        format!("{error} (could not put back: {})", stuck.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    type CopyHook = Box<dyn Fn()>;

    /// What goes wrong with one named file of the photo.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Fault {
        /// Renaming it and copying it both fail; a copy stops midway.
        Locked,
        /// Its copy is complete but cannot take its real name.
        NoFinalName,
    }

    /// The real disk, except that two folders can be "other drives" (a rename
    /// between them fails) and one file can fail. Remembers what it did.
    struct Flaky {
        other_drive: bool,
        fault: Option<(&'static str, Fault)>,
        renamed: RefCell<Vec<String>>,
        removed: RefCell<Vec<String>>,
        /// Nothing can be renamed from folder `b` back into `a`.
        no_rename_back: bool,
        /// Nothing can be copied from folder `b` back into `a`.
        no_copy_back: bool,
        /// Called before the copy of the file of this name starts.
        on_copy: Option<(&'static str, CopyHook)>,
    }

    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    fn back_to_a(from: &Path, to: &Path) -> bool {
        from.parent()
            .and_then(Path::file_name)
            .is_some_and(|f| f == "b")
            && to
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|f| f == "a")
    }

    impl Flaky {
        fn new(other_drive: bool, fault: Option<(&'static str, Fault)>) -> Self {
            Flaky {
                other_drive,
                fault,
                renamed: RefCell::new(Vec::new()),
                removed: RefCell::new(Vec::new()),
                no_rename_back: false,
                no_copy_back: false,
                on_copy: None,
            }
        }
    }

    impl Disk for Flaky {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let from_name = name_of(from);
            if self.no_rename_back && back_to_a(from, to) {
                return Err(io::Error::other("cannot go back"));
            }
            if let Some((file, Fault::NoFinalName)) = self.fault
                && from_name == format!("{file}.argentum-partial")
            {
                return Err(io::Error::other("no final name"));
            }
            if let Some((file, Fault::Locked)) = self.fault
                && from_name == file
                && from.parent() != to.parent()
            {
                return Err(io::Error::other("locked"));
            }
            if self.other_drive && from.parent() != to.parent() {
                return Err(io::Error::other("different drive"));
            }
            self.renamed.borrow_mut().push(name_of(to));
            fs::rename(from, to)
        }
        fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
            if self.no_copy_back && back_to_a(from, to) {
                return Err(io::Error::other("cannot copy back"));
            }
            if let Some((file, call)) = &self.on_copy
                && name_of(from) == *file
            {
                call();
            }
            if let Some((file, Fault::Locked)) = self.fault
                && name_of(from) == file
                && from.parent() != to.parent()
            {
                // The disk fills up: half of the file is written.
                let bytes = fs::read(from)?;
                fs::write(to, &bytes[..bytes.len() / 2])?;
                return Err(io::Error::other("disk full"));
            }
            fs::copy(from, to)
        }
        fn remove(&self, file: &Path) -> io::Result<()> {
            self.removed.borrow_mut().push(name_of(file));
            fs::remove_file(file)
        }
    }

    const FILES: [&str; 5] = [
        "IMG.CR2",
        "IMG.CR2.agdata",
        "IMG.CR2.agexif",
        "IMG.CR2.ab12cd.agdata",
        "IMG.xmp",
    ];

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Every file of `folder`, name to content.
    fn contents(folder: &Path) -> BTreeMap<String, String> {
        match fs::read_dir(folder) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|e| (name_of(&e.path()), fs::read_to_string(e.path()).unwrap()))
                .collect(),
            Err(_) => BTreeMap::new(),
        }
    }

    /// The plan to move `names` (the photo first) from `a` to an empty `b`.
    fn setup(dir: &Path, names: &[&str]) -> MovePlan {
        let (source, target) = (dir.join("a"), dir.join("b"));
        fs::create_dir_all(&target).unwrap();
        let mut files = Vec::new();
        for name in names {
            write(&source.join(name), &format!("content of {name}"));
            files.push((source.join(name), target.join(name), false));
        }
        let photo = source.join(names[0]);
        let renames = HashMap::from([(
            photo.to_string_lossy().into_owned(),
            target.join(names[0]).to_string_lossy().into_owned(),
        )]);
        MovePlan {
            photos: vec![PhotoMove { photo, files }],
            renames,
        }
    }

    fn original(names: &[&str]) -> BTreeMap<String, String> {
        names
            .iter()
            .map(|n| (n.to_string(), format!("content of {n}")))
            .collect()
    }

    #[test]
    fn a_move_that_works_leaves_every_file_at_the_destination_only() {
        for other_drive in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let plan = setup(dir.path(), &FILES);
            let (renames, result) = plan.carry_out_on(&Flaky::new(other_drive, None));

            assert!(result.is_ok());
            assert_eq!(renames.len(), 1);
            assert_eq!(contents(&dir.path().join("b")), original(&FILES));
            assert!(contents(&dir.path().join("a")).is_empty());
        }
    }

    #[test]
    fn the_photo_is_renamed_after_every_file_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Flaky::new(false, None);
        setup(dir.path(), &FILES).carry_out_on(&disk).1.unwrap();

        let renamed = disk.renamed.borrow();
        assert_eq!(renamed.len(), FILES.len());
        assert_eq!(renamed.last().unwrap(), "IMG.CR2");
    }

    #[test]
    fn across_drives_the_originals_go_only_when_every_copy_is_complete_photo_last() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Flaky::new(true, None);
        setup(dir.path(), &FILES).carry_out_on(&disk).1.unwrap();

        // Every copy was written under a temporary name and renamed once.
        let renamed = disk.renamed.borrow();
        assert_eq!(renamed.len(), FILES.len());
        assert_eq!(renamed.last().unwrap(), "IMG.CR2");
        // The originals were removed after that, the photo's last.
        let removed = disk.removed.borrow();
        assert_eq!(removed.len(), FILES.len());
        assert_eq!(removed.last().unwrap(), "IMG.CR2");
    }

    #[test]
    fn a_failure_at_any_file_leaves_the_photo_whole_in_its_folder() {
        for other_drive in [false, true] {
            for fault in [Fault::Locked, Fault::NoFinalName] {
                for failing in FILES {
                    let dir = tempfile::tempdir().unwrap();
                    let plan = setup(dir.path(), &FILES);
                    let disk = Flaky::new(other_drive, Some((failing, fault)));
                    let (renames, result) = plan.carry_out_on(&disk);
                    let here = format!("{failing} {fault:?} other_drive={other_drive}");

                    let (source, target) = (dir.path().join("a"), dir.path().join("b"));
                    if fault == Fault::NoFinalName && !other_drive {
                        // Renames work, nothing is copied, nothing can fail.
                        assert!(result.is_ok(), "{here}");
                        assert_eq!(contents(&target), original(&FILES), "{here}");
                        continue;
                    }
                    assert!(result.is_err(), "{here}");
                    assert!(renames.is_empty(), "{here}");
                    assert_eq!(contents(&source), original(&FILES), "{here}");
                    assert!(contents(&target).is_empty(), "{here}");
                }
            }
        }
    }

    #[test]
    fn a_failing_photo_leaves_the_photos_before_it_moved_whole() {
        let dir = tempfile::tempdir().unwrap();
        let first = setup(dir.path(), &["a.CR2", "a.CR2.agdata"]);
        let second = setup(dir.path(), &["b.CR2", "b.CR2.agdata"]);
        let plan = MovePlan {
            photos: first.photos.into_iter().chain(second.photos).collect(),
            renames: first.renames.into_iter().chain(second.renames).collect(),
        };
        let disk = Flaky::new(true, Some(("b.CR2.agdata", Fault::Locked)));
        let (renames, result) = plan.carry_out_on(&disk);

        assert!(result.is_err());
        assert_eq!(renames.len(), 1);
        assert_eq!(
            contents(&dir.path().join("b")),
            original(&["a.CR2", "a.CR2.agdata"])
        );
        assert_eq!(
            contents(&dir.path().join("a")),
            original(&["b.CR2", "b.CR2.agdata"])
        );
    }

    #[test]
    fn an_xmp_that_stays_behind_is_copied_and_the_copy_undone_with_the_photo() {
        for failing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut plan = setup(dir.path(), &["IMG.CR2", "IMG.xmp"]);
            plan.photos[0].files[1].2 = true;
            let fault = failing.then_some(("IMG.CR2", Fault::Locked));
            let (_, result) = plan.carry_out_on(&Flaky::new(true, fault));

            let (source, target) = (dir.path().join("a"), dir.path().join("b"));
            if failing {
                assert!(result.is_err());
                assert_eq!(contents(&source), original(&["IMG.CR2", "IMG.xmp"]));
                assert!(contents(&target).is_empty());
            } else {
                assert!(result.is_ok());
                assert_eq!(contents(&target), original(&["IMG.CR2", "IMG.xmp"]));
                assert_eq!(contents(&source), original(&["IMG.xmp"]));
            }
        }
    }

    #[test]
    fn a_file_that_cannot_be_renamed_back_is_copied_back() {
        let dir = tempfile::tempdir().unwrap();
        let plan = setup(dir.path(), &FILES);
        let mut disk = Flaky::new(false, Some(("IMG.CR2", Fault::Locked)));
        disk.no_rename_back = true;
        let (_, result) = plan.carry_out_on(&disk);

        assert!(result.is_err());
        assert_eq!(contents(&dir.path().join("a")), original(&FILES));
        assert!(contents(&dir.path().join("b")).is_empty());
    }

    #[test]
    fn a_file_that_cannot_be_put_back_is_named_and_kept_at_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let plan = setup(dir.path(), &FILES);
        let mut disk = Flaky::new(false, Some(("IMG.CR2", Fault::Locked)));
        disk.no_rename_back = true;
        disk.no_copy_back = true;
        let (renames, result) = plan.carry_out_on(&disk);

        let error = result.unwrap_err();
        assert!(error.contains("could not put back"), "{error}");
        assert!(error.contains("IMG.CR2.agdata"), "{error}");
        assert!(renames.is_empty());
        // Nothing is deleted: the edits wait at the destination, the photo at home.
        assert_eq!(contents(&dir.path().join("a")), original(&["IMG.CR2"]));
        assert_eq!(contents(&dir.path().join("b")), original(&FILES[1..]));
    }

    #[test]
    fn a_partial_file_an_earlier_move_left_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let plan = setup(dir.path(), &FILES);
        write(
            &dir.path().join("b").join("IMG.CR2.agdata.argentum-partial"),
            "half",
        );
        plan.carry_out_on(&Flaky::new(false, None)).1.unwrap();

        assert_eq!(contents(&dir.path().join("b")), original(&FILES));
    }

    #[test]
    fn a_save_during_a_move_waits_for_it_and_is_not_lost() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, mpsc};
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let plan = setup(dir.path(), &["IMG.CR2", "IMG.CR2.agdata"]);
        let sidecar = dir.path().join("a").join("IMG.CR2.agdata");
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let mover = std::thread::spawn(move || {
            let mut disk = Flaky::new(true, None);
            disk.on_copy = Some((
                "IMG.CR2",
                Box::new(move || {
                    started_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                }),
            ));
            plan.carry_out_on(&disk).1
        });
        // The sidecar is copied; the photo is being copied.
        started_rx.recv().unwrap();

        let saved = Arc::new(AtomicBool::new(false));
        let saver = {
            let (saved, sidecar) = (saved.clone(), sidecar.clone());
            std::thread::spawn(move || {
                let _lock = crate::mods::write_locks::lock_sidecar(&sidecar);
                saved.store(true, Ordering::SeqCst);
                fs::write(&sidecar, "saved").unwrap();
            })
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(!saved.load(Ordering::SeqCst), "the save did not wait");

        go_tx.send(()).unwrap();
        mover.join().unwrap().unwrap();
        saver.join().unwrap();

        assert!(saved.load(Ordering::SeqCst));
        // The move took the edit as it was, and the save after it was not deleted.
        assert_eq!(
            read_in(&dir.path().join("b"), "IMG.CR2.agdata"),
            "content of IMG.CR2.agdata"
        );
        assert_eq!(read_in(&dir.path().join("a"), "IMG.CR2.agdata"), "saved");
    }

    fn read_in(folder: &Path, name: &str) -> String {
        fs::read_to_string(folder.join(name)).unwrap()
    }
}
