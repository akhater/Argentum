//! The mark that says GPU setup is under way, and what a leftover one means. Ours.
//!
//! A bad driver can take the whole process down during setup, with no error to
//! catch. So a file is written before setup and removed after it, and finding
//! it at the next launch means setup never finished. `lib.rs` then moves the
//! processing backend to OpenGL.
//!
//! WHAT A LEFTOVER MARK CAN SAY
//!
//! Only that setup did not finish. A driver crash, a hang and a forced stop all
//! leave the same file, so `fallback_note` never calls it a crash.
//!
//! Setup also runs again after a lost device, in a process that has already
//! shown the backend works. Stopping the app there says nothing about the
//! backend, so `begin` only marks the first setup of a process.
//!
//! WHY THE FALLBACK HAS ITS OWN RECORD
//!
//! `app.log` is emptied at every launch, so a line there is gone by the launch
//! after the fallback, long before anyone wonders why the setting changed.
//! `keep_record` appends to a file of its own that nothing empties.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

const RECORD_FILE: &str = "gpu-fallback.log";

#[derive(Default)]
pub struct GpuSetupMark {
    path: OnceLock<PathBuf>,
    backend_worked: AtomicBool,
}

impl GpuSetupMark {
    pub fn set_path(&self, path: PathBuf) {
        let _ = self.path.set(path);
    }

    /// Call before setup starts.
    pub fn begin(&self) {
        if self.backend_worked.load(Ordering::Acquire) {
            return;
        }
        if let Some(path) = self.path.get() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, "initializing_gpu");
        }
    }

    /// Call when setup has produced a working device.
    pub fn finish(&self) {
        self.backend_worked.store(true, Ordering::Release);
        self.clear();
    }

    /// Whether the last run left its mark behind.
    pub fn left_over(&self) -> bool {
        self.path.get().is_some_and(|path| path.exists())
    }

    pub fn clear(&self) {
        if let Some(path) = self.path.get() {
            let _ = fs::remove_file(path);
        }
    }
}

/// What happened, for the log and the record. `backend` is the saved setting
/// before the fallback changed it.
pub fn fallback_note(backend: Option<&str>) -> String {
    format!(
        "GPU setup did not finish on the last run (a driver crash, a hang or a forced stop, \
         which look the same from here). Processing backend changed from {} to gl.",
        backend.unwrap_or("auto")
    )
}

/// Add `note` to the fallback record in `log_dir`.
pub fn keep_record(log_dir: &Path, note: &str) -> io::Result<()> {
    fs::create_dir_all(log_dir)?;
    let mut record = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(RECORD_FILE))?;
    writeln!(
        record,
        "{} {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        note
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(path: &Path) -> GpuSetupMark {
        let mark = GpuSetupMark::default();
        mark.set_path(path.to_path_buf());
        mark
    }

    #[test]
    fn stopping_the_app_during_recovery_leaves_no_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mark");

        let run = launch(&path);
        run.begin();
        run.finish();
        // The device is lost, setup runs again, and the app is stopped mid-way.
        run.begin();

        assert!(!launch(&path).left_over());
    }

    #[test]
    fn a_first_setup_that_never_ended_leaves_the_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mark");

        launch(&path).begin();

        assert!(launch(&path).left_over());
    }

    #[test]
    fn a_setup_that_returned_an_error_leaves_no_mark_and_the_retry_is_marked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mark");

        let run = launch(&path);
        run.begin();
        run.clear();
        assert!(!launch(&path).left_over());

        // An error proved nothing about the backend, so the retry still counts.
        run.begin();
        assert!(launch(&path).left_over());
    }

    #[test]
    fn the_note_names_the_old_backend_and_does_not_claim_a_crash_happened() {
        let vulkan = fallback_note(Some("vulkan"));
        assert!(vulkan.contains("from vulkan to gl"));
        assert!(!vulkan.contains("crash detected"));
        assert!(fallback_note(None).contains("from auto to gl"));
    }

    #[test]
    fn the_record_keeps_every_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");

        keep_record(&log_dir, "first").unwrap();
        keep_record(&log_dir, "second").unwrap();

        let record = fs::read_to_string(log_dir.join(RECORD_FILE)).unwrap();
        assert_eq!(record.lines().count(), 2);
        assert!(record.contains("first") && record.contains("second"));
    }
}
