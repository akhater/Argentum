//! The mark that says GPU setup is under way, and what a leftover one means. Ours.
//!
//! A bad driver can take the whole process down during setup, with no error to
//! catch. So a file naming the backend is written before setup and removed
//! after it, and finding it at the next launch means setup never finished.
//!
//! WHAT A LEFTOVER MARK CAN SAY
//!
//! Only that setup did not finish. A driver crash, a hang and a forced stop all
//! leave the same file, so the note never calls it a crash.
//!
//! Setup also runs again after a lost device, in a process that has already
//! shown the backend works. Stopping the app there says nothing about the
//! backend, so `begin` only marks the first setup of a process.
//!
//! WHAT THE NEXT LAUNCH DOES
//!
//! Once proves little, so that launch runs on OpenGL and the saved setting
//! stays. The launch after it tries the saved backend again, and only a second
//! unfinished setup in a row on that backend saves `gl`.
//!
//! The first unfinished setup is remembered in a file beside the mark, and it
//! has to outlive the OpenGL launch. That launch's setup finishes, but it says
//! nothing about the saved backend. If it cleared the count, a driver that
//! really does crash would take the app down on every second launch for ever.
//! So a finished setup only clears the count of its own backend.
//!
//! A saved backend that differs from the mark or the count was changed by hand
//! since, and what was learned about the old one does not apply to it.
//!
//! WHY THE FALLBACK HAS ITS OWN RECORD
//!
//! `app.log` is emptied at every launch, so a line there is gone by the launch
//! after the fallback, long before anyone wonders why the setting changed.
//! `keep_record` appends to a file of its own that nothing empties.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

const RECORD_FILE: &str = "gpu-fallback.log";
const COUNT_FILE: &str = ".gpu_setup_unfinished";
/// What the mark held before it named its backend.
const UNNAMED_MARK: &str = "initializing_gpu";
const AUTO: &str = "auto";
const OPENGL: &str = "gl";

/// What the fallback did, for the message the user sees. `backend` is the
/// saved setting it moved away from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Fallback {
    /// OpenGL for this launch only; the saved setting stays.
    ThisLaunch { backend: String },
    /// A second unfinished setup in a row; the saved setting becomes `gl`.
    Saved { backend: String },
}

/// What the launch learned from the run before it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LastRun {
    pub fallback: Option<Fallback>,
    /// For the log and the record, when setup did not finish.
    pub note: Option<String>,
}

#[derive(Default)]
pub struct GpuSetupMark {
    path: OnceLock<PathBuf>,
    running: OnceLock<String>,
    backend_worked: AtomicBool,
    notice: Mutex<Option<Fallback>>,
}

impl GpuSetupMark {
    pub fn set_path(&self, path: PathBuf) {
        let _ = self.path.set(path);
    }

    /// Read what the last run left, once, before the backend is chosen.
    /// `saved` is the processing backend setting.
    pub fn check_last_run(&self, saved: Option<&str>) -> LastRun {
        let saved = saved.unwrap_or(AUTO);
        let counted = self.counted_backend();
        if counted.as_deref().is_some_and(|backend| backend != saved) {
            self.clear_count();
        }

        let leftover = self
            .path
            .get()
            .and_then(|path| fs::read_to_string(path).ok());
        self.clear();
        let Some(leftover) = leftover else {
            let _ = self.running.set(saved.to_string());
            return LastRun::default();
        };
        let tried = match leftover.trim() {
            "" | UNNAMED_MARK => saved,
            backend => backend,
        };

        let backend = saved.to_string();
        let fallback = if tried != saved || saved == OPENGL {
            None
        } else if counted.as_deref() == Some(saved) {
            self.clear_count();
            Some(Fallback::Saved { backend })
        } else {
            self.count(saved);
            Some(Fallback::ThisLaunch { backend })
        };

        let running = if fallback.is_some() { OPENGL } else { saved };
        let _ = self.running.set(running.to_string());
        *self
            .notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = fallback.clone();
        LastRun {
            note: Some(note(fallback.as_ref(), tried, saved)),
            fallback,
        }
    }

    /// The backend this launch runs on: the saved one, or OpenGL after a fallback.
    pub fn running_backend(&self) -> &str {
        self.running.get().map_or(AUTO, String::as_str)
    }

    /// The fallback the user has not been told about yet.
    pub fn take_notice(&self) -> Option<Fallback> {
        self.notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
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
            let _ = fs::write(path, self.running_backend());
        }
    }

    /// Call when setup has produced a working device.
    pub fn finish(&self) {
        self.backend_worked.store(true, Ordering::Release);
        self.clear();
        if self.counted_backend().as_deref() == Some(self.running_backend()) {
            self.clear_count();
        }
    }

    pub fn clear(&self) {
        if let Some(path) = self.path.get() {
            let _ = fs::remove_file(path);
        }
    }

    fn count_path(&self) -> Option<PathBuf> {
        Some(self.path.get()?.with_file_name(COUNT_FILE))
    }

    /// The backend with one unfinished setup against it. The file only ever
    /// counts to one: the second writes the setting and clears it.
    fn counted_backend(&self) -> Option<String> {
        fs::read_to_string(self.count_path()?).ok()
    }

    fn count(&self, backend: &str) {
        if let Some(path) = self.count_path() {
            let _ = fs::write(path, backend);
        }
    }

    fn clear_count(&self) {
        if let Some(path) = self.count_path() {
            let _ = fs::remove_file(path);
        }
    }
}

fn note(fallback: Option<&Fallback>, tried: &str, saved: &str) -> String {
    let what = format!(
        "GPU setup did not finish on the last run, on {tried} (a driver crash, a hang or a \
         forced stop, which look the same from here)."
    );
    match fallback {
        Some(Fallback::ThisLaunch { .. }) => {
            format!("{what} Using gl for this launch; the saved setting stays {saved}.")
        }
        Some(Fallback::Saved { .. }) => {
            format!("{what} Second time in a row: the saved setting changes from {saved} to gl.")
        }
        None => format!("{what} The saved setting stays {saved}."),
    }
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

    fn this_launch(backend: &str) -> Option<Fallback> {
        Some(Fallback::ThisLaunch {
            backend: backend.to_string(),
        })
    }

    fn saved(backend: &str) -> Option<Fallback> {
        Some(Fallback::Saved {
            backend: backend.to_string(),
        })
    }

    /// One launch on a config folder that outlives it.
    struct Launch {
        mark: GpuSetupMark,
        last_run: LastRun,
    }

    fn launch(path: &Path, saved: &str) -> Launch {
        let mark = GpuSetupMark::default();
        mark.set_path(path.to_path_buf());
        let last_run = mark.check_last_run(Some(saved));
        Launch { mark, last_run }
    }

    /// A launch whose first setup finishes.
    fn launch_that_works(path: &Path, saved: &str) -> Launch {
        let run = launch(path, saved);
        run.mark.begin();
        run.mark.finish();
        run
    }

    /// A launch stopped during its first setup.
    fn launch_that_stops(path: &Path, saved: &str) -> Launch {
        let run = launch(path, saved);
        run.mark.begin();
        run
    }

    fn mark_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mark");
        (dir, path)
    }

    #[test]
    fn a_clean_run_changes_nothing() {
        let (_dir, path) = mark_path();

        launch_that_works(&path, "vulkan");
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run, LastRun::default());
        assert_eq!(next.mark.running_backend(), "vulkan");
        assert_eq!(next.mark.take_notice(), None);
    }

    #[test]
    fn stopping_the_app_during_recovery_leaves_no_mark() {
        let (_dir, path) = mark_path();

        let run = launch_that_works(&path, "vulkan");
        // The device is lost, setup runs again, and the app is stopped mid-way.
        run.mark.begin();

        assert_eq!(launch(&path, "vulkan").last_run, LastRun::default());
    }

    #[test]
    fn a_setup_that_returned_an_error_leaves_no_mark_and_the_retry_is_marked() {
        let (_dir, path) = mark_path();

        let run = launch(&path, "vulkan");
        run.mark.begin();
        run.mark.clear();
        assert_eq!(launch(&path, "vulkan").last_run, LastRun::default());

        // An error proved nothing about the backend, so the retry still counts.
        run.mark.begin();
        assert_eq!(
            launch(&path, "vulkan").last_run.fallback,
            this_launch("vulkan")
        );
    }

    #[test]
    fn a_first_failure_runs_one_launch_on_opengl_and_the_setting_stays() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run.fallback, this_launch("vulkan"));
        assert_eq!(next.mark.running_backend(), "gl");
        assert_eq!(next.mark.take_notice(), this_launch("vulkan"));
    }

    #[test]
    fn the_notice_is_given_once() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        let next = launch(&path, "vulkan");

        assert!(next.mark.take_notice().is_some());
        assert_eq!(next.mark.take_notice(), None);
    }

    #[test]
    fn the_launch_after_a_fallback_tries_the_saved_backend_again() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        launch_that_works(&path, "vulkan");
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run, LastRun::default());
        assert_eq!(next.mark.running_backend(), "vulkan");
    }

    #[test]
    fn a_second_failure_in_a_row_saves_opengl() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan"); // 1: unfinished.
        launch_that_works(&path, "vulkan"); // 2: OpenGL for one launch.
        launch_that_stops(&path, "vulkan"); // 3: the saved backend again, unfinished.
        let next = launch(&path, "vulkan"); // 4.

        assert_eq!(next.last_run.fallback, saved("vulkan"));
        assert_eq!(next.mark.running_backend(), "gl");
        assert_eq!(next.mark.take_notice(), saved("vulkan"));
    }

    #[test]
    fn a_success_between_failures_starts_the_count_again() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan"); // Unfinished.
        launch_that_works(&path, "vulkan"); // OpenGL for one launch.
        launch_that_works(&path, "vulkan"); // Vulkan finishes: the count clears.
        launch_that_stops(&path, "vulkan"); // Unfinished, the first again.
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run.fallback, this_launch("vulkan"));
    }

    #[test]
    fn a_backend_changed_between_failures_starts_the_count_again() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan"); // Unfinished on Vulkan.
        launch_that_works(&path, "vulkan"); // OpenGL for one launch.
        launch_that_stops(&path, "dx12"); // Changed by hand, then unfinished.
        let next = launch(&path, "dx12");

        assert_eq!(next.last_run.fallback, this_launch("dx12"));
    }

    #[test]
    fn going_back_to_a_backend_does_not_bring_back_its_old_count() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        launch_that_works(&path, "vulkan"); // OpenGL for one launch.
        launch_that_works(&path, "dx12"); // Changed by hand, and it works.
        launch_that_stops(&path, "vulkan"); // Back to Vulkan, unfinished.
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run.fallback, this_launch("vulkan"));
    }

    #[test]
    fn a_setting_changed_since_the_mark_is_left_alone() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        let next = launch(&path, "dx12");

        assert_eq!(next.last_run.fallback, None);
        assert!(next.last_run.note.is_some());
        assert_eq!(next.mark.running_backend(), "dx12");
        assert_eq!(next.mark.take_notice(), None);
    }

    #[test]
    fn someone_who_chose_opengl_sees_nothing_new() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "gl");
        let next = launch(&path, "gl");

        assert_eq!(next.last_run.fallback, None);
        assert_eq!(next.mark.running_backend(), "gl");
        assert_eq!(next.mark.take_notice(), None);
    }

    #[test]
    fn opengl_failing_during_the_one_launch_does_not_change_the_setting() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan"); // Unfinished on Vulkan.
        launch_that_stops(&path, "vulkan"); // The OpenGL launch, unfinished too.
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run.fallback, None);
        assert_eq!(next.mark.running_backend(), "vulkan");
    }

    #[test]
    fn a_mark_from_before_it_named_its_backend_counts_for_the_saved_one() {
        let (_dir, path) = mark_path();

        fs::write(&path, UNNAMED_MARK).unwrap();
        let next = launch(&path, "vulkan");

        assert_eq!(next.last_run.fallback, this_launch("vulkan"));
    }

    #[test]
    fn no_setting_yet_counts_as_auto() {
        let (_dir, path) = mark_path();

        let mark = GpuSetupMark::default();
        mark.set_path(path.clone());
        assert_eq!(mark.check_last_run(None), LastRun::default());
        assert_eq!(mark.running_backend(), "auto");
        mark.begin();

        assert_eq!(launch(&path, "auto").last_run.fallback, this_launch("auto"));
    }

    #[test]
    fn the_note_says_what_happened_without_claiming_a_crash() {
        let (_dir, path) = mark_path();

        launch_that_stops(&path, "vulkan");
        let first = launch(&path, "vulkan").last_run.note.unwrap();
        assert!(first.contains("on vulkan"));
        assert!(first.contains("Using gl for this launch"));
        assert!(!first.contains("crash detected"));

        launch_that_stops(&path, "vulkan");
        let second = launch(&path, "vulkan").last_run.note.unwrap();
        assert!(second.contains("from vulkan to gl"));
    }

    #[test]
    fn the_message_tells_the_interface_which_fallback_this_was() {
        assert_eq!(
            serde_json::to_value(this_launch("vulkan")).unwrap(),
            serde_json::json!({ "kind": "thisLaunch", "backend": "vulkan" })
        );
        assert_eq!(
            serde_json::to_value(saved("vulkan")).unwrap(),
            serde_json::json!({ "kind": "saved", "backend": "vulkan" })
        );
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
