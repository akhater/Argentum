# Adding a tool

**New tools go in the engine.** Follow
[src-tauri/engine/README.md](../src-tauri/engine/README.md): one file in
`src/tools/`, one line in `src/registry.rs`, and its tests. The engine builds
and tests on its own, without the app, and nothing in it is shared with
RapidRAW's files.

Name a tool by where it came from: `dt_` darktable, `rt_` RawTherapee, `rr_`
RapidRAW, `rm_` RAWmakase, `ag_` Argentum (also `gimp_`, `gmic_`, `paper_`).
Put the source file and commit in a comment above anything copied.

The rest of this page is for the tools still in today's pipeline
(`src-tauri/src/mods/` and `shaders/modules.wgsl`), until they move into the
engine. It is also the map of where those pieces live.

---

## What today's pipeline allows

Every file is ours to change, but a tool in today's pipeline keeps its code in
files of ours and touches an inherited file only to call it:

| Piece                               | Where it goes                                                                                                                                                                              |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The math                            | `src-tauri/src/shaders/modules.wgsl` (ours), called from one of the existing stages: `ag_stage_scene_linear`, `ag_stage_color`, `ag_stage_tone`, `ag_stage_grade` or `ag_stage_display`    |
| A whole-image GPU pass on the input | inside `src-tauri/src/mods/input_stage.rs`                                                                                                                                                 |
| A setting the render needs          | a field in the adjustments struct (`image_processing.rs`) and the matching WGSL struct in `shader.wgsl`, kept in step; a tool that needs many new settings belongs in the engine           |
| The control                         | today's panels (`src/components/panel/`, `src/components/adjustments/`) are ours to change in place; a component of its own goes in `src/argentum/`. Strings go in `src/argentum/locales/` |

If a tool cannot be built that way, that is the signal to build it in the
engine.

---

## 1. The math → `src-tauri/src/shaders/modules.wgsl` (ours)

Find darktable's version. GPU code lives in `data/kernels/*.cl`; if there's no
kernel for it, the math is in `src/iop/<name>.c`.

Translate it to WGSL and add it to our file. Wrap it in the color conversion:

```wgsl
// from darktable src/iop/<name>.c @ <commit>
fn dt_something(color: vec3<f32>, ...) -> vec3<f32> {
    let wide = AG_SRGB_TO_PROPHOTO * color;    // in
    // darktable's math here
    return AG_PROPHOTO_TO_SRGB * wide;         // out
}
```

The matrices in `modules.wgsl` today are sRGB ↔ ProPhoto and sRGB → V-Gamut.
The XYZ and Bradford (LMS) pairs were retired with our white balance in
`26.41.6`. If a module needs a space that is not there, add the matrix pair
beside the others — don't convert inline.

OpenCL → WGSL is mostly mechanical:

| OpenCL                 | WGSL               |
| ---------------------- | ------------------ |
| `float4`               | `vec4<f32>`        |
| `(float3)(a,b,c)`      | `vec3<f32>(a,b,c)` |
| `native_powr` / `powr` | `pow`              |
| `fmax` / `fmin`        | `max` / `min`      |
| `clamp`, `mix`, `dot`  | same               |

## 2. Turn it on → inside a stage in `modules.wgsl` (ours)

The inherited `main()` already calls our five stages, one line each. Call your
function inside the stage where it belongs:

```wgsl
fn ag_stage_scene_linear(color: vec3<f32>, ...) -> vec3<f32> {
    var c = color;
    // ... the tools already here ...
    c = dt_something(c, ...);
    return c;
}
```

No new line in `shader.wgsl` is needed. Its functions stay where they are while
anything still uses them.

## 3. The settings

Only if the tool needs settings the app doesn't already have. A setting or two
is a field in the adjustments struct; a tool with many settings of its own
belongs in the engine.

When a field is added, the Rust struct in `image_processing.rs` and the WGSL
struct must match exactly — same fields, same order — or the GPU reads garbage.

## 4. The control → today's panels, or `src/argentum/` (both ours)

Today's interface is ours to change in place, and its panels are being swapped
one at a time for ones built from the engine's tool registry. A control goes
straight into the panel it belongs in; a component of its own goes in
`src/argentum/` and is imported there. The older `data-argentum` markers
(`src/argentum/Argentum.tsx`) are being replaced by direct edits, so don't add
new ones. Strings go in `src/argentum/locales/en.json`.

---

## Check it

```bash
npm run tauri dev
```

Open a RAW, drag the slider. If the picture moves and doesn't go neon green,
the wiring is right.

Then judge it properly: same photo, same setting, side by side against
darktable. It won't match to the pixel — different pipelines — but the
character should be recognisably darktable's.

```bash
npm run check:sources
npm run check:frontend
npm run check:wiring
npm run check:fmt
npm run check:ui
```

The first fails if wiring a feature depends on is gone; the second if types,
lint or formatting got worse than `scripts/frontend-baseline.json`; `check:fmt`
if a Rust module in `src-tauri/src/mods/` is not rustfmt-clean; the third
if the interface calls a command or shortcut that is not wired, or the
dependencies changed without `scripts/wiring-baseline.json` saying so. The
last opens the interface in headless Edge or Chrome over a stub of the app,
clicks every menu, tab, Settings section and default shortcut, and fails on a
page error, a raw translation key, or a control that appeared or went against
`scripts/ui-walk/ui-reachability.json` (`-- --update` rewrites it; the diff
shows what a change added or removed).

---

## Rules

- **Delete what nothing uses,** in its own change, with a CHANGELOG "Removed" line.
- **One tool per commit.** Makes it obvious what to undo when something looks wrong.
- **Note where you got it.** A comment with the source file and commit hash,
  so a year from now you can diff against it:

  ```wgsl
  // from darktable data/kernels/filmic.cl @ a1b2c3d
  ```

---

## Roughly how long

|                                               |                                                     |
| --------------------------------------------- | --------------------------------------------------- |
| First tool                                    | a few days — includes building the color conversion |
| After that                                    | 1–2 days each                                       |
| Tools needing multiple passes (blur, denoise) | ~a week                                             |
