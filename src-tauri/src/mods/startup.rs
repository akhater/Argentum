//! Everything Argentum does when the app starts. One anchor.
//!
//! `lib.rs` calls `init` once, and that call is the whole upstream cost of any
//! startup work we ever add. It replaced a direct call to the thumbnail cache
//! check for exactly that reason: the second thing that needed to happen at
//! startup would otherwise have been a second line of their file.
//!
//! It takes the app handle rather than a path because different things want
//! different directories, and resolving them here keeps that decision on our
//! side. It matters: the thumbnail check works on the *cache* directory, which
//! is disposable by definition, while imported camera profiles are the user's
//! own files and must not live somewhere a cleaner may empty.

use tauri::{AppHandle, Manager};

/// Called once, early, before any photo is opened.
///
/// Best-effort throughout. A directory that cannot be resolved disables the
/// feature that needed it; none of them is a reason to stop the app starting.
pub fn init(app: &AppHandle) {
    // Kept so the camera-profile table can ask which photo is open and never
    // throw that one away.
    super::profile_correction::remember_app_handle(app.clone());
    // And so the display conversion can find the window, to find its screen.
    super::display_monitor::remember_app_handle(app.clone());
    // And notice if that screen's profile is changed under a window that never
    // moves — there is no frame to catch it on.
    super::display_monitor::watch();


    if let Ok(cache) = app.path().app_cache_dir() {
        super::cache_version::clear_thumbnails_if_pipeline_changed(&cache);
    }

    // Imported profiles are user data, not cache.
    if let Ok(data) = app.path().app_data_dir()
        && let Ok(library) = super::profiles::library_dir(&data)
    {
        // Highlight recovery's on/off lives beside them, for the same reason:
        // it is ours, and it is a preference rather than a cache.
        super::highlights::load(&library);
        super::profiles::set_library(library);
    }
}
