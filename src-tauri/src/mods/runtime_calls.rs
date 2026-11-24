//! A source check, no runtime code. Ours.
//!
//! `tokio::task::spawn_blocking` and `tokio::spawn` panic ("there is no reactor
//! running") when called from a thread with no Tokio context, such as the
//! main thread during start-up or a window-event handler. Tauri's own
//! `tauri::async_runtime::spawn` / `spawn_blocking` work from anywhere. A
//! direct Tokio call is safe inside an `async fn` (it runs on the runtime);
//! 26.48.1 crashed at every start because one sat in the synchronous setup
//! code. No test launches the app, so this test reads the source instead:
//! every direct Tokio spawn must be inside an `async fn`.

#[cfg(test)]
mod tests {
    use std::path::Path;

    const CALLS: [&str; 3] = [
        "tokio::task::spawn_blocking(",
        "tokio::spawn(",
        "tokio::task::spawn(",
    ];

    /// The signature of the function a line sits in: the nearest line above it
    /// that declares one.
    fn enclosing_fn(lines: &[&str], at: usize) -> Option<String> {
        lines[..at]
            .iter()
            .rev()
            .find(|l| {
                let t = l.trim_start();
                (t.starts_with("fn ")
                    || t.contains(" fn ")
                    || t.starts_with("pub fn ")
                    || t.starts_with("async fn "))
                    && !t.starts_with("//")
            })
            .map(|l| l.trim().to_string())
    }

    /// Lines from the first `#[cfg(test)]` on are tests, which run on a runtime.
    fn production_part(text: &str) -> &str {
        text.find("#[cfg(test)]").map_or(text, |at| &text[..at])
    }

    fn scan(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                scan(&path, found);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read file");
            let lines: Vec<&str> = production_part(&text).lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim_start().starts_with("//") || !CALLS.iter().any(|c| line.contains(c)) {
                    continue;
                }
                let sig = enclosing_fn(&lines, i).unwrap_or_default();
                if !sig.contains("async fn") {
                    found.push(format!("{}:{}: in `{}`", path.display(), i + 1, sig));
                }
            }
        }
    }

    #[test]
    fn every_direct_tokio_spawn_is_inside_an_async_fn() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        scan(&src, &mut found);
        assert!(
            found.is_empty(),
            "a direct Tokio spawn outside an async fn panics when no runtime is current; \
             use tauri::async_runtime instead:\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn the_check_catches_a_spawn_in_a_plain_fn() {
        let text =
            "fn setup() {\n    let x = 1;\n    tokio::task::spawn_blocking(move || {});\n}\n";
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(enclosing_fn(&lines, 2).as_deref(), Some("fn setup() {"));
        let text = "pub async fn cmd() {\n    tokio::task::spawn_blocking(move || {});\n}\n";
        let lines: Vec<&str> = text.lines().collect();
        assert!(enclosing_fn(&lines, 1).unwrap().contains("async fn"));
    }
}
