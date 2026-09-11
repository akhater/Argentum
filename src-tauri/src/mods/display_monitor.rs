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
        CreateDCW, DeleteDC, GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW,
        MONITOR_DEFAULTTONEAREST,
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
    let monitor = monitor_key(hwnd);
    let now = std::time::Instant::now();

    if let Ok(cache) = CACHE.lock()
        && let Some(cached) = cache.as_ref()
        && cached.monitor == monitor
        && now.duration_since(cached.checked) < RECHECK_AFTER
    {
        return cached.rows;
    }

    let profile = profile_for_window(hwnd);
    let written = profile.as_ref().and_then(|p| {
        std::fs::metadata(p).ok().and_then(|m| m.modified().ok())
    });

    // Still the same screen, the same file, and the file has not been rewritten:
    // nothing to redo, so only the clock is updated.
    if let Ok(mut cache) = CACHE.lock()
        && let Some(cached) = cache.as_mut()
        && cached.monitor == monitor
        && cached.profile == profile
        && cached.written == written
        && written.is_some()
    {
        cached.checked = now;
        return cached.rows;
    }

    let rows = profile
        .as_ref()
        .and_then(|p| super::display_profile::from_file(p))
        .filter(|m| !is_identity(m))
        .map(|m| super::display_profile::shader_rows(&m))
        .unwrap_or(super::display_profile::SHADER_IDENTITY);

    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached { monitor, profile, written, checked: now, rows });
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
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
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

    /// A failed lookup must not be remembered as the answer.
    ///
    /// The cache used to key on the monitor alone, so a lookup that failed once
    /// — a profile briefly unreadable, a screen still settling — cached the
    /// identity and never tried again while the window stayed put. Panning
    /// could not repair it because panning hits the same key.
    #[test]
    fn a_failure_is_retried_rather_than_kept() {
        forget();
        // Nothing to find for a handle that is not a window on a machine with
        // no profile; what matters is that asking twice asks twice.
        let first = shader_rows_for_window(0);
        forget();
        let second = shader_rows_for_window(0);
        assert_eq!(first, second, "the same question gave two answers");
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
            std::thread::sleep(std::time::Duration::from_secs(2));
            refresh_after_window_change();
        }
    });
}
