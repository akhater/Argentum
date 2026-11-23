//! Which `#[ignore]`d tests CI runs on its software GPU.
//!
//! A test that needs a GPU and nothing else (no photos, no models, no
//! environment variable, no stopwatch) is written like this:
//!
//! ```ignore
//! #[test]
//! #[ignore = "needs a GPU; run with --ignored"]
//! fn gpu_what_it_proves() { ... }
//! ```
//!
//! Two things mark it, and the test below keeps them together: that exact
//! reason, and a name that starts with `gpu_`. `app.yml` runs the ignored tests
//! whose path contains `::gpu_` (after `cargo test --lib`, on Mesa's software
//! Vulkan driver), so a new test written this way runs in CI without anyone
//! editing the workflow.
//!
//! Run the set one test at a time, here and in CI:
//!
//! ```bash
//! cargo test --lib -- --ignored ::gpu_ --test-threads=1
//! ```
//!
//! Two of them, `gpu_panning_matches_sharpening_it_all_at_once` and
//! `gpu_the_mask_view_and_the_drag_preview_keep_the_result` in `sharpen_gpu`,
//! were written to be run that way (their reasons used to say so). The others
//! do not need it, but the set has one command and `app.yml` uses it.
//!
//! A test that needs more than a GPU says so in its own reason ("reads local
//! photos and needs a GPU", "timing ...") and keeps an ordinary name: it is
//! not in the set. A GPU test that cannot pass on a software GPU is left out
//! by name, with the reason, in `app.yml`.

/// The one reason every GPU-only test gives. Attributes take literals only, so
/// the attribute repeats it and the test below is what ties the two.
#[cfg(test)]
const REASON: &str = "needs a GPU; run with --ignored";

/// The name prefix `app.yml`'s filter (`::gpu_`) selects.
#[cfg(test)]
const PREFIX: &str = "gpu_";

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Every `fn` in `text` with the attribute lines (and the comments between
    /// them) that sit directly above it.
    fn functions(text: &str) -> Vec<(String, Vec<&str>)> {
        let mut found = Vec::new();
        let mut attrs: Vec<&str> = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("#[") {
                attrs.push(t);
            } else if t.starts_with("//") {
                // A comment between attributes does not end the block.
            } else if let Some(rest) = t
                .strip_prefix("fn ")
                .or_else(|| t.strip_prefix("pub fn "))
                .or_else(|| t.strip_prefix("pub(crate) fn "))
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                found.push((name, std::mem::take(&mut attrs)));
            } else {
                attrs.clear();
            }
        }
        found
    }

    #[test]
    fn the_reason_and_the_name_go_together() {
        let want = format!("#[ignore = \"{REASON}\"]");
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        assert!(!files.is_empty());

        let mut gpu_only = 0;
        let mut wrong = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file).unwrap();
            for (name, attrs) in functions(&text) {
                let reason = attrs.contains(&want.as_str());
                let named = name.starts_with(PREFIX);
                let ignored = attrs.iter().any(|a| a.starts_with("#[ignore"));
                let is_test = attrs.contains(&"#[test]");
                let in_gpu_module = file
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().starts_with(PREFIX));
                if is_test && ignored && in_gpu_module && !reason {
                    wrong.push(format!(
                        "{}: `{name}` sits in a module whose name starts `{PREFIX}`, so CI selects it, but its ignore reason is not exactly `{REASON}`",
                        file.display()
                    ));
                }
                if reason && !named {
                    wrong.push(format!(
                        "{}: `{name}` gives the GPU-only reason but is not named `{PREFIX}...`, so CI would not run it",
                        file.display()
                    ));
                }
                if is_test && ignored && named && !reason {
                    wrong.push(format!(
                        "{}: `{name}` is named `{PREFIX}...` but its ignore reason is not exactly `{REASON}`",
                        file.display()
                    ));
                }
                if is_test && !ignored && named {
                    wrong.push(format!(
                        "{}: `{name}` is named `{PREFIX}...` but is not #[ignore]d: it would fail wherever there is no GPU",
                        file.display()
                    ));
                }
                if reason && named {
                    gpu_only += 1;
                }
            }
        }
        assert!(wrong.is_empty(), "\n{}\n", wrong.join("\n"));
        assert!(gpu_only > 0, "no GPU-only test found: the scan is broken");
    }
}
