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
//!
//! Some disks refuse the flush to the platter: macOS SMB and NFS shares and
//! some USB or exFAT drives answer "not supported". The save goes ahead there
//! without it (after a plain `fsync` on macOS, whose full flush is the one
//! refused), and says so once in the log. Any other flush error still fails
//! the save.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const HELD_RETRIES: u32 = 10;

/// Replace `path` with `contents`, or leave it exactly as it was.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_atomic_with(path, contents.as_ref(), File::sync_all)
}

/// `write_atomic` for a file other people open: a photo the user exports.
/// The new file gets the permissions any newly created file gets (the
/// umask's), not the owner-only ones of a temporary file.
pub fn write_atomic_shared(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_through(path, contents.as_ref(), File::sync_all, true)
}

/// `write_atomic` with the full flush passed in, so a test can make the disk
/// refuse it.
fn write_atomic_with(
    path: &Path,
    contents: &[u8],
    full_flush: impl FnOnce(&File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    write_through(path, contents, full_flush, false)
}

fn write_through(
    path: &Path,
    contents: &[u8],
    full_flush: impl FnOnce(&File) -> std::io::Result<()>,
    shared: bool,
) -> std::io::Result<()> {
    let folder = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    if shared {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    #[cfg(not(unix))]
    let _ = shared;
    let mut temp = builder.tempfile_in(folder)?;
    temp.write_all(contents)?;
    flush(temp.as_file(), full_flush)?;
    replace(temp, path)
}

// RapidRAW 526d0f38: src-tauri/src/file_management.rs
/// `file` flushed to the disk, or as far as the disk allows: a flush it
/// refuses as unsupported is skipped, not an error.
fn flush(
    file: &File,
    full_flush: impl FnOnce(&File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match full_flush(file) {
        Err(e) if is_flush_unsupported(&e) => {
            // macOS' full flush is `F_FULLFSYNC`; a disk that refuses it may
            // still take a plain `fsync`.
            #[cfg(target_vendor = "apple")]
            {
                use std::os::fd::AsRawFd;
                // SAFETY: the descriptor belongs to `file`, open for this call.
                if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
                    let e = std::io::Error::last_os_error();
                    if !is_flush_unsupported(&e) {
                        return Err(e);
                    }
                }
            }
            if !FLUSH_REFUSAL_LOGGED.swap(true, Ordering::Relaxed) {
                log::warn!("a disk refused a full flush ({e}); saving without it");
            }
            Ok(())
        }
        result => result,
    }
}

static FLUSH_REFUSAL_LOGGED: AtomicBool = AtomicBool::new(false);

/// The errors a disk answers when it cannot flush at all: `ENOTSUP` and
/// `EOPNOTSUPP` (the same number on Linux, two on macOS).
#[cfg(unix)]
const FLUSH_UNSUPPORTED: [i32; 2] = [libc::ENOTSUP, libc::EOPNOTSUPP];
#[cfg(not(unix))]
const FLUSH_UNSUPPORTED: [i32; 0] = [];

// RapidRAW 526d0f38: src-tauri/src/file_management.rs
fn is_flush_unsupported(error: &std::io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|code| FLUSH_UNSUPPORTED.contains(&code))
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
    use super::{write_atomic, write_atomic_shared};

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

    /// macOS SMB and NFS shares and some USB or exFAT drives refuse the full
    /// flush as unsupported. The edit is saved anyway.
    #[cfg(unix)]
    #[test]
    fn a_disk_that_refuses_the_flush_still_saves() {
        for code in [libc::ENOTSUP, libc::EOPNOTSUPP] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("photo.agdata");
            std::fs::write(&path, "old").unwrap();
            super::write_atomic_with(&path, b"new", |_| {
                Err(std::io::Error::from_raw_os_error(code))
            })
            .unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
            assert_eq!(names_in(dir.path()), ["photo.agdata"]);
        }
    }

    /// Only "not supported" is let through: a disk that fails the flush any
    /// other way fails the save, and the old file stays.
    #[test]
    fn any_other_flush_error_still_fails_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.agdata");
        std::fs::write(&path, "old").unwrap();
        let failed = super::write_atomic_with(&path, b"new", |_| {
            Err(std::io::Error::other("the disk failed"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(
            names_in(dir.path()),
            ["photo.agdata"],
            "no temporary file left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_input_output_error_from_the_flush_still_fails_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.agdata");
        std::fs::write(&path, "old").unwrap();
        let failed = super::write_atomic_with(&path, b"new", |_| {
            Err(std::io::Error::from_raw_os_error(libc::EIO))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn a_shared_write_replaces_whole_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG.jpg");
        std::fs::write(&path, "a much longer earlier export").unwrap();
        write_atomic_shared(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(names_in(dir.path()), ["IMG.jpg"]);
        // Blocked by a folder of that name: fails, leaves no temporary file.
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_atomic_shared(&blocked, "x").is_err());
        assert_eq!(names_in(dir.path()), ["IMG.jpg", "blocked"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_write_gets_the_permissions_of_a_new_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("control");
        std::fs::write(&control, "x").unwrap();
        let path = dir.path().join("IMG.jpg");
        write_atomic_shared(&path, "x").unwrap();
        assert_eq!(
            path.metadata().unwrap().permissions().mode() & 0o777,
            control.metadata().unwrap().permissions().mode() & 0o777,
        );
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone").join("photo.agdata");
        assert!(write_atomic(&path, "edit").is_err());
    }
}
