//! The window always closes, even when the page cannot answer. Ours.
//!
//! With a close-requested listener in the page (src/argentum/saveOnClose.ts), Tauri holds the
//! window open until the page has saved and asks for the destroy. A page that is busy (start-up)
//! or hung would then keep the window open for good. On every close request a one-shot timer
//! starts here: when it fires and the window still exists, Rust destroys it, and the normal exit
//! path runs. When the page finished first the window is gone and the timer does nothing.

use std::time::Duration;
use tauri::{Manager, WebviewWindow};

/// How long the page may take to save before closing: `CLOSE_WAIT_MS` in saveOnClose.ts, kept
/// equal by a test below (the generated contract carries types, not constants).
const PAGE_WAIT_MS: u64 = 2000;
/// Room for the page to finish its wait and ask for the destroy, so the failsafe never beats it.
const MARGIN_MS: u64 = 500;
/// How long after a close request Rust destroys the window itself.
pub const FAILSAFE_MS: u64 = PAGE_WAIT_MS + MARGIN_MS;
const _: () = assert!(MARGIN_MS > 0); // the failsafe never beats the page

/// Whether the failsafe has work to do: only while the window still exists.
fn must_destroy(window_still_open: bool) -> bool {
    window_still_open
}

/// Starts the one-shot timer for a close request on `window`.
pub fn arm(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(FAILSAFE_MS)).await;
        let still_open = app.get_webview_window(&label);
        if must_destroy(still_open.is_some())
            && let Some(window) = still_open
        {
            log::warn!("The page did not close the window in time; destroying it.");
            let _ = window.destroy();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acts_only_while_the_window_exists() {
        assert!(must_destroy(true));
        assert!(!must_destroy(false));
    }

    #[test]
    fn page_wait_equals_the_one_in_the_page() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/argentum/saveOnClose.ts");
        let source = std::fs::read_to_string(path).unwrap();
        let line = source
            .lines()
            .find(|l| l.starts_with("export const CLOSE_WAIT_MS"))
            .unwrap();
        let number: u64 = line
            .split('=')
            .nth(1)
            .unwrap()
            .trim()
            .trim_end_matches(';')
            .trim()
            .parse()
            .unwrap();
        assert_eq!(number, PAGE_WAIT_MS);
    }
}
