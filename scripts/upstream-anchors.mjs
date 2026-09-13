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
 */
export const ANCHORS = [
  {
    file: 'src-tauri/src/lib.rs',
    hooks: 5,
    what:
      '`mod mods`, the cache-version check, the single `ag` command, the display '
      + 'conversion for the screen the window is on, and the refresh when the window '
      + 'moves to another screen',
    instead: 'add a match arm to mods/dispatch.rs — commands cost nothing here',
  },
  {
    file: 'src-tauri/src/shaders/display.wgsl',
    hooks: 1,
    what: 'one call to ag_stage_present, the presentation stage',
    instead: 'add your tool inside ag_stage_present in shaders/ag_display.wgsl',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    hooks: 2,
    what: 'one call to ag_stage_scene_linear, one to ag_stage_display',
    instead: 'add your tool inside one of those two stages in shaders/modules.wgsl',
  },
  {
    file: 'src-tauri/src/raw_processing.rs',
    hooks: 1,
    what: 'one call to mods::decode::on_raw_decoded',
    instead: 'add a step to mods/decode.rs',
  },
  {
    // One import, and it has to stay one. Everything the export render does
    // differently is reachable from the Precision value it carries: the storage
    // format, the shader text, the pipeline constant that silences the dither,
    // the bytes per pixel of the readback. A second hook here would mean some of
    // that decision had been written into their file instead of ours.
    file: 'src-tauri/src/gpu_processing.rs',
    hooks: 1,
    what: 'the import of Precision, which the processor carries and reads from',
    instead: 'add a method to mods/export_precision.rs - the processor already has a Precision',
  },
  {
    file: 'src-tauri/src/export_processing.rs',
    hooks: 1,
    what: 'the import of Precision and render_high_precision',
    instead: 'which format gets which precision is decided in Precision::for_path - change it there',
  },
  {
    file: 'src-tauri/src/image_processing.rs',
    hooks: 4,
    what: 'the CPU preview encode interception, and the clipping view mode',
    instead: 'change mods/preview_encode.rs, or mods/clipping.rs',
  },
  {
    file: 'src/App.tsx',
    hooks: 2,
    what: 'the single <Argentum /> mount',
    instead: 'add a portal in src/argentum/Argentum.tsx',
  },
  // UI markers. One per panel, never one per feature: everything Argentum shows
  // in that panel portals into the same marker.
  {
    file: 'src/components/panel/right/MetadataPanel.tsx',
    hooks: 1,
    what: 'the data-argentum="camera-details" marker',
    instead: 'portal into [data-argentum="camera-details"] from Argentum.tsx',
  },
  {
    // Two, and two is the ceiling: a panel has an inline slot (in a heading
    // row, for buttons) and a block slot (below the controls, for a section).
    // Those are positions, not features — every Argentum colour control mounts
    // into one of them. A third would mean a feature bought its own, which is
    // the growth this whole file exists to prevent.
    file: 'src/components/adjustments/Color.tsx',
    hooks: 2,
    what: 'the color-tools (inline) and camera-profile (block) markers',
    instead: 'portal into one of the two existing markers from Argentum.tsx',
  },
  {
    // One marker, and their file renders it only while TIFF is the chosen
    // format — so the control appears and disappears without Argentum reading
    // any state of theirs, and without a second hook to tell it when to show.
    // Upstream's own version of this feature (#1466) instead puts the setting on
    // ExportSettings and threads it through six of their files; this is the same
    // feature for one line.
    file: 'src/components/panel/right/ExportPanel.tsx',
    hooks: 2,
    // Counting the hooks proves they are not too many. It does not prove they
    // still *work*: the three lines below are what make the size estimate
    // follow the bit-depth control, and deleting any one of them silently
    // restores a bug that every test here passed through. AK found it by using
    // the app; nothing else did.
    //
    // Each pattern names only an identifier of ours, so an upstream rename of
    // their state, their effect or their debounce cannot fail this. The only
    // thing that can is the wiring going away, which is the event worth failing
    // for.
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
    what:
      'the data-argentum="export-precision" marker rendered only for TIFF, and the '
      + 'useTiffDepth import that lets their size estimate see the depth change',
    instead:
      'portal into the marker from Argentum.tsx — every future export control mounts '
      + 'there. The second hook is not a second feature and does not go up again: '
      + 'estimatedSize is useState inside their component and the debounced estimator '
      + 'is a useMemo beside it, so an effect dependency is the only way in. Any later '
      + 'control publishes into src/argentum/exportDepth.ts and rides the same one.',
  },
  {
    file: 'src/components/panel/SettingsPanel.tsx',
    hooks: 1,
    what: 'the About tab, an empty div Argentum fills',
    instead: 'add a section to src/argentum/AboutPanel.tsx — or a card of its own '
      + 'inside [data-argentum="settings-about"]',
  },
  // Behaviour, not UI. A portal can add a control; it cannot change what
  // happens when the user clicks one of theirs. These replace the body of an
  // existing handler, so they are one-time replacements rather than additions —
  // if one of these ever needs a *second* hook, the injection is in the wrong
  // place and should become an event our code listens for.
  {
    file: 'src/components/panel/editor/ImageCanvas.tsx',
    hooks: 2,
    what: 'the white balance picker solving in Rust',
    instead: 'change src/argentum/whiteBalance.ts',
  },
  {
    file: 'src/components/panel/right/CropPanel.tsx',
    hooks: 1,
    what: 'lens auto-detection running on load, not only on click',
    instead: 'change src/argentum/useAutoDetectOnLoad.ts',
  },
  {
    file: 'src-tauri/src/exif_processing.rs',
    hooks: 1,
    what: 'reading the lens name out of the maker note',
    instead: 'change mods/makernote_lens.rs',
  },
  {
    file: 'src-tauri/src/lens_correction.rs',
    hooks: 1,
    what: 'matching a lens profile for the body that shot the frame',
    instead: 'change mods/lens_crop.rs',
  },
];
