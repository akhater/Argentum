// The anchors: how many lines of each file upstream owns may mention Argentum,
// and - where it matters - which lines those must be.
//
// Its own module because two things read it: `check-mergeability.mjs`, which
// enforces it, and `test-upstream-checks.mjs`, which tests that the enforcement
// works. Importing the checker from its own test would run every gate as a side
// effect of the import.

/**
 * THE ANCHORS — the real rule, of which the budgets above are only a backstop.
 *
 * A line budget answers "how much of their code have we touched". It does not
 * answer the question that decides whether this fork is still alive in two
 * years: **what does the next feature cost?**
 *
 * If every feature adds a line to one of their files, the budget is a countdown.
 * Four commands is four lines of `lib.rs` and looks free; a hundred is a hundred
 * lines in the file upstream also edits every release.
 *
 * So each of their files gets a *fixed* number of hooks into our code — an
 * anchor — and everything after that is routed through it on our side. The
 * count below is the number of lines in that file that may mention Argentum.
 * It is not a budget to spend. It does not go up when a feature is added,
 * because a feature must not need it to.
 *
 * Rebrand lines (`.agdata`, "Argentum" inside their own sentences) are counted
 * separately and ignored here: they happened once and do not grow.
 *
 * The render path and today's interface (RENDER_PATH and INTERFACE in
 * upstream-registry.mjs) are ours to change and have no anchor count. An entry
 * for one of those files carries only `requires`.
 */
export const ANCHORS = [
  {
    file: 'src-tauri/src/file_management.rs',
    hooks: 1,
    what:
      'the first thumbnail while browsing, in generate_single_thumbnail_and_cache: '
      + 'the camera\'s JPEG before the thumbnail is made',
    instead: 'do more inside mods/first_thumbnail.rs',
  },
  {
    file: 'src-tauri/src/shaders/display.wgsl',
    hooks: 1,
    what: 'one call to ag_stage_present, the presentation stage',
    instead: 'add your tool inside ag_stage_present in shaders/ag_display.wgsl',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    // Three since 2026-10-17, by a recorded decision (DEC-27): the tone
    // stage, one call right after their tone-mapper chain. Until then the RAW
    // tone rendering was two blocks of logic inside their main() - a condition
    // on their chain and a per-channel curve after it - which mentioned nothing
    // of ours and so counted against no anchor. DNG Standard needed more than a
    // per-channel curve; the blocks came out and their chain is theirs again.
    hooks: 3,
    what: 'one call to ag_stage_scene_linear, one to ag_stage_tone, one to ag_stage_display',
    instead: 'add your tool inside one of those three stages in shaders/modules.wgsl',
  },
  {
    file: 'src-tauri/src/raw_processing.rs',
    hooks: 1,
    what: 'one call to mods::decode::on_raw_decoded',
    instead: 'add a step to mods/decode.rs',
  },
  {
    // The render path (RENDER_PATH in upstream-registry.mjs) is ours to change,
    // so this file has no hook count. The wiring check stays: it is what notices
    // a change that silently stops sharpening from reaching their passes.
    file: 'src-tauri/src/gpu_processing.rs',
    requires: [
      {
        // The use of the result mentions nothing of ours, so a merge that
        // resolves it back to `&cache.texture_view` passes every other gate:
        // sharpening would run, and nothing would read what it made.
        pattern: /staged\.as_ref\(\)\.unwrap_or\(&cache\.texture_view\)/,
        why: 'their render has to read what the input stage returns, or sharpening silently does nothing',
      },
    ],
  },
  {
    // Still one, and it stays one: a single `use crate::mods::{...}` block. The
    // metadata writer joined it rather than bringing its own line, because two
    // imports here would be two features each holding a piece of their file.
    file: 'src-tauri/src/export_processing.rs',
    hooks: 1,
    // The old wording named render_high_precision, which this import has never
    // held - it has render_for_export and render_for_estimate. Corrected here
    // rather than left, because an anchor nobody can check against the file is
    // a comment, not a gate.
    what:
      'the one import block: Precision, encode_tiff, overlay_preserving_precision, '
      + 'render_for_export, render_for_estimate, write_export_metadata',
    instead:
      'precision: which format gets which is decided in Precision::for_path. '
      + 'metadata: mods/export_metadata.rs - their call already routes through it',
    requires: [
      {
        // The call is not a hook - it mentions no module of ours - so nothing
        // else notices if an upstream merge resolves it back to theirs. A TIFF
        // would export with no metadata, every gate green, and the only symptom
        // is a missing tag nobody looks for.
        pattern: /write_export_metadata\(/,
        why: 'the export has to go through ours or a TIFF silently loses its metadata again',
      },
    ],
  },
  {
    // One import, as in export_processing.rs. Their two shape masks each hand
    // their straight ramp to ours, one line apiece, and every later change to
    // how a mask fades belongs in that module, not in another line here.
    file: 'src-tauri/src/mask_generation.rs',
    hooks: 1,
    what: 'the import of mods::mask_falloff, which shapes the linear and radial fall-off',
    instead: 'change the curve in mods/mask_falloff.rs',
    requires: [
      {
        // The calls mention no module of ours, so a merge that resolves either
        // line back to theirs passes every other gate: that mask goes back to
        // a straight ramp with a line at each handle, and nothing says so.
        pattern: /mask_falloff::linear\(/,
        why: 'the linear mask has to fade through ours or it gets its hard edges back',
      },
      {
        pattern: /mask_falloff::radial\(/,
        why: 'the radial mask has to fade through ours or its feather gets a hard inner edge back',
      },
    ],
  },
  {
    // Taken 2026-10-09, from a budget of zero, by AK's decision (DEC in the
    // brain). Their AI sessions keep ONNX Runtime's CPU memory arena, so a model
    // keeps the most working memory it ever needed for as long as it is loaded:
    // the five mask models held 6.1 GB after one selection on a 32 MP raw. The
    // arena can only be turned off where a session is built, and every session
    // is built here, so there is no shape without a line of theirs. One import,
    // as in mask_generation.rs; the eight builder calls name only the imported
    // function, so they are one-for-one swaps rather than hooks.
    file: 'src-tauri/src/ai_processing.rs',
    hooks: 1,
    what: 'the import of mods::ai_session::session_builder, which every model session is built with',
    instead: 'change how sessions are built in mods/ai_session.rs',
    requires: [
      {
        // The calls mention no module of ours, so a merge that resolves one back
        // to `Session::builder()` passes every other gate, and that model quietly
        // keeps its arena again. The SAM encoder is the one that costs most.
        pattern: /session_builder\(\)\?\.commit_from_file\(encoder_path\)/,
        why: 'the mask models have to be built through ours or they keep gigabytes of working memory again',
      },
      {
        pattern: /session_builder\(\)\?\.commit_from_file\(depth_path\)/,
        why: 'and the depth model, the other large one in the same set',
      },
    ],
  },
  {
    // Today's interface is ours to change in place (INTERFACE in
    // upstream-registry.mjs), so this file has no hook count. The wiring check
    // stays: the three lines below make the size estimate follow the bit-depth
    // control, and deleting any one of them silently restores a bug that every
    // test here once passed through.
    //
    // Each pattern names only an identifier of ours, so an upstream rename of
    // their state, their effect or their debounce cannot fail this. The only
    // thing that can is the wiring going away, which is the event worth failing
    // for.
    file: 'src/components/panel/right/ExportPanel.tsx',
    requires: [
      {
        pattern: /from\s+['"][^'"]*argentum\/exportDepth['"]/,
        why: 'their estimate effect has to import the depth',
      },
      {
        pattern: /\buseTiffDepth\(\)/,
        why: 'and read it',
      },
      {
        pattern: /\[[^\]]*\btiffDepth\b[^\]]*\]\s*\)/,
        why:
          'and list it as a dependency. Without this the size estimate froze on '
          + 'the depth you had before — found by AK on 2026-09-13, after four '
          + 'review passes and a full green suite missed it',
        // The trailing `\s*\)` is load-bearing: without it any future bracketed
        // mention of the name — an index, a tuple, `// see [tiffDepth]` in a
        // comment — satisfies the pattern while the dependency itself is gone.
        // An array literal that closes a call is the shape of a hook dependency
        // list and of nothing else in that file.
      },
    ],
  },
  // Behaviour, not UI. A portal can add a control; it cannot change what
  // happens when the user clicks one of theirs. These replace the body of an
  // existing handler, so they are one-time replacements rather than additions —
  // if one of these ever needs a *second* hook, the injection is in the wrong
  // place and should become an event our code listens for.
  {
    file: 'src-tauri/src/exif_processing.rs',
    hooks: 1,
    what: 'the lens name, read from any file format when their read leaves it blank',
    instead: 'change mods/lens_name.rs',
  },
  {
    file: 'src-tauri/src/lens_correction.rs',
    hooks: 1,
    what: 'matching a lens profile for the body that shot the frame',
    instead: 'change mods/lens_crop.rs',
  },
  {
    // Taken deliberately on 2026-09-15, from a budget of zero. See DEC in the
    // brain. The anchor is not for one camera: it is for "the extension is not
    // the answer". Canon's 2002 1D and 1Ds wrote raws called .TIF, and so did
    // Kodak, Leaf and Phase One backs. Every one of those costs zero further
    // lines of theirs — they are decided inside is_camera_raw.
    //
    // There is no shape without it. is_raw_file is theirs, it is a pure
    // extension test, and it is asked from thirty-odd places; nothing outside
    // their file can change its answer. Hooking only the loader costs the same
    // permission and fixes only the picture, leaving export and EXIF still
    // believing the file is not a raw.
    file: 'src-tauri/src/formats.rs',
    hooks: 1,
    what: 'asking the contents when the extension cannot say - a .tif may be a raw',
    instead: 'change mods/tif_raw.rs, which decides every ambiguous extension',
    requires: [
      {
        // Counting is not enough here. If an upstream merge resolves this line
        // back to theirs the hook count becomes zero, which is under the budget,
        // and every gate goes green - while a 1Ds quietly shows its 288x192
        // thumbnail as the photograph again. Silent, and exactly the failure the
        // anchor was taken for.
        pattern: /tif_raw::is_camera_raw\(/,
        why: 'without it a .TIF raw silently opens as its embedded thumbnail',
      },
    ],
  },
];
