# rm-selection

RAWmakase's selection models in Argentum: one click finds the photo's people and
animals (Subject), its sky (Sky), or everything else (Background), and clicks or
a box select any object (Object).

- **Source:** `crates/rawmakase-inference` from
  [RAWmakase](https://github.com/pch/rawmakase) at commit `7c3c81a`, MIT
  (`LICENSE`). RAWmakase 0.4.0 changed none of these files.
- **Models:** SAM 2.1 Hiera small (Meta AI, Apache-2.0; onnx-community export)
  and DETR ResNet-50 panoptic (Facebook AI Research, Apache-2.0; Xenova export).
  The files, sizes and checksums are in `src/manifest.rs`.

## What changed from RAWmakase

Each change is marked with a comment starting `Argentum:`.

- `runtime.rs`: written for `ort` 2.0.0-rc.10, the app's version, instead of
  rc.13. That means provider names in full (`CPUExecutionProvider`),
  `GraphOptimizationLevel::Level3` for `All`, and `init_from` returning its
  builder directly. A prompt of points alone passes its empty box tensor
  allocated, because rc.10 refuses a zero-sized dimension built from raw data.
- `runtime.rs`: `private_environment`, RAWmakase's later telemetry fix (`4b8d6d04`),
  so the environment is built with telemetry off. The app builds its own
  environment through it too (`src/mods/ai_session.rs`).
- Every file starts with a line naming its source.

Both models load and run on the app's ONNX Runtime 1.22 with these changes;
tested on 45 RAWs on 2026-10-19.

## Tests

`cargo test` here runs the model-free tests, and `tests/telemetry.rs`, which
loads the app's `../resources` runtime when the app has been built. `tests/real_model.rs` needs the
models and a runtime: set `RAWMAKASE_TEST_MODEL` to the folder holding the five
files and `RAWMAKASE_ORT_LIB` to `onnxruntime.dll`, then
`cargo test -- --ignored`.
