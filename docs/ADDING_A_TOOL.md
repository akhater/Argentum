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

Our lines in RapidRAW's files may only go down (the ratchet in
`scripts/upstream-ratchet.json`, enforced by `npm run check:merge`). So a tool
in today's pipeline adds nothing to a file of theirs:

| Piece | Where it goes |
|---|---|
| The math | `src-tauri/src/shaders/modules.wgsl` (ours), called from one of the existing stages: `ag_stage_scene_linear`, `ag_stage_tone` or `ag_stage_display` |
| A whole-image GPU pass on the input | inside `src-tauri/src/mods/input_stage.rs` |
| A setting the render needs | the render path (`image_processing.rs`) is ours to change. The matching WGSL struct is in their `shader.wgsl`, so a new field there is a line of ours in their file and has to be paid for by removing one; a tool that needs new settings belongs in the engine |
| The control | a component in `src/argentum/`, mounted through an existing `data-argentum` marker; strings in `src/argentum/locales/` |

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
    let xyz = AG_SRGB_TO_XYZ * color;    // in
    // darktable's math here
    return AG_XYZ_TO_SRGB * xyz;         // out
}
```

The matrices already exist in `modules.wgsl`: sRGB ↔ XYZ, and XYZ ↔ Bradford
cone space (`AG_XYZ_TO_LMS` / `AG_LMS_TO_XYZ`) for anything that adapts between
illuminants. If a module needs a space we don't have yet, add the matrix pair
there — don't convert inline.

OpenCL → WGSL is mostly mechanical:

| OpenCL | WGSL |
|---|---|
| `float4` | `vec4<f32>` |
| `(float3)(a,b,c)` | `vec3<f32>(a,b,c)` |
| `native_powr` / `powr` | `pow` |
| `fmax` / `fmin` | `max` / `min` |
| `clamp`, `mix`, `dot` | same |

## 2. Turn it on → inside a stage in `modules.wgsl` (ours)

Their `main()` already calls our three stages, one line each. Call your
function inside the stage where it belongs:

```wgsl
fn ag_stage_scene_linear(color: vec3<f32>, ...) -> vec3<f32> {
    var c = color;
    // ... the tools already here ...
    c = dt_something(c, ...);
    return c;
}
```

No line in their `shader.wgsl`. Their functions stay where they are while
anything still uses them.

## 3. The settings

Only if the tool needs settings the app doesn't already have. See the table
above: a field in their WGSL struct is paid for with a line removed from that
file, so a tool with settings of its own is usually an engine tool.

When a field is added, the Rust struct in `image_processing.rs` and the WGSL
struct must match exactly — same fields, same order — or the GPU reads garbage.

## 4. The control → `src/argentum/` (ours)

A component of ours, placed through an existing `data-argentum` marker in their
panel (see `src/argentum/Argentum.tsx`), never a new line in their component.
Strings go in `src/argentum/locales/en.json`.

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
npm run check:merge
```

It fails if the tool added a line to a file of theirs, and says which.

---

## Rules

- **Never restructure a file of theirs.** Don't move their lines around.
- **Nothing of theirs is deleted while something still uses it.** Once nothing
  does, it is deleted and noted in that release's upstream review.
- **One tool per commit.** Makes it obvious what to undo when something looks wrong.
- **Note where you got it.** A comment with the source file and commit hash,
  so a year from now you can diff against it:

  ```wgsl
  // from darktable data/kernels/filmic.cl @ a1b2c3d
  ```

---

## Roughly how long

| | |
|---|---|
| First tool | a few days — includes building the color conversion |
| After that | 1–2 days each |
| Tools needing multiple passes (blur, denoise) | ~a week |
