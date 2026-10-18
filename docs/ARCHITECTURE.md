# Architecture

Argentum began as a fork of RapidRAW. It is becoming an application of its
own, one piece at a time, and it keeps working at every step:

> **New code goes in the engine. The render path is ours. RapidRAW is a source
> we take from, reviewed once per release - not a base we merge forever.**

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

So the line moved:

| Where | Rule |
|---|---|
| `src-tauri/engine/` | New tools: one file in `tools/`, one line in `registry.rs` (see its README) |
| `src-tauri/src/mods/`, `shaders/modules.wgsl`, `src/argentum/` | Ours, as before, until moved into the engine |
| The render path: `gpu_processing.rs`, `lib.rs`, `image_processing.rs` | Ours to change. RapidRAW's changes here are read at each release and copied in by hand when wanted, never merged |
| Today's interface: `App.tsx`, `components/panel/` (`Editor.tsx` and the panels), `components/adjustments/`, `components/views/` | Ours to change in place: for a layout switch built from today's panels, and for swapping panels one at a time for ones built from the engine's tool registry. RapidRAW's changes here are read at each release and copied when wanted, never merged blind |
| Their other files | Still merged, once per release. Our lines in them may only go down |

Nothing of theirs is deleted while something still uses it. Once nothing does,
it is deleted and noted in that release's review. RapidRAW stops being merged
at all when none of their files runs while previewing, exporting or masking,
none of their panels is shown, and the sidecar format is ours.

---

## How that works for shaders

This is how the tools in today's pipeline reach the GPU. New tools go into the
engine instead (`src-tauri/engine/README.md`).

RapidRAW's image engine is one file, `src-tauri/src/shaders/shader.wgsl`.
Every tool is a function; `main()` calls them in order.

The naive approach — writing our functions into that file — means every
upstream update to it is a merge conflict.

Instead: the shader is pulled in with Rust's `include_str!`, so we can glue
two files together at build time.

```rust
// gpu_processing.rs — on the render path, ours
source: wgpu::ShaderSource::Wgsl(
    concat!(
        include_str!("shaders/modules.wgsl"),   // ours
        include_str!("shaders/shader.wgsl"),       // theirs, untouched
    ).into()
)
```

**`modules.wgsl` is ours alone.** Every function we harvest
goes in there. Upstream never sees it, never conflicts with it.

The only mark we leave in their shader is one call line per stage, and every
tool of ours runs inside a stage (see THE PIPELINE ANCHOR in `modules.wgsl`):

```wgsl
// in main(), theirs:
composite_rgb_linear = ag_stage_scene_linear(composite_rgb_linear, /* profile rows */);  // ← our one line
```

A one-line change is a trivial conflict to resolve, in the rare case upstream
edits nearby.

---

## Layout

```
src-tauri/
  engine/                OURS    — the processing engine: a tool registry, one file per tool
  src/
    shaders/
      shader.wgsl        THEIRS  — one call line per stage
      modules.wgsl       OURS    — harvested math for the current pipeline
    mods/                OURS    — our Rust code, until it moves into the engine
    gpu_processing.rs    RENDER PATH — came from RapidRAW, ours to change
    image_processing.rs  RENDER PATH
    lib.rs               RENDER PATH
    *.rs                 THEIRS  — merged once per release

src/
  App.tsx                INTERFACE — came from RapidRAW, ours to change in place
  argentum/              OURS
  components/
    panel/               INTERFACE — Editor.tsx and every panel
    adjustments/         INTERFACE — the adjustment sections
    views/               INTERFACE — the editor and library layouts
    ui/, modals/, ...    THEIRS  — merged once per release
```

**Rule of thumb:** a file of ours takes anything. A file on the render path or
in today's interface is ours to change, a piece at a time. A file of theirs
takes no new lines: what a feature needs goes into a file of ours, and the line
count in theirs only goes down.

---

## The color space thing

RapidRAW works in linear sRGB. darktable's modules assume something wider or
more perceptual. Transplanted math needs converting in and back out.

That conversion lives in `modules.wgsl` and is **written once, reused by every
tool after** — not a per-tool cost.

**What actually got built, in `26.37.2`:** linear sRGB ↔ CIE XYZ, plus XYZ ↔
Bradford cone space (LMS). White balance needed a colorimetric space and a cone
space, not a wider RGB one, so that is what existed. (Retired in `26.41.6`,
with our white balance: RapidRAW 1.6.5 does the same in its own engine. The
pattern below stands for the next tool that needs it.)

```wgsl
const AG_SRGB_TO_XYZ  // and AG_XYZ_TO_SRGB
const AG_XYZ_TO_LMS   // and AG_LMS_TO_XYZ  — Bradford
```

```wgsl
fn dt_something(color: vec3<f32>, ...) -> vec3<f32> {
    let xyz = AG_SRGB_TO_XYZ * color;    // in
    // ... darktable's math, largely as-is ...
    return AG_XYZ_TO_SRGB * xyz;         // out
}
```

There is **no `srgb_to_rec2020` and no `mods/color.rs`** — earlier drafts of
this document promised both. If a later harvested module genuinely needs linear
Rec2020, add that matrix pair to `modules.wgsl` alongside the others. Going
through XYZ is the general route; a wide-RGB working space is one destination,
not the only one.

---

## Data and storage

- **Edits** — JSON sidecar next to each photo (`IMG_1234.CR3.rrdata`).
  Upstream's format; we add fields to it. Original RAW never modified.
- **Catalog** — SQLite in the data folder. Ours entirely. Lets you browse
  photos whose drive is unplugged.
- **Thumbnails** — JPEGs in the data folder. Already exists upstream; we make
  the location configurable and keep them across sessions.

All of it in one user-chosen folder. See [README](README.md#everything-in-one-folder).

---

## What the next feature costs

The question that decides whether this fork is alive in two years is not "how
much of their code have we touched". It is **what does feature number one
hundred cost?**

If every feature adds a line to one of their files, the answer is a hundred
lines in the files upstream also edits, and the merge stops being worth doing.
Four features looks free. That is the trap.

So each of their files gets a **fixed** number of hooks into our code — an
anchor — and everything after that routes through it on our side. An anchor is
not a budget to spend. It does not go up when a feature is added, because a
feature must not need it to.

| Their file | Anchor | A new feature instead |
|---|---|---|
| `shader.wgsl` | one call each to `ag_stage_scene_linear`, `ag_stage_tone` and `ag_stage_display` | add your tool inside one of those functions in `modules.wgsl` |
| `raw_processing.rs` | one call to `mods::decode::on_raw_decoded` | add a step in `mods/decode.rs` |
| 13 locale files | nothing | add a string to `argentum/locales/en.json` |

`scripts/check-mergeability.mjs` enforces this. It knows the difference between
a hook (a call, an import, a mount point — their code depending on ours) and the
rebrand (`.agdata`, "Argentum" inside a sentence they already had). The rebrand
happened once and does not grow, so it is not counted.

Add a hook to one of their files and the build fails, naming the anchor and what
to do instead. The render path and today's interface have no anchor counts:
they are ours to change. The `data-argentum` markers in the panels and the
single `<Argentum />` in `App.tsx` stay as the way our components mount, and
the wiring checks on them stay. New tools need no anchor at all, because they
go into the engine.

### The one category that is not free

Portals add UI. They cannot change what happens when the user clicks something
of *theirs*. Two anchors in their Rust files exist for that: reading the lens
from the maker note (`exif_processing.rs`) and the lens profile match in
`lens_correction.rs`. (Lens auto-detection on load, in `CropPanel.tsx`, is in
today's interface now, which is ours to change.)

Each replaces the body of an existing handler, so each was a one-time
replacement rather than something a later feature adds to. **If one of these
ever needs a second hook, the injection is in the wrong place** — it should
become an event our code listens for, not another line of theirs.

### A mask tool, with no anchor at all

The object brush (26.41.4) needed three things that look as if they need
hooks: a tile in their masks toolbox, a mask type their renderer can draw, and
a canvas that paints instead of drawing a box. It had none. It was removed in
26.41.5, because their Subject box selected better than our paint, and its
code is in git at `c15e53f0`. The pattern stands, for the next mask tool:

1. **The tile is data.** Their grid renders `MASK_AI_TYPES`, an exported
   array it reads on every render. `objectMask.ts` splices an entry into it at
   startup. The type string is chosen so their fallback label reads right:
   `object` becomes "Object".
2. **The type does not survive.** Nothing of theirs can draw an `object`
   mask, so a store subscription turns it into an ordinary `ai-subject` the
   moment their panel creates it, with `objectStrokes` in its parameters as
   the mark that it is ours. From there on, rendering, export, Grow and Feather
   are theirs, unchanged.
3. **The canvas is intercepted, not changed.** A capture-phase listener on the
   window takes a press aimed at their Konva stage before their handler hears
   it. Their box tool never starts, and moves and releases with no press are
   ignored by their own code.

What it costs is coupling, not lines: if their grid stops reading the array
live, or their fallback label changes, the tool degrades without a conflict.
Every one of those dependencies is listed in the `object-brush` registry entry,
retired now but kept as the checklist for the next tool built this way.

---

## Planned: naming the objects before masking them

Written down now because the shape of it is decided and the cost is not where it
looks. Nothing is built yet. See the Masking row in `ROADMAP.md`.

**What it is.** RapidRAW's AI mask (`generate_ai_subject_mask`) takes a
`start_point` and an `end_point`: you drag a box round the thing, SAM segments
inside it. The wanted behaviour is the other way round — look at the photo once,
list what is in it, tick the dog.

**Three models, not one.**

| Step | Model | What it answers |
|---|---|---|
| Name | RAM++ | *what* is in this picture — a list of tags, no positions |
| Locate | Grounding DINO (or YOLO-World) | *where* the thing called "dog" is — a box |
| Cut | SAM | the mask inside that box — **already shipped** |

RAM++ classifies and does not localise. That is the whole reason the middle row
exists, and it is the thing most likely to be misremembered later as "RAM++ gives
you objects". It does not. This chain is Grounded-SAM's own arrangement, so it is
a known-good combination rather than an invention.

**What is already here.** `ort` (ONNX Runtime, `load-dynamic`) is a dependency.
`ai_processing.rs` already downloads a model from a URL into
`get_models_dir(app_handle)`, checks a SHA-256, and caches the session — so two
more models are a list entry, not new machinery. SAM needs a box; we would be
handing it one we computed instead of one the user drew, which is the same
argument it takes today.

**Where the cost actually is.** Not the models — the mount point. A picker has to
appear inside their masks panel, and `MasksPanel.tsx` has no `data-argentum`
marker. So this feature wants a **new anchor in one of their files**, which the
rule above says a feature must not need, and `check-mergeability.mjs` will fail
the build until someone decides. Three ways out, in order of preference:

1. Put the picker in a panel we already have a marker in, and have it *create*
   the mask rather than live inside the mask UI. No new anchor.
2. One new marker in `MasksPanel.tsx`, as a one-time mount point in the same
   category as `color-tools`. The ratchet counts it as a line of ours in their
   file, so it has to be paid for by removing one elsewhere in that file.
3. Our own window. No anchor at all and the worst place for it to be.

Decide that before writing any of the model code, because it decides where the
code goes.

**Open, and not to be guessed at.** Model licences and sizes (two more downloads
on top of SAM's). Whether the tags are worth storing per photo — they would make
library search work on content, which needs somewhere to put them, and the
catalogue index is the obvious place and does not exist yet.

---

## Staying mergeable

RapidRAW is merged once per release, after a review of that release. Between
releases nothing of theirs is merged.

```bash
git fetch upstream
```

```bash
npm run review:upstream
```

The review covers the range from the last reviewed release to their newest
release tag, in the order it is read:

1. **Their shader changes first**, as a diff: the most valuable changes and the
   easiest to copy.
2. **Their changes to the render path and today's interface**, to read. What
   is wanted is copied in by hand, with the source commit in a comment; nothing
   there is merged.
3. **Their changes to files our registry entries depend on**
   (`scripts/upstream-registry.mjs`), with the entries that rely on each one.
4. **One line per feature**, grouped from their pull requests, with a verdict
   to fill in: **take** (merge it as it is), **copy** (into a file of ours,
   naming it), **skip** (not wanted: their library, cloud, or an interface
   being replaced), or **decide** (they built something we already have).

It ends with the register entry to paste into
`scripts/upstream-decisions.mjs` and the merge commands. The review is meant
to take under a day, and the entry records how long it took.

### What `npm run check:merge` holds

It runs in CI as a required check, and on demand. It takes about a second:
each question is asked of git once, by one streamed command.

- **The ratchet.** For every file of theirs outside the render path and today's
  interface, the number of lines we have added to it is recorded in
  `scripts/upstream-ratchet.json`.
  A file above its number fails the build. A file below it lowers the number:
  the check writes it locally, and CI fails until it is committed. The file is
  also compared with the one on `main`, so a number raised by hand fails too.
- **Coverage.** A file from RapidRAW that we changed, render path and interface
  included, must be claimed by a registry entry saying what depends on it and
  what proves it works. That is all a pull request owes for touching one:
  overlaps with upstream are handled once, at the release review.
- **Integrity.** Registry entries are retired, never deleted, and a retirement
  names the release review that decided it. Borrowed upstream fixes carry
  `// upstream #NNNN` markers that the registry must agree with.
- **Anchors.** Calls from their files into ours stay at a fixed count, and the
  wiring an anchor exists for is still there.
- **Shadows.** Code of theirs we leave in place and step around, outside the
  render path and the interface, must still exist in the release we merged.
  Its disappearance from their main is a warning for the next review.
- **The release reviews.** One unbroken chain of reviews; upstream merged in
  exactly up to the newest one; every commit of that release in some feature;
  every change of theirs to the render path or the interface marked copy, skip
  or decide, never take; `sameAsOurs` saying whether they built something we have; and
  `stillLive`, the list of their files still used when previewing, exporting
  or masking, never longer than at the review before.

The reviews before 2026-10-18 were per commit. They stay in the register as
history and are still checked for shape, but no new per-commit overlaps are
derived.

### A clean merge is not the same as a safe one

On 2026-09-13 ten upstream commits merged with no conflict in application
code, and that was written up as proof the architecture works. It was not
proof of anything.

`git merge` answers one question: *do these texts collide?* It cannot answer
the one that matters: *has upstream now built, moved or renamed the thing we
built around?* Those changes conflict with nothing:

- **Upstream fixes a function we shadow.** We left it in place and stopped
  calling it. Their fix lands in code that never runs.
- **Upstream edits a file we carry one of their own fixes in.** Most RapidRAW
  commits are the maintainer's own and name no pull request, so nothing marks
  it.
- **Upstream changes a key we re-typed.** `showClipping` is 0..4 here and still
  a boolean in their `Waveform.tsx`. The day they write `=== true`, our control
  reads as off and nothing errors.
- **Upstream builds the feature we built**, in files we have never touched.
  Nothing detects that: not a diff, not a file match, not a pattern over commit
  subjects.

The registry and the review's third section cover the first three. The last
one is a sentence: every release review carries `sameAsOurs`, saying what the
release was read for and what was concluded, with a `decide` line for each
duplicate found. Nothing verifies that sentence. It is required so that the
claim is made explicitly by someone rather than implied by a green check.

### Review the interface separately from the processing

For the exact merged boundary, pending updates, reasons and resumption steps,
read [UPSTREAM_CATCHUP.md](UPSTREAM_CATCHUP.md). Update that handoff when a
temporary deferral is resolved; do not infer a permanent rejection from it.

**Keeping our implementation does not mean rejecting RapidRAW's interface.**
Whenever upstream changes a feature we also have, review both how it works
underneath and how a person uses it: controls, layout, shortcuts, feedback and
the steps needed to finish an edit.

We may have good reasons to keep Argentum's processing while adopting
RapidRAW's better controls or workflow. For example, keeping our highlight
recovery would not prevent us from adopting a useful upstream highlight
control, provided that control makes sense for our recovery method.

Prefer reusing their interface through the existing anchors over maintaining
a second interface of our own. Check that labels, ranges, defaults, saved
settings and undo still mean what the interface promises when connected to our
logic. If the combination requires extensive changes to upstream files, weigh
that maintenance cost before adopting it.

Record both conclusions in the upstream review: what happens to the processing,
and what happens to the interface and workflow. A decision to keep ours must
still say whether upstream's UI/UX offers anything worth taking; if it does not,
say why. A feature is not an all-or-nothing choice between two implementations.

---

## Where code comes from

Nothing in this design is darktable-specific. `modules.wgsl` holds *harvested*
math — the source doesn't matter, only that it's a formula we can express as a
shader function.

Name functions by where they came from, so it stays obvious a year later:

| Prefix | Source | Good for |
|---|---|---|
| `dt_` | darktable | color science, tone mapping, denoise, highlight recovery |
| `rt_` | RawTherapee | demosaic, highlight recovery — often the strongest here |
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

---

## Where things live

Two locations, deliberately. **Nothing heavy goes in OneDrive.**

| | Path | Rule |
|---|---|---|
| Source, docs, config | `...\OneDrive\...\Personal\Argentum` | Git-tracked. Text only |
| Build output, runtimes, models, app data | the checkout | Never tracked, never synced |

A Rust `target/` directory reaches several GB and `node_modules` tens of
thousands of files. Syncing either would be miserable. So they are redirected
at the tool level, not merely gitignored:

- ~~**Rust** - `.cargo/config.toml` sets `target-dir` into NoCloudZone~~
- ~~**Node** - `node_modules` is a directory junction pointing into NoCloudZone~~
- **App runtime data** - catalog, thumbnails, AI models: the app's own
  configurable data folder, defaulting into NoCloudZone

**Both redirects above are gone.** See the correction at the end of this file -
OneDrive destroyed the junction, and the `target-dir` override later broke the
dev build. Neither is needed now that the whole working tree sits outside
OneDrive.

`setup.ps1` creates all of it and verifies the toolchain.

`.gitignore` covers the same ground as a safety net. **It is the net, not the
mechanism** - if something large slips past a redirect it should still never be
committed, but the redirect is what actually keeps it out of OneDrive.

If you add a dependency that writes something large, redirect it before
committing.

---

## Correction: the working tree is not in OneDrive

The section above described a split where source lived in OneDrive and only heavy
output went to NoCloudZone, joined by a `node_modules` junction. **That does not
work and was abandoned the same day.**

OneDrive does not tolerate directory junctions inside a synced folder. It
replaced the junction with a real directory - reparse tag `0x9000e01a`, its own
cloud-files tag - and began syncing 182 npm packages (239MB). It also holds those
files locked while uploading, so they cannot be removed until the sync client is
stopped.

**Current layout:**

| | Path | What |
|---|---|---|
| Work | the checkout | Everything - source, node_modules, target, app data |
| Backup | `...\OneDrive\...\Personal\Argentum.git` | Bare git repo, ~7MB, source history only |

The split moved up a level. Instead of separating files *within* one folder, the
working tree sits entirely outside OneDrive and OneDrive holds the git history.
Every version of every source file is still backed up, in far less space, and a
binary can never arrive because git never tracks one.

`git push` is the backup. `.gitignore` still keeps build output out of history.

Note: the bare repo needs `receive.shallowUpdate true`, because this repo is
shallow at RapidRAW's root (see the upstream section).

### And the target-dir redirect had to go too

The same cleanup missed one thing. `.cargo/config.toml` still pointed Rust's
`target-dir` at `NoCloudZone\Argentum\target`. That was correct while the source was
in OneDrive - but once the project itself moved to NoCloudZone, that path became
the **repo root**, so `target/` landed inside the source tree.

Vite watches the project directory and only ignores `**/src-tauri/**`. It tried
to watch 819 crates worth of build artifacts, hit a build script `.exe` that
cargo had open, and crashed with `EBUSY` - which killed the whole dev command.

Removed. Tauri's default `src-tauri/target` is already inside Vite's ignore list
and already covered by `.gitignore`.

**General lesson:** with the working tree outside OneDrive, no redirect is needed
for anything. Default tool behaviour is correct now. Adding one back is how both
of these breakages happened.
