//! Writing a file the user would miss: whole or not at all. Ours.
//!
//! `fs::write` truncates the file first and then writes. A crash, a full disk
//! or a killed process in between leaves a short or empty file: a photo's
//! whole edit, every setting, an XMP another app wrote. So the new bytes go to
//! a temporary file beside the target, reach the disk, and only then replace
//! it, in one rename. Until that rename the old file is untouched.
//!
//! The temporary file and the rename are `tempfile`'s (`NamedTempFile::persist`,
//! already a dependency): it replaces an existing file on Windows too, and
//! deletes the temporary file when anything fails.
//!
//! On Windows a virus scanner, OneDrive or a viewer can hold the file for a
//! moment without letting anyone replace it ("Access is denied"), and a
//! scanner can hold the new file it has just seen written ("being used by
//! another process"). The rename is tried again for about a second before the
//! save gives up, as cargo and npm do for the same reasons.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

const HELD_RETRIES: u32 = 10;

/// Replace `path` with `contents`, or leave it exactly as it was.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let folder = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut temp = tempfile::NamedTempFile::new_in(folder)?;
    temp.write_all(contents.as_ref())?;
    temp.as_file().sync_all()?;
    replace(temp, path)
}

/// `temp` in place of `path`, tried again while another process holds either.
fn replace(mut temp: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match temp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < HELD_RETRIES && is_held(&e.error) => {
                attempt += 1;
                temp = e.file;
                std::thread::sleep(Duration::from_millis(20 * u64::from(attempt)));
            }
            Err(e) => return Err(e.error),
        }
    }
}

/// Windows' `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`: another
/// process has the file open, or part of it locked, for now. Rust has no
/// error kind for them.
const BUSY_OS_ERRORS: [i32; 2] = [32, 33];

/// Whether a rename failed because another process holds one of its files.
/// The codes npm's rename tries again for: graceful-fs 4.2.11, polyfills.js,
/// `EACCES || EPERM || EBUSY`, where libuv turns a sharing or lock violation
/// into `EBUSY`. A held target gives "access denied"; a held new file, busy.
fn is_held(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
        || error
            .raw_os_error()
            .is_some_and(|code| cfg!(windows) && BUSY_OS_ERRORS.contains(&code))
}

#[cfg(test)]
mod tests {
    use super::write_atomic;

    fn names_in(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_file_holds_exactly_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.agdata");
        write_atomic(&path, "{\"rating\":3}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"rating\":3}");
        assert_eq!(names_in(dir.path()), ["photo.agdata"]);
    }

    #[test]
    fn a_longer_file_is_replaced_whole_not_overwritten_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "a much longer old settings file").unwrap();
        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(names_in(dir.path()), ["settings.json"]);
    }

    #[test]
    fn a_write_that_cannot_land_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should go: the final rename must fail.
        let path = dir.path().join("blocked");
        std::fs::create_dir(&path).unwrap();
        assert!(write_atomic(&path, "edit").is_err());
        assert!(path.is_dir(), "what was there is untouched");
        assert_eq!(names_in(dir.path()), ["blocked"], "no temporary file left");
    }

    /// Defender, OneDrive or a viewer can hold the file without letting
    /// anyone replace it, for a moment. The save waits that moment out.
    #[cfg(windows)]
    #[test]
    fn a_file_held_for_a_moment_is_still_replaced() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.agdata");
        std::fs::write(&path, "old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(held);
        });
        write_atomic(&path, "new").unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(names_in(dir.path()), ["photo.agdata"]);
    }

    /// A scanner can also hold the new file, just written, for a moment: it
    /// reads it and lets others read and write, but not rename it away.
    /// (It can't hold it with no sharing at all: the writer's own handle is
    /// open.) The save waits that moment out too.
    #[cfg(windows)]
    #[test]
    fn a_new_file_held_for_a_moment_still_replaces_the_old() {
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.agdata");
        std::fs::write(&path, "old").unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(b"new").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(temp.path())
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(held);
        });
        let replaced = super::replace(temp, &path);
        release.join().unwrap();
        replaced.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(names_in(dir.path()), ["photo.agdata"]);
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone").join("photo.agdata");
        assert!(write_atomic(&path, "edit").is_err());
    }
}
