//! What the UI thread must never wait for. Ours; tests only.
//!
//! THE FREEZE THESE TESTS HOLD SHUT
//!
//! A synchronous `#[tauri::command]` runs on the UI thread, and a Tauri window
//! getter called from any other thread blocks until the UI thread answers it.
//! Three threads closed a circle out of those two facts:
//!
//! - the UI thread, rendering a look's swatch, waited for `gpu_processor`;
//! - the preview worker, holding `gpu_processor` to the end of its render,
//!   waited for `display` to put the frame on screen;
//! - `update_wgpu_transform`, holding `display`, asked for the window handle and
//!   waited for the UI thread.
//!
//! Nobody could move, and the window was gone for good. Switching between an
//! upright and a sideways photo set it off: the viewer's rectangle changes, which
//! sends the transform, at the moment the new size rebuilds the processor, which
//! holds `gpu_processor` long enough for a swatch to run into it. It needs no
//! particular GPU backend, and nothing needs to be on screen.
//!
//! Either rule below breaks the circle alone. Both are kept because each is the
//! rule on its own: a render on the UI thread stalls the window whether or not it
//! ends in a deadlock, and a lock held across a question to the UI thread is a
//! deadlock waiting for whichever lock the UI thread takes next.

use std::path::{Path, PathBuf};

/// The source files of the app, tests included.
fn sources() -> Vec<(PathBuf, String)> {
    fn walk(folder: &Path, found: &mut Vec<(PathBuf, String)>) {
        for entry in std::fs::read_dir(folder).expect("source folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("source file");
                found.push((path, text));
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found
}

/// The body of the function that starts at `from`: from its first `{` to the
/// brace that closes it. Good enough for this code, which keeps no unbalanced
/// brace in a string or comment inside a command.
fn body_from(source: &str, from: usize) -> &str {
    let open = from + source[from..].find('{').expect("function body");
    let mut depth = 0;
    for (at, c) in source[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[open..=open + at];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced function body");
}

/// Every function declared in `source`, as `(name, body)`.
fn functions(source: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find("fn ") {
        let start = from + at;
        from = start + "fn ".len();
        let line_start = source[..start].rfind('\n').map_or(0, |n| n + 1);
        let lead = source[line_start..start].trim();
        if !matches!(lead, "" | "pub" | "pub(crate)" | "async" | "pub async") {
            continue;
        }
        let name_end = source[from..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(from, |n| from + n);
        let rest = &source[name_end..];
        // A declaration without a body, as in a trait or an extern block.
        if rest.find(';').is_some_and(|semi| Some(semi) < rest.find('{')) {
            continue;
        }
        found.push((&source[from..name_end], body_from(source, start)));
    }
    found
}

/// `code` with its whitespace gone, so a chain broken over lines still reads as
/// one: `context\n    .display\n    .lock()` is `context.display.lock()`.
fn flat(code: &str) -> String {
    code.split_whitespace().collect()
}

/// The synchronous commands in `source`, as `(name, body)`.
fn synchronous_commands(source: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find("#[tauri::command]") {
        from += at + "#[tauri::command]".len();
        let after = source[from..].trim_start();
        let start = source.len() - after.len();
        let is_sync = after.starts_with("fn ") || after.starts_with("pub fn ");
        if !is_sync {
            continue;
        }
        let name_at = start + after.find("fn ").expect("fn") + 3;
        let name_end = name_at
            + source[name_at..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .expect("name");
        found.push((&source[name_at..name_end], body_from(source, name_at)));
    }
    found
}

/// What a command on the UI thread may not touch: a render, or a lock that a
/// render holds from start to end.
const RENDERS: [&str; 5] = [
    "process_and_get_dynamic_image",
    "render_lut_swatch",
    "gpu_processor",
    "gpu_image_cache",
    ".display.lock",
];

#[test]
fn no_command_on_the_ui_thread_touches_a_render() {
    let mut caught = Vec::new();
    for (path, source) in sources() {
        for (name, body) in synchronous_commands(&source) {
            let body = flat(body);
            for word in RENDERS {
                if body.contains(word) {
                    caught.push(format!("{} in {}: {word}", name, path.display()));
                }
            }
        }
    }
    assert!(caught.is_empty(), "{caught:#?}");
}

/// What sends a question to the UI thread and waits for the answer.
const ASKS_THE_UI_THREAD: [&str; 2] = ["main_window()", ".hwnd()"];

#[test]
fn nothing_asks_the_ui_thread_while_it_holds_the_display() {
    let mut caught = Vec::new();
    for (path, source) in sources() {
        // Only the files that take the lock are read as functions: some others
        // hold a lone brace in a string, which would end a body in the wrong place.
        if path.ends_with("ui_thread.rs") || !flat(&source).contains(".display.lock") {
            continue;
        }
        for (name, body) in functions(&source) {
            let body = flat(body);
            let Some(locked) = body.find(".display.lock") else {
                continue;
            };
            let held = &body[locked..];
            for word in ASKS_THE_UI_THREAD {
                if held.contains(word) {
                    caught.push(format!("{} in {}: {word}", name, path.display()));
                }
            }
        }
    }
    assert!(caught.is_empty(), "{caught:#?}");
}
