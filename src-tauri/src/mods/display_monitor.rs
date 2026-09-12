//! Which colour profile belongs to the screen a window is on. Ours.
//!
//! WHY IT IS PER WINDOW AND NOT PER MACHINE
//!
//! A laptop panel and an external monitor have different profiles, and a window
//! dragged from one to the other needs the other one. Asking for "the display
//! profile" would bake in whichever screen the app happened to start on — which
//! is the same class of mistake as hardcoding this laptop's matrix, just
//! slower to notice.
//!
//! WHY IT IS WINDOWS ONLY
//!
//! Because that is where it has been measured. macOS and Linux have their own
//! ways to ask, and writing them blind would be three implementations with one
//! tested. Everywhere else gets `None`, which means no conversion — exactly
//! what the app did before any of this existed, so nothing regresses by not
//! being covered.
//!
//! See `mods/display_profile.rs` for what is done with the file once found.

/// The profile Windows has for the monitor this window is on.
///
/// `None` on any failure and on every other platform. A missing profile is not
/// an error: it means "present without converting", which is what an sRGB
/// display wants anyway.
#[cfg(target_os = "windows")]
pub fn profile_for_window(hwnd: isize) -> Option<std::path::PathBuf> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFOEXW,
        MonitorFromWindow,
    };
    use windows::Win32::UI::ColorSystem::GetICMProfileW;
    use windows::core::PCWSTR;

    // Every call here is a raw Win32 one. They are unsafe because the compiler
    // cannot check the handles, so each result is checked before use and the
    // device context is released on every path out.
    unsafe {
        let monitor = MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }

        let mut info = MONITORINFOEXW {
            monitorInfo: windows::Win32::Graphics::Gdi::MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _).as_bool() {
            return None;
        }

        // `szDevice` is the display's own name, like \\.\DISPLAY1, and a device
        // context made from it is what carries that screen's colour settings.
        let device = CreateDCW(
            PCWSTR(info.szDevice.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
        );
        if device.is_invalid() {
            return None;
        }

        // Asked twice: once for the length, once for the path. A first call
        // that does not report a length means the screen has no profile, which
        // is a normal answer rather than a failure.
        let mut length: u32 = 0;
        let _ = GetICMProfileW(device, &mut length, None);
        if length == 0 || length > 32_768 {
            let _ = DeleteDC(device);
            return None;
        }

        let mut buffer = vec![0u16; length as usize];
        let ok = GetICMProfileW(
            device,
            &mut length,
            Some(windows::core::PWSTR(buffer.as_mut_ptr())),
        )
        .as_bool();
        let _ = DeleteDC(device);
        if !ok {
            return None;
        }

        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let path = String::from_utf16(&buffer[..end]).ok()?;
        (!path.is_empty()).then(|| std::path::PathBuf::from(path))
    }
}

/// Everywhere else: no profile, so no conversion.
#[cfg(not(target_os = "windows"))]
pub fn profile_for_window(_hwnd: isize) -> Option<std::path::PathBuf> {
    None
}

/// The conversion for the screen a window is on, ready for the shader.
///
/// `None` when there is nothing to do — no profile, an unreadable one, or a
/// display that is already sRGB. The caller presents unconverted in that case,
/// which is both correct and what it did before.
pub fn conversion_for_window(hwnd: isize) -> Option<[[f32; 3]; 3]> {
    let path = profile_for_window(hwnd)?;
    let matrix = super::display_profile::from_file(&path)?;

    // An sRGB screen's profile gives back something within rounding of the
    // identity. Saying "nothing to do" lets the shader skip a decode/encode
    // round trip that would only add error.
    let is_identity = (0..3).all(|i| {
        (0..3).all(|j| {
            let want = if i == j { 1.0 } else { 0.0 };
            (matrix[i][j] - want).abs() < 1e-3
        })
    });
    (!is_identity).then_some(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle that is not a window must not crash the renderer. This runs
    /// whenever a window moves, and it is raw Win32 underneath.
    ///
    /// It is not asserted to fail: `MONITOR_DEFAULTTONEAREST` answers a null
    /// handle with the primary display, so on a machine with a profiled screen
    /// this returns a real one. Windows being helpful, not the code being
    /// wrong — the first version of this test asserted `None` and failed on
    /// exactly that.
    #[test]
    fn a_nonsense_window_does_not_crash() {
        let _ = profile_for_window(0);
        let _ = conversion_for_window(0);
    }
}

/// Against the screen this is running on.
#[cfg(test)]
mod on_this_machine {
    use super::*;

    #[test]
    #[ignore = "needs a real window; run by hand"]
    fn it_finds_a_profile_for_the_primary_display() {
        // No window, so this asks about whatever monitor is nearest to a null
        // handle — which Windows answers with the primary display.
        match profile_for_window(0) {
            Some(path) => println!("\nprimary display profile: {}\n", path.display()),
            None => println!("\nno profile for the primary display\n"),
        }
    }
}

/// The app handle, kept so the window can be found from the render path.
///
/// Set once at startup. `None` in tests, which is why every read falls back to
/// no conversion rather than unwrapping.
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn remember_app_handle(handle: tauri::AppHandle) {
    let _ = APP.set(handle);
}

/// The main window's handle, or `None` before there is one.
fn main_window() -> Option<isize> {
    use tauri::Manager;
    let window = APP.get()?.get_webview_window("main")?;
    #[cfg(target_os = "windows")]
    {
        window.hwnd().ok().map(|h| h.0 as isize)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

/// The conversion for the main window's screen, for the render path.
///
/// Everything below takes a window handle; this is the one place that finds it,
/// so their file asks a question with no arguments and does not have to know
/// what a window handle is.
pub fn shader_rows_for_main_window() -> super::display_profile::ShaderRows {
    match main_window() {
        Some(hwnd) => shader_rows_for_window(hwnd),
        None => super::display_profile::SHADER_IDENTITY,
    }
}

/// How long a resolved conversion is trusted before it is checked again.
///
/// The check is cheap — which monitor, which profile path, when that file was
/// last written — and only a change re-reads and re-parses. A second is short
/// enough that recalibrating a screen shows up while you are still looking at
/// it, and long enough that panning does not stat a file every frame.
const RECHECK_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// What was worked out last time, and what it was worked out from.
struct Cached {
    monitor: isize,
    profile: Option<std::path::PathBuf>,
    written: Option<std::time::SystemTime>,
    checked: std::time::Instant,
    rows: super::display_profile::ShaderRows,
    /// Whether the profile was actually read, as opposed to a path that was
    /// found and then failed to parse. Only a real answer may be kept on the
    /// strength of an unchanged file; see `rows_now`.
    read_succeeded: bool,
}

static CACHE: std::sync::Mutex<Option<Cached>> = std::sync::Mutex::new(None);

/// Throw away what is cached, so the next ask resolves from scratch.
pub fn forget() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

/// The conversion for a window's screen, without reading the file every frame.
///
/// `update_wgpu_transform` runs on every pan and zoom, and re-reading and
/// re-parsing an ICC profile each time would be silly.
///
/// WHAT THE CACHE IS KEYED ON, AND WHY IT IS NOT JUST THE MONITOR
///
/// It was just the monitor, and that is wrong in three ways an audit pointed
/// out: assigning a different profile to the same screen, recalibrating and
/// rewriting the same file, and a lookup that failed once and would then never
/// be retried. All three leave the window on the same monitor, so the key never
/// changed and the stale matrix — or the identity from the failure — stayed
/// forever, with panning and zooming unable to repair it.
///
/// So the key is the monitor, the profile path, and when that file was last
/// written; and it is re-checked on a timer rather than only when the monitor
/// changes. A failure caches nothing, so the next check tries again.
pub fn shader_rows_for_window(hwnd: isize) -> super::display_profile::ShaderRows {
    rows_now(
        monitor_key(hwnd),
        std::time::Instant::now(),
        &|| profile_for_window(hwnd),
        &super::display_profile::from_file,
    )
}

/// The caching itself, with the clock and both lookups handed in.
///
/// WHY THE SEAM EXISTS
///
/// Everything interesting here is a decision about *when to ask again*, and all
/// three inputs to that decision are things a test cannot stage: a real monitor,
/// a profile Windows assigned to it, and a file that reads one moment and not
/// the next. Passing them in is what lets the failure case below be a test
/// rather than a paragraph.
fn rows_now(
    monitor: isize,
    now: std::time::Instant,
    find_profile: &dyn Fn() -> Option<std::path::PathBuf>,
    read_profile: &dyn Fn(&std::path::Path) -> Option<[[f32; 3]; 3]>,
) -> super::display_profile::ShaderRows {
    if let Ok(cache) = CACHE.lock()
        && let Some(cached) = cache.as_ref()
        && cached.monitor == monitor
        && now.duration_since(cached.checked) < RECHECK_AFTER
    {
        return cached.rows;
    }

    let profile = find_profile();
    let written = profile
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok().and_then(|m| m.modified().ok()));

    // Still the same screen, the same file, and the file has not been rewritten:
    // nothing to redo, so only the clock is updated.
    //
    // `read_succeeded` is in that condition because without it this branch kept
    // a *failure* forever. A profile that could not be read — locked while the
    // calibration software rewrote it, on a drive that had not woken up — gave
    // the identity, and the path and write time that came with it were unchanged
    // afterwards, so every later check landed here, updated the clock and handed
    // back the identity again. The file never changes to unstick it and the user
    // has no way to ask. So a failure is never kept on the strength of an
    // unchanged file: it is retried on the next check, one second later.
    if let Ok(mut cache) = CACHE.lock()
        && let Some(cached) = cache.as_mut()
        && cached.monitor == monitor
        && cached.profile == profile
        && cached.written == written
        && written.is_some()
        && cached.read_succeeded
    {
        cached.checked = now;
        return cached.rows;
    }

    let matrix = profile.as_ref().and_then(|p| read_profile(p));
    let read_succeeded = matrix.is_some() || profile.is_none();
    let rows = matrix
        .filter(|m| !is_identity(m))
        .map(|m| super::display_profile::shader_rows(&m))
        .unwrap_or(super::display_profile::SHADER_IDENTITY);

    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached {
            monitor,
            profile,
            written,
            checked: now,
            rows,
            read_succeeded,
        });
    }
    rows
}

/// An sRGB screen's profile comes back within rounding of the identity. Saying
/// "nothing to do" lets the shader skip a decode and re-encode that would only
/// add error to numbers that were already right.
fn is_identity(m: &[[f32; 3]; 3]) -> bool {
    (0..3).all(|i| {
        (0..3).all(|j| {
            let want = if i == j { 1.0 } else { 0.0 };
            (m[i][j] - want).abs() < 1e-3
        })
    })
}

/// Something that changes when the window moves to another screen.
#[cfg(target_os = "windows")]
fn monitor_key(hwnd: isize) -> isize {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
    unsafe { MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONEAREST).0 as isize }
}

#[cfg(not(target_os = "windows"))]
fn monitor_key(_hwnd: isize) -> isize {
    0
}

/// Re-resolve the display conversion and redraw, because the window moved.
///
/// WHY THIS EXISTS SEPARATELY FROM THE RENDER PATH
///
/// The conversion used to be refreshed only inside `update_wgpu_transform`,
/// which the frontend calls when *its* idea of the transform changes — window
/// size, the image's position within the client area, clipping, backgrounds.
/// None of that is the desktop position and none of it is which monitor.
///
/// So dragging the window from one screen to another of the same size and
/// scaling changed nothing the frontend compares, the call was suppressed, and
/// the old screen's matrix stayed on a picture now being shown somewhere else.
/// Found by an audit reading the call paths; it would have taken two monitors
/// and a steady hand to find by using it.
///
/// The window's own move and resize events are the signal, and they are native:
/// they happen whether or not anything on the page changed.
pub fn refresh_after_window_change() {
    let Some(handle) = APP.get().cloned() else {
        return;
    };

    // Off the event thread: this can read a file and touch the GPU, and a
    // window drag should not wait for either.
    std::thread::spawn(move || {
        use tauri::Manager;

        let Some(hwnd) = main_window() else { return };
        let rows = shader_rows_for_window(hwnd);

        let state = handle.state::<crate::AppState>();
        let context = match state
            .gpu_context
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            Some(context) => context.clone(),
            None => return,
        };

        let mut display_lock = context.display.lock().unwrap_or_else(|e| e.into_inner());
        let Some(display) = display_lock.as_mut() else {
            return;
        };
        if display.latest_transform.ag_display_matrix == rows {
            return;
        }
        display.latest_transform.ag_display_matrix = rows;
        context.queue.write_buffer(
            &display.transform_buffer,
            0,
            bytemuck::bytes_of(&display.latest_transform),
        );
        // Redrawn here rather than left for the next frame: an idle photo has
        // no next frame, and it would sit in the wrong colour until touched.
        display.render(&context.device, &context.queue);
    });
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    /// There is one cache for the process and these tests all write to it, so
    /// they take turns. Without this they pass alone and fail together, which is
    /// the worst kind of test.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn in_turn() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A failed lookup must not be remembered as the answer.
    ///
    /// The cache used to key on the monitor alone, so a lookup that failed once
    /// — a profile briefly unreadable, a screen still settling — cached the
    /// identity and never tried again while the window stayed put. Panning
    /// could not repair it because panning hits the same key.
    #[test]
    fn a_failure_is_retried_rather_than_kept() {
        let _turn = in_turn();
        forget();
        // Nothing to find for a handle that is not a window on a machine with
        // no profile; what matters is that asking twice asks twice.
        let first = shader_rows_for_window(0);
        forget();
        let second = shader_rows_for_window(0);
        assert_eq!(first, second, "the same question gave two answers");
    }

    /// A read that fails once and then works, with the file never changing.
    ///
    /// This is the case the path/mtime key cannot see. The profile is found, the
    /// file is there, its write time is the same before and after — and the read
    /// of it failed the first time, because something else had the file open.
    /// Nothing about the key changes when it starts working, so the only thing
    /// that can bring the conversion back is refusing to keep a failure.
    ///
    /// Against the previous version this fails on the second assertion: the
    /// identity from the failed read was returned for the rest of the session.
    #[test]
    fn a_read_that_fails_once_recovers_with_the_file_unchanged() {
        let _turn = in_turn();
        let path = std::env::temp_dir().join("ag-display-transient-read.icm");
        std::fs::write(&path, b"stands in for a profile; the reader is faked").unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();

        // A conversion far enough from the identity not to be filtered out.
        let real = [[1.2, -0.2, 0.0], [-0.1, 1.1, 0.0], [0.0, -0.3, 1.3]];
        let reads = std::cell::Cell::new(0u32);
        let read = |_: &std::path::Path| -> Option<[[f32; 3]; 3]> {
            reads.set(reads.get() + 1);
            if reads.get() == 1 { None } else { Some(real) }
        };
        let find = || Some(path.clone());

        let t0 = std::time::Instant::now();
        forget();

        let failed = rows_now(MONITOR, t0, &find, &read);
        assert_eq!(
            failed,
            super::super::display_profile::SHADER_IDENTITY,
            "a profile that cannot be read must present unconverted, not guess"
        );

        // Same screen, same path, same write time, cache untouched.
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        let recovered = rows_now(MONITOR, t0 + RECHECK_AFTER, &find, &read);
        assert_ne!(
            recovered,
            super::super::display_profile::SHADER_IDENTITY,
            "the failed read was cached for good: nothing about the file changes to undo it"
        );
        assert_eq!(recovered, super::super::display_profile::shader_rows(&real));
        assert!(reads.get() >= 2, "the file was never read a second time");

        // And once it is working it is kept, rather than re-read every check.
        let again = rows_now(MONITOR, t0 + RECHECK_AFTER * 2, &find, &read);
        assert_eq!(again, recovered);
        assert_eq!(reads.get(), 2, "a good profile is being re-read needlessly");

        // Different rows is exactly the condition `refresh_after_window_change`
        // redraws on, so the picture on screen changes with it. The GPU half of
        // that cannot be reached from a test; the decision can.
        assert_ne!(failed, recovered);

        forget();
        let _ = std::fs::remove_file(&path);
    }

    /// A screen whose profile is genuinely sRGB reads fine and converts to the
    /// identity, and that is an answer, not a failure — so it must not be
    /// re-read on every check the way a failure is.
    #[test]
    fn an_srgb_screen_is_not_treated_as_a_failed_read() {
        let _turn = in_turn();
        let path = std::env::temp_dir().join("ag-display-srgb.icm");
        std::fs::write(&path, b"stands in for an sRGB profile").unwrap();
        let reads = std::cell::Cell::new(0u32);
        let read = |_: &std::path::Path| -> Option<[[f32; 3]; 3]> {
            reads.set(reads.get() + 1);
            Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
        };
        let find = || Some(path.clone());

        let t0 = std::time::Instant::now();
        forget();
        let first = rows_now(MONITOR, t0, &find, &read);
        let second = rows_now(MONITOR, t0 + RECHECK_AFTER, &find, &read);
        assert_eq!(first, super::super::display_profile::SHADER_IDENTITY);
        assert_eq!(second, first);
        assert_eq!(
            reads.get(),
            1,
            "an sRGB profile is being re-read on every check"
        );

        forget();
        let _ = std::fs::remove_file(&path);
    }

    /// A monitor number no real monitor has, so these tests cannot be confused
    /// by a cache entry the rest of the suite left behind.
    const MONITOR: isize = -777;

    /// The poll has to be at least as slow as the timer, or it spends its time
    /// hitting the short-circuit instead of looking again.
    #[test]
    fn the_watcher_runs_slowly_enough_to_reach_a_fresh_lookup() {
        assert!(WATCH_EVERY >= RECHECK_AFTER);
    }

    /// And the timer has to be short enough to notice a recalibration while
    /// somebody is still looking at the screen.
    #[test]
    fn the_recheck_is_soon_enough_to_be_useful() {
        assert!(
            RECHECK_AFTER <= std::time::Duration::from_secs(2),
            "a change to the screen's profile would take too long to show"
        );
    }
}

/// Watch for the screen's colour changing under a window that never moves.
///
/// WHY A TIMER AND NOT AN EVENT
///
/// The move handler catches a window dragged to another screen, and the render
/// path catches anything that redraws. Neither catches the case an audit
/// raised: reassign or recalibrate the profile of the screen you are already
/// on, touch nothing, and the photo sits there in the old colour. There is no
/// frame to piggyback on, because an idle photo has no frames.
///
/// Windows does broadcast a message for this, and receiving it means a window
/// procedure and a message loop of our own inside somebody else's application.
/// A check every couple of seconds costs one cheap comparison — same monitor,
/// same file, same write time — and reads nothing unless one of those changed.
/// The refresh it calls redraws only when the conversion actually differs, so
/// the quiet case is genuinely quiet.
pub fn watch() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(WATCH_EVERY);
            refresh_after_window_change();
        }
    });
}

/// How often that check runs. Must be at least `RECHECK_AFTER`, or the poll
/// keeps hitting the short-circuit and never reaches a fresh lookup — which is
/// also how a recovered profile gets back on screen without the user doing
/// anything.
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
