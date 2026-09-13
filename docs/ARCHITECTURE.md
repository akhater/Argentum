# Architecture

The whole design has one goal: **be able to pull RapidRAW's updates forever
without a fight.**

---

## The problem this solves

A fork dies when your changes and upstream's changes land in the same lines of
the same files. Every update becomes an argument. Eventually you stop updating,
and now you're maintaining a whole photo editor alone.

## The rule that prevents it

> **Our code goes in our own files. We touch their files as little as physically possible — ideally one line per tool.**

Files upstream has never heard of can never conflict.

---

## How that works for shaders

RapidRAW's image engine is one file, `src-tauri/src/shaders/shader.wgsl`.
Every tool is a function; `main()` calls them in order.

The naive approach — writing our functions into that file — means every
upstream update to it is a merge conflict.

Instead: the shader is pulled in with Rust's `include_str!`, so we can glue
two files together at build time.

```rust
// gpu_processing.rs — changed once, ever
source: wgpu::ShaderSource::Wgsl(
    concat!(
        include_str!("shaders/modules.wgsl"),   // ours
        include_str!("shaders/shader.wgsl"),       // theirs, untouched
    ).into()
)
```

**`modules.wgsl` is ours alone.** Every function we harvest
goes in there. Upstream never sees it, never conflicts with it.

The only mark we leave in their shader is one call line per tool:

```wgsl
// in main(), theirs:
color = dt_white_balance(color, t_temperature, t_tint);   // ← our one line
```

A one-line change is a trivial conflict to resolve, in the rare case upstream
edits nearby.

---

## Layout

```
src-tauri/src/
  shaders/
    shader.wgsl          THEIRS  — one call line added per tool
    modules.wgsl         OURS    — all harvested math lives here
  mods/                  OURS    — our Rust code
    catalog.rs             the offline photo index
    paths.rs               where the data folder lives
    color.rs               conversion between their color space and darktable's
  image_processing.rs    THEIRS  — settings fields added per tool
  gpu_processing.rs      THEIRS  — changed once, for the concat above

src/
  components/adjustments/
    *.tsx                THEIRS  — a slider added per tool
```

**Rule of thumb:** if a file is ours, put anything in it. If a file is theirs,
you should be adding a handful of lines, not restructuring.

---

## The color space thing

RapidRAW works in linear sRGB. darktable's modules assume something wider or
more perceptual. Transplanted math needs converting in and back out.

That conversion lives in `modules.wgsl` and is **written once, reused by every
tool after** — not a per-tool cost.

**What actually got built, in `26.37.2`:** linear sRGB ↔ CIE XYZ, plus XYZ ↔
Bradford cone space (LMS). White balance needed a colorimetric space and a cone
space, not a wider RGB one, so that is what exists:

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
| `lib.rs` | `mod mods`, the cache check, one `ag` command | add a match arm in `mods/dispatch.rs` |
| `shader.wgsl` | one call to `ag_stage_scene_linear` | add your tool inside that function in `modules.wgsl` |
| `raw_processing.rs` | one call to `mods::decode::on_raw_decoded` | add a step in `mods/decode.rs` |
| `image_processing.rs` | the CPU preview encode interception | change `mods/preview_encode.rs` |
| `App.tsx` | one `<Argentum />` | add a portal in `argentum/Argentum.tsx` |
| `Color.tsx`, `MetadataPanel.tsx` | one `data-argentum` marker each | portal into the existing marker |
| 13 locale files | nothing | add a string to `argentum/locales/en.json` |

`scripts/check-mergeability.mjs` enforces this. It knows the difference between
a hook (a call, an import, a mount point — their code depending on ours) and the
rebrand (`.agdata`, "Argentum" inside a sentence they already had). The rebrand
happened once and does not grow, so it is not counted.

Add a hook to one of their files and the build fails, naming the anchor and what
to do instead.

### The one category that is not free

Portals add UI. They cannot change what happens when the user clicks something
of *theirs*. Three anchors exist for that: the white balance picker
(`ImageCanvas.tsx`), lens auto-detection on load (`CropPanel.tsx`), and reading
the lens from the maker note (`exif_processing.rs`), plus the lens profile match
in `lens_correction.rs`.

Each replaces the body of an existing handler, so each was a one-time
replacement rather than something a later feature adds to. **If one of these
ever needs a second hook, the injection is in the wrong place** — it should
become an event our code listens for, not another line of theirs.

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
   category as `color-tools` — an approved exception with a date, like the three
   that exist.
3. Our own window. No anchor at all and the worst place for it to be.

Decide that before writing any of the model code, because it decides where the
code goes.

**Open, and not to be guessed at.** Model licences and sizes (two more downloads
on top of SAM's). Whether the tags are worth storing per photo — they would make
library search work on content, which needs somewhere to put them, and the
catalogue index is the obvious place and does not exist yet.

---

## Staying mergeable

```bash
git remote add upstream https://github.com/CyberTimon/RapidRAW.git
```

```bash
git fetch upstream && git merge upstream/main
```

If a conflict appears, it will almost always be in `shader.wgsl` or
`image_processing.rs`, and it will be our added lines sitting next to their
changed ones. Keep both. That's the whole resolution.

**If you ever find yourself doing a real merge, something drifted from the rule
above.** Move that code into a file of ours instead.

### A clean merge is not the same as a safe one

This is the part that took a mistake to learn. On 2026-09-13 ten upstream
commits merged with no conflict in application code, and that was written up as
proof the architecture works. It was not proof of anything.

`git merge` answers one question: *do these texts collide?* It cannot answer the
one a fork lives on: *has upstream now built, moved or renamed the thing we
built around?* Those changes conflict with nothing. Had upstream shipped their
own auto white balance that week, git would have merged it in silence and
Argentum would have had two of them — or theirs would quietly have won.

Three kinds of change merge cleanly and still break us:

- **Upstream fixes a function we shadow.** We left it in place and stopped
  calling it, exactly as the rule says. Their fix lands in code that never runs.
- **Upstream edits a file we carry one of their own fixes in.** Nothing marks
  it: they need not mention the pull request number, and in RapidRAW most
  commits are the maintainer's own and mention nothing. `8737fc4e` rewrote the
  cache hashing that one of our borrowed blocks sits inside.
- **Upstream changes a key we re-typed.** `showClipping` is 0..4 here and still
  a boolean in their `Waveform.tsx`. The day they write `=== true`, our control
  reads as off and nothing errors.

A fourth merges cleanly and breaks nothing, which is worse: **upstream builds
the feature we built**, in files we have never touched. Nothing detects that.
Not a diff, not a file match, not a regular expression over commit subjects —
a commit called "improve colour handling" matches nothing and could be our
entire white balance module arriving from the other direction.

So there are two halves, and only one of them is mechanical.

`scripts/upstream-registry.mjs` is the inventory: every feature, borrowed fix
and deliberate behaviour change, with the upstream files, symbols and keys it
rests on, and what proves it works. Overlaps are derived from that and from git,
keyed to the exact upstream commit, and `scripts/upstream-decisions.mjs` records
a verdict and a reason for each. `npm run check:merge` re-derives them and fails
until the decisions are there.

The other half is a sentence. Every review entry carries a `featureReview`
saying that a person read the incoming batch for things we already have, and
what they concluded. Nothing verifies it. It is required so that the claim is
made explicitly by someone rather than implied by a green check — which is the
same mistake, one level up, as reading a clean merge as proof of safety.

Neither half checks that the reasoning is any good. They make sure the question
was asked, which is the part that was being skipped.

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
