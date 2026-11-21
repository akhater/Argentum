//! What a panic leaves behind, and the message when one ends the app. RAWmakase's.
//!
//! Copied from RAWmakase e62bf3e and fb6c877 (#679, MIT): `src/app/crash_log.rs`
//! (at v0.7.1 `src/app/shell/crash_log.rs`, moved, unchanged since) and
//! `show_error` in `src/main.rs`.
//!
//! WHY A FILE OF ITS OWN
//!
//! `app.log` is emptied at every launch, so a panic's line there is gone as
//! soon as the app is opened again, which is when someone goes looking for it.
//! Every panic, including those a worker catches, is appended to `crash.log`
//! beside it, which nothing empties. Past 1 MB it moves to `crash.old.log`, so
//! a panic repeated on every frame cannot fill the disk.
//!
//! WHY THE MESSAGE IS NOT SHOWN FROM THE HOOK
//!
//! The released app has no console on Windows, so a panic that ended it closed
//! the window without a word. The hook runs on the panicking thread, which may
//! hold any lock, so it only writes the record. The event loop hands a panic on
//! the main thread back out of `run` (tao's runner re-raises it), and
//! [`run_catching`] shows the message there, on the main thread, once
//! unwinding is over. On macOS and Linux the panic is recorded only, as
//! RAWmakase does.
//!
//! A panic that aborts instead of unwinding (inside a system callback that
//! cannot unwind, or a second panic during the first) is still recorded, but
//! shows no message.

use std::{
    any::Any,
    backtrace::Backtrace,
    fs,
    io::Write,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, PoisonError},
};

const FILE: &str = "crash.log";
const OLD_FILE: &str = "crash.old.log";
/// Past this size the log moves to [`OLD_FILE`].
const LIMIT_BYTES: u64 = 1 << 20;

/// The crash log the message names; set only for the app with a window.
static SHOWN_LOG: OnceLock<PathBuf> = OnceLock::new();

/// Records every panic in `crash.log` in `log_dir`, and its line in `app.log`.
/// A headless export never shows the message.
pub fn install(log_dir: &Path, headless: bool) {
    let path = log_dir.join(FILE);
    if !headless {
        let _ = SHOWN_LOG.set(path.clone());
    }
    panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("unknown panic");
        let location = info
            .location()
            .map_or_else(|| "an unknown location".to_string(), ToString::to_string);
        log::error!("PANIC! at {} - {}", location, message.trim());
        let thread = std::thread::current();
        let text = record(
            &chrono::Local::now().to_rfc3339(),
            thread.name().unwrap_or("unnamed"),
            message,
            &location,
            &Backtrace::force_capture().to_string(),
        );
        // Nothing to report a failure to from here.
        let _ = append(&path, &text);
    }));
}

/// Runs `app`. A panic that unwinds out of it shows the message naming the
/// crash log, then goes on ending the process as before.
pub fn run_catching(app: impl FnOnce()) {
    if let Err(panic) = panic::catch_unwind(AssertUnwindSafe(app)) {
        #[cfg(windows)]
        if let Some(log) = SHOWN_LOG.get() {
            show(&message_text(panic_message(&*panic), log), log);
        }
        panic::resume_unwind(panic);
    }
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

fn record(time: &str, thread: &str, message: &str, location: &str, backtrace: &str) -> String {
    format!(
        "=== {time} Argentum {} ({} {})\nthread '{thread}' panicked at {location}:\n{message}\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Appends `text`, first moving a full log aside. Threads panicking at once
/// take turns, so none checks the size of a log another is moving aside.
fn append(path: &Path, text: &str) -> std::io::Result<()> {
    static WRITING: Mutex<()> = Mutex::new(());
    // A poisoned lock still serializes; panicking here would abort.
    let _turn = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > LIMIT_BYTES) {
        fs::rename(path, path.with_file_name(OLD_FILE))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn message_text(message: &str, log: &Path) -> String {
    format!(
        "Argentum stopped because of an error.\n\n{message}\n\nDetails are in {}; please attach it to a bug report.\n\nOpen its folder?",
        log.display()
    )
}

/// A message box with no owner window: the app's own may already be gone.
#[cfg(windows)]
fn show(text: &str, log: &Path) {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONERROR, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW,
    };
    use windows::core::PCWSTR;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("Argentum"));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let answer = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    if answer == IDYES {
        let _ = crate::file_management::show_in_finder(log.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_in_a_worker_thread_is_recorded_beside_app_log() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), true);
        let joined = std::thread::Builder::new()
            .name("render-worker".into())
            .spawn(|| panic!("wgpu error: device lost"))
            .unwrap()
            .join();
        // Back to the default hook, so later panics in other tests stay out of
        // a folder that is about to be deleted.
        let _ = panic::take_hook();
        assert!(joined.is_err());

        let text = fs::read_to_string(dir.path().join(FILE)).unwrap();
        let header = format!(
            " Argentum {} ({} {})\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert!(text.starts_with("=== "), "{text}");
        assert!(text.contains(&header), "{text}");
        assert!(
            text.contains("thread 'render-worker' panicked at src"),
            "{text}"
        );
        assert!(text.contains("crash_log.rs:"), "{text}");
        assert!(text.contains(":\nwgpu error: device lost\n"), "{text}");
        // The backtrace follows the message.
        assert!(text.lines().count() > 3, "{text}");
    }

    #[test]
    fn a_panic_out_of_the_app_still_ends_it() {
        let panic = panic::catch_unwind(|| run_catching(|| panic!("boom"))).unwrap_err();
        assert_eq!(panic_message(&*panic), "boom");
        run_catching(|| {});
    }

    #[test]
    fn the_message_names_the_panic_and_the_log() {
        let log = Path::new(r"C:\logs\crash.log");
        let text = message_text(panic_message(&String::from("device lost")), log);
        assert!(text.contains("\ndevice lost\n"), "{text}");
        assert!(text.contains(&log.display().to_string()), "{text}");
    }

    #[test]
    fn records_are_appended_and_a_full_log_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs").join(FILE);
        let first = record(
            "2026-11-21T10:00:00+01:00",
            "main",
            "boom",
            "src/lib.rs:1:2",
            "bt",
        );
        append(&log, &first).unwrap();
        append(&log, "second\n").unwrap();
        let text = fs::read_to_string(&log).unwrap();
        assert!(
            text.starts_with("=== 2026-11-21T10:00:00+01:00 Argentum "),
            "{text}"
        );
        assert!(text.contains("thread 'main' panicked at src/lib.rs:1:2:\nboom\nbt\n"));
        assert!(text.ends_with("second\n"));

        fs::write(&log, vec![b'x'; LIMIT_BYTES as usize + 1]).unwrap();
        append(&log, "after\n").unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap(), "after\n");
        let old = fs::metadata(dir.path().join("logs").join(OLD_FILE)).unwrap();
        assert_eq!(old.len(), LIMIT_BYTES + 1);
    }

    #[test]
    fn records_from_threads_at_once_all_survive_moving_a_full_log_aside() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join(FILE);
        let barrier = std::sync::Barrier::new(16);
        for _round in 0..20 {
            fs::write(&log, vec![b'x'; LIMIT_BYTES as usize + 1]).unwrap();
            let _ = fs::remove_file(dir.path().join(OLD_FILE));
            std::thread::scope(|s| {
                for n in 0..16 {
                    let (log, barrier) = (&log, &barrier);
                    s.spawn(move || {
                        barrier.wait();
                        append(log, &format!("record {n}\n")).unwrap();
                    });
                }
            });
            let kept = fs::read_to_string(&log).unwrap()
                + &String::from_utf8_lossy(&fs::read(dir.path().join(OLD_FILE)).unwrap());
            for n in 0..16 {
                assert!(kept.contains(&format!("record {n}\n")), "lost record {n}");
            }
        }
    }

    /// By hand, on Windows: shows the box a crash shows, and waits for an answer.
    /// `ARGENTUM_SHOW_CRASH_BOX=1 cargo test --lib mods::crash_log::tests::shows_the_message_box -- --ignored --exact`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn shows_the_message_box() {
        if std::env::var_os("ARGENTUM_SHOW_CRASH_BOX").is_none() {
            return;
        }
        let log = std::env::temp_dir().join(FILE);
        show(
            &message_text("forced panic for a look at the message", &log),
            &log,
        );
    }
}
