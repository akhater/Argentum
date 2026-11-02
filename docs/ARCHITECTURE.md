# Architecture

Argentum was forked from RapidRAW at 1.6.6. It is an application of its own,
changed one piece at a time, and it keeps working at every step:

> **New code goes in the engine. Everything inherited from RapidRAW is ours to
> change. RapidRAW is a source we harvest from, like darktable, and is never
> merged again.**

---

## Why the fork rule changed

A fork dies when your changes and upstream's land in the same lines of the
same files. Every update becomes an argument, you stop updating, and you are
maintaining a whole photo editor alone. For its first months Argentum answered
that with one rule: our code in our own files, as few lines as possible in
theirs, and every upstream commit that touched something we depend on reviewed
on its own.

By October 2026 the rule no longer held. Our lines were in 66 of RapidRAW's
files, about 2,000 of them, and the review of RapidRAW 1.6.5 recorded 1,242
per-commit decisions, 84% of them "not applicable". The effort went into
saying that unrelated commits were unrelated, while the code that renders a
photo, where Argentum differs most, was the code we were least free to change.

So the line moved: the code that renders a photo and the interface became ours
to change, and only their other files were still merged, once per release.

On 2026-10-25 that ended too. RapidRAW 1.6.7 rewrote its render path, and their
other files (export, files, AI, LUTs, masks) now call it. Over our render path a
merge gave about ninety compile errors, and taking it would have meant porting
some five thousand lines with their pixel changes mixed into the speed work.
Argentum stopped merging RapidRAW for good:

| Where | Rule |
|---|---|
| `src-tauri/engine/` | New tools: one file in `tools/`, one line in `registry.rs` (see its README) |
| Everything else: the files inherited at 1.6.6, `src-tauri/src/mods/`, the shaders, `src/argentum/`, the interface | Ours to change. New code still goes in a module of ours with one job, rather than growing an inherited file |

Inherited code is replaced only when it is dead or measurably worse, and what
nothing uses is deleted, one small change at a time. RapidRAW's later releases
are read and harvested by hand (see "Harvesting from RapidRAW" below).

---

## How that works for shaders

This is how the tools in today's pipeline reach the GPU. New tools go into the
engine instead (`src-tauri/engine/README.md`).

RapidRAW's image engine is one file, `src-tauri/src/shaders/shader.wgsl`.
Every tool is a function; `main()` calls them in order.

Our tools do not go into that file: it is pulled in with Rust's `include_str!`,
and our own shader file is glued in front of it at build time, so each tool of
ours lives in one place.

```rust
// mods/export_precision.rs — the one source every render compiles
pub const SHADER_SOURCE: &str = concat!(
    include_str!("../shaders/modules.wgsl"),   // ours
    include_str!("../shaders/shader.wgsl"),    // inherited from RapidRAW
);
```

A 16-bit export compiles the same text with its storage format rewritten
(`export_shader_source`), so preview and export never run different tools.

**`modules.wgsl` holds every function we harvest.** The main shader calls it
once per stage, and every tool of ours runs inside a stage (see THE PIPELINE
ANCHOR in `modules.wgsl`):

```wgsl
// in main():
composite_rgb_linear = ag_stage_scene_linear(composite_rgb_linear, /* profile rows */);
```

Those calls are among the wiring `npm run check:sources` guards: if one goes,
a stage's tools stop running without any error.

---

## Layout

```
src-tauri/
  engine/                OURS    — the processing engine: a tool registry, one file per tool
  rm_selection/          RAWMAKASE — the selection masks (SAM 2.1, DETR), taken whole
  tests/look/            OURS    — the golden photos: public RAWs rendered in CI
  src/
    shaders/
      shader.wgsl        INHERITED — the main shader, one call per stage into ours
      modules.wgsl       OURS      — harvested math for the current pipeline
    mods/                OURS      — our Rust code, until it moves into the engine
    *.rs                 INHERITED — came from RapidRAW 1.6.6, ours to change

src/
  argentum/              OURS
    modern/              OURS      — the window: top bar, columns, right panel tabs, Settings
  shell/                 OURS      — a test bench for the engine's contract, not a product
  App.tsx, components/   INHERITED — the panels, editor and views the window is built from
```

**Rule of thumb:** everything is ours to change, and new code goes in a module
of ours with one job. An inherited file gets the smallest change that calls
into it, so the 4,000-line files shrink over time instead of growing.

---

## The color space thing

RapidRAW works in linear sRGB. darktable's modules assume something wider or
more perceptual. Transplanted math needs converting in and back out.

That conversion lives in `modules.wgsl` and is **written once, reused by every
tool after** — not a per-tool cost.

**What is there today:** linear sRGB ↔ ProPhoto (`AG_SRGB_TO_PROPHOTO`,
`AG_PROPHOTO_TO_SRGB`) and sRGB → V-Gamut (`AG_SRGB_TO_V_GAMUT`, for the film
looks). The first set, built in `26.37.2` for white balance, was linear sRGB ↔
CIE XYZ plus XYZ ↔ Bradford cone space (LMS); it was retired in `26.41.6` with
our white balance, when RapidRAW 1.6.5 did the same in its own engine. The
pattern stands for the next tool that needs a space:

```wgsl
fn dt_something(color: vec3<f32>, ...) -> vec3<f32> {
    let wide = AG_SRGB_TO_PROPHOTO * color;    // in
    // ... darktable's math, largely as-is ...
    return AG_PROPHOTO_TO_SRGB * wide;         // out
}
```

There is **no `srgb_to_rec2020` and no `mods/color.rs`** — earlier drafts of
this document promised both. If a later harvested module needs another space
(XYZ, LMS, linear Rec2020), add its matrix pair to `modules.wgsl` beside the
others. The colour difference used to measure pixel changes (`lab`,
`delta_e2000`) is defined once, in the engine's `colour.rs`.

---

## Data and storage

- **Edits** — JSON sidecar next to each photo (`IMG_1234.CR3.agdata`).
  RapidRAW's format, renamed; we add fields to it, among them the photo's
  History (`agHistory`) and Snapshots (`agSnapshots`). Original RAW never
  modified.
- **Settings, AI models, presets, My Gear** — the operating system's
  application-data folder, under `co.argentum.editor`.
- **Thumbnails** — JPEGs in the application cache folder, kept across sessions.
- **Catalog** — planned, not built: an index (SQLite) to browse photos whose
  drive is unplugged. See `ROADMAP.md`.

The exact folders per system are in the
[README](../README.md#where-the-app-keeps-its-files). One user-chosen folder for
all of it is on the roadmap, not built.

**Stored files keep reading.** `src-tauri/fixtures/compat/<release>/` holds a
`settings.json` and a sidecar in the shape each release wrote them, and the
tests in `src-tauri/src/mods/compat_corpus.rs` (and
`src/argentum/compatCorpus.test.ts` for the interface's loader) fail when one no
longer reads, loses a key when written back, or gives the renderer something
other than `fixtures/compat/snapshots/<release>.txt` records. To add the
version being worked on, after a version bump or a new setting or edit key:
`cargo test --lib mods::compat_fixture -- --ignored` in `src-tauri`, then record
its snapshot. To rewrite the snapshots after a change meant to move them (and
bump `PIPELINE` in `mods/cache_version.rs` when it changes pixels):
`AG_COMPAT_UPDATE=1 cargo test --lib mods::compat_corpus`. A released folder is
never edited; a key dropped on purpose goes in the module's `DROPPED` list.

---

## Where a new feature goes

1. **A tool that changes pixels** goes in the engine: one file, one registry line,
   its tests (`src-tauri/engine/README.md`).
2. **Anything else of ours** goes in a module with one job: `src-tauri/src/mods/`
   for Rust, `src/argentum/` for the interface, `shaders/modules.wgsl` for math
   in today's pipeline.
3. **The inherited file** that has to reach it gets the smallest change that calls
   into ours: a call, an import, a mount point. Inherited files are ours to change,
   so this is a choice for clarity, not a rule about someone else's code.

Some of the early features were built without touching an inherited file at all
(a component mounted through a `data-argentum` marker, a store patched at import,
an entry spliced into an exported array). With merging over, that indirection
buys nothing: a direct, readable change is preferred, and the old workarounds are
replaced by direct edits one at a time, each with its test.

---

## Planned: naming the objects before masking them

Written down because the shape of it is decided and the cost is not where it
looks. The list is not built yet. See the Masking row in `ROADMAP.md`.

**What it is.** Since `26.43.1` masking is a click: Subject, Sky and Background
find themselves, and Object cuts what is clicked, on RAWmakase's selection crate
(`src-tauri/rm_selection`: DETR panoptic finds and labels, SAM 2.1 cuts). The
wanted behaviour goes one step further — look at the photo once, list what is in
it by name, tick the dog.

**What is already here.** DETR panoptic already labels what it finds in COCO's
classes, and SAM 2.1 already takes a box or points. `ort` (ONNX Runtime) runs
both, and `mods/model_download.rs` fetches a model, checks its SHA-256 and keeps
it, for the selection masks and the Settings page alike. So the first version
needs no new model: show DETR's labels as a list and hand SAM the chosen one's
box.

**Only for things outside those classes,** a naming and a locating model would
join it, Grounded-SAM's own arrangement:

| Step | Model | What it answers |
|---|---|---|
| Name | RAM++ | *what* is in this picture — a list of tags, no positions |
| Locate | Grounding DINO (or YOLO-World) | *where* the thing called "dog" is — a box |
| Cut | SAM 2.1 | the mask inside that box — **already shipped** |

RAM++ classifies and does not localise. That is the whole reason the middle row
exists, and it is the thing most likely to be misremembered later as "RAM++ gives
you objects". It does not.

**Where the picker goes.** In the Masks tab, which is ours to change. It
*creates* the mask rather than living inside the mask list, so the list keeps one
job.

**Open, and not to be guessed at.** Model licences and sizes (two more downloads
on top of SAM's). Whether the tags are worth storing per photo — they would make
library search work on content, which needs somewhere to put them, and the
catalogue index is the obvious place and does not exist yet.

---

## Harvesting from RapidRAW

RapidRAW is read once per release and nothing is merged. `npm run check:sources`
warns when a release newer than the one named on the CHANGELOG's "read up to"
line is out.

1. **Read the release** from the last one read to the new tag: the release
   notes, the commit list, and their shader changes first, the most valuable
   and the easiest to copy.
2. **One line per feature** with a verdict: **copy** (into a file of ours, with
   the source commit in a comment), **skip** (not wanted: cloud, sync, their
   library), or **decide** (they built something we have).
3. **Measure pixel changes.** A change to how photos look is rendered on a set
   of test RAWs against today's render and compared per pixel (CIEDE2000); the
   numbers decide.
4. **Each copy is its own pull request,** with its tests and a CHANGELOG entry
   naming the source file and commit. Then the "read up to" line moves.

Three lessons from the merging years still hold:

- **They may build what we built.** When a release changes a feature we also
  have, say so and decide between the two on evidence. No diff detects it.
- **Review the interface as well as the processing.** Keeping our processing
  does not mean rejecting a better control or workflow of theirs.
- **A paired change comes over whole.** When their change removes something and
  adds its replacement elsewhere (a new event shape, a renamed setting), copy
  both halves or neither.

---

## Where code comes from

Nothing in this design is darktable-specific. `modules.wgsl` holds *harvested*
math — the source doesn't matter, only that it's a formula we can express as a
shader function.

Name functions by where they came from, so it stays obvious a year later:

| Prefix | Source | Good for |
|---|---|---|
| `dt_` | darktable | color science, tone mapping, denoise, highlight recovery |
| `rt_` | RawTherapee | demosaic, highlight recovery, capture sharpening — often the strongest here |
| `rm_` | RAWmakase | Lightroom-matched looks, B&W, Color Grading, lens readers, selection masks |
| `rr_` | RapidRAW | a tool taken from a RapidRAW release after 1.6.6 |
| `ag_` | Argentum | our own, where no source had it |
| `gimp_` | GIMP / GEGL | pixel-level effects, blend modes, distortions |
| `gmic_` | G'MIC | huge filter library, film looks, stylisation |
| `paper_` | a published paper | when nobody's implemented it yet |

Honest note on GIMP: it's a *pixel editor*, not a RAW developer, so it has less
to offer at the RAW end than darktable does. Its useful parts are GEGL
operations — self-contained image effects, which is exactly the shape we want.
Worth raiding for creative effects, not for color science.

For anything RAW-specific that darktable doesn't win outright, look at
**RawTherapee** before GIMP. Its highlight recovery and demosaic are
best-in-class, and it's clean C++.

**Same recipe regardless of source.** See [ADDING_A_TOOL.md](ADDING_A_TOOL.md).

**One rawler for both apps.** Argentum and Shoebox pin `rawler` from the same
fork, and the pins move to the same commit. `npm run check:sources` warns when
they differ. It never blocks, so neither app's release waits for the other.

---

## Where things live

Everything for development lives in the checkout: source, `node_modules` and
Rust's build output (the running app keeps its data in the operating system's
folders, as above). Git never tracks a binary, so pushing is the only backup the
source needs.

**Keep the checkout out of cloud-synced folders.** A sync client such as
OneDrive does not tolerate directory junctions inside a synced folder: it
replaced a `node_modules` junction with a real directory (reparse tag
`0x9000e01a`, its own cloud-files tag), began syncing 182 npm packages
(239MB), and held them locked while uploading, so they could not be removed
until the sync client was stopped. A bare repository in a synced folder works
as a backup remote.

**Leave Rust's `target/` where Tauri puts it.** A `.cargo/config.toml` once
redirected `target-dir`, and the redirect put `target/` at the repository
root. Vite watches the project directory and only ignores `**/src-tauri/**`,
so it tried to watch 819 crates worth of build artifacts, hit a build script
`.exe` that cargo had open, and crashed with `EBUSY` - which killed the whole
dev command. Tauri's default `src-tauri/target` is already inside Vite's
ignore list and already covered by `.gitignore`.

Default tool behaviour is correct; adding a redirect back is how both of these
breakages happened. `.gitignore` is the safety net: if you add a dependency
that writes something large, make sure it lands in an ignored folder before
committing.
