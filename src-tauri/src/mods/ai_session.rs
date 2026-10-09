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

use ort::execution_providers::CPUExecutionProvider;
use ort::session::Session;
use ort::session::builder::SessionBuilder;

/// `Session::builder()`, with the CPU arena off.
pub fn session_builder() -> ort::Result<SessionBuilder> {
    Session::builder()?.with_execution_providers([CPUExecutionProvider::default()
        .with_arena_allocator(false)
        .build()])
}
