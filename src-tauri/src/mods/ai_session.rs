//! How every one of RapidRAW's AI model sessions is built: with ONNX Runtime's
//! CPU memory arena off.
//!
//! The arena keeps the largest working memory a model has ever needed for as
//! long as the session lives, so the next run need not ask for it again. Their
//! models live for the whole session, so their working memory did too. Measured
//! on 2026-10-09 with a Canon R6 Mark III raw: the five mask models (SAM's
//! encoder and decoder, U2-Net, the sky model, Depth Anything) took the app from
//! 3.3 GB to 8.5 GB after one selection, of which loading them was 1.4 GB, and
//! unloading them gave back 6.1 GB. Without the arena a run's working memory is
//! returned when the run ends, and a loaded model costs what its weights cost.
//!
//! The price is an allocation per tensor per run instead of reuse from the
//! pool. Against a model run that takes a second or more, that is noise.
//!
//! One import in their ai_processing.rs, on an anchor taken 2026-10-09: each of
//! their eight `Session::builder()?` became `session_builder()?`, one line for
//! one. Unloading models nothing is using stays in mods/memory.rs; this makes a
//! loaded one cheap, that makes an idle one free.

//!
//! Every session also goes through [`builder`], which first makes the process's
//! ONNX Runtime environment with telemetry off (`rm_selection`'s
//! `private_environment`). The environment is process-wide and the first one
//! made is the only one: a session built before it would have made `ort`'s
//! default, which reports to Microsoft. The test below keeps every session and
//! every environment in the app on this path.

use ort::execution_providers::CPUExecutionProvider;
use ort::session::Session;
use ort::session::builder::SessionBuilder;

/// Makes the process's ONNX Runtime environment, telemetry off, unless one is
/// already made (`Ok(false)`). Call it after `gpu_runtime` has chosen which
/// runtime loads: this loads it.
pub fn environment() -> ort::Result<bool> {
    rm_selection::runtime::private_environment(ort::init().with_name("Argentum")).commit()
}

/// `Session::builder()` on the private environment.
pub fn builder() -> ort::Result<SessionBuilder> {
    environment()?;
    Session::builder()
}

/// `Session::builder()`, with the CPU arena off.
pub fn session_builder() -> ort::Result<SessionBuilder> {
    builder()?.with_execution_providers([CPUExecutionProvider::default()
        .with_arena_allocator(false)
        .build()])
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under `dir`.
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// No ONNX Runtime environment or session in the app or in `rm_selection`
    /// can be made with telemetry on: environments are committed only through
    /// `private_environment`, and sessions are built only after one is.
    #[test]
    fn every_onnx_runtime_environment_is_made_with_telemetry_off() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        sources(&root.join("rm_selection").join("src"), &mut files);
        let init = concat!("ort::", "init");
        // Each of these makes `ort`'s default environment if none is made yet.
        let sessions = [
            concat!("Session::", "builder("),
            concat!("SessionBuilder::", "new("),
            concat!("get_", "environment("),
        ];
        let mut wrong = Vec::new();
        for file in &files {
            let text = fs::read_to_string(file).unwrap();
            let name = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            for (at, _) in text.match_indices(init) {
                let statement = text[at..].split(';').next().unwrap_or_default();
                let commits = statement.contains(".commit()");
                let private = text[..at].ends_with("private_environment(");
                if commits && !private {
                    wrong.push(format!("{name}: an environment committed without it"));
                }
            }
            // Here a session is built after `environment()`; in rm_selection
            // after `load_runtime`, which commits through `private_environment`.
            let allowed = ["src/mods/ai_session.rs", "rm_selection/src/runtime.rs"];
            let builds = sessions.iter().any(|s| text.contains(s));
            if builds && !allowed.contains(&name.as_str()) {
                wrong.push(format!("{name}: a session not built through `builder()`"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let runtime = fs::read_to_string(root.join("rm_selection/src/runtime.rs")).unwrap();
        assert!(runtime.contains("builder.with_telemetry(false)"));
    }
}
