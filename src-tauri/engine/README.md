# argentum-engine

Argentum's processing engine: a registry of tools, a pipeline that runs them
in order and keeps every tool's result, and the contract the editor's
interface talks to it through.

Tools come from darktable, RawTherapee, RapidRAW, RAWmakase or Argentum.
Each is **one file in `src/tools/` and one line in `src/registry.rs`**.

This crate builds and tests on its own, without building the app, and the app
does not use it yet. It lives under `src-tauri/` so Vite ignores it, and it is
a workspace of its own, so the app's `Cargo.toml` does not know about it.

```bash
cd src-tauri/engine
cargo test                     # everything; real-photo and GPU tests skip when they can't run
cargo test --test rerun -- --nocapture   # one file, with its printed numbers
```

From a worktree, build into the main tree's engine target rather than a new
one: `CARGO_TARGET_DIR=<main checkout>/src-tauri/engine/target`.

## The interface's contract (`src/contract.rs`, `src/engine.rs`)

Every type the interface sees is in `contract.rs` and nowhere else. The
TypeScript types in `src/shell/contract.generated.ts` are generated from it,
with an example of each in `src/shell/contract.fixtures.json` for both sides
to test against, including the patches the engine takes ("patched") and
refuses ("refused"), made by the engine itself. `tests/contract.rs` fails
when either file is stale; to write them:

```bash
AG_WRITE_CONTRACT=1 cargo test --test contract
```

The TypeScript derive (ts-rs) is behind the crate's `ts` feature, which only
the tests turn on: whatever links the engine does not build it.

- **`ToolDescriptor`** - a tool's id, origin, label (an i18n key), section,
  order, stage, settings version and `SettingSpec`s: key, label, kind
  (`float`, `int`, `bool`, `choice`), default, and where the tool gives them
  min, max, step, unit, choices, `advanced`, a `scale` for a float (`linear`
  or `log`) and `enabledWhen: { key, equals }` for a setting that only means
  something while another has one value. Read off the tool's own settings
  struct, so a tool is still one file and one line.
- **`SectionDescriptor`** - the panels, in order, each with an id and a
  label key, defined once in `contract.rs`; each tool names its own.
- **`EditDocument`** - `{ version: 1, tools: { id: { v, settings } } }`.
- **`EditPatch`** - `{ id: { key: value } }`, merged into the edit by
  `Registry::merge`; `null` puts a setting back to its default. All or
  nothing: an unknown tool or setting, or a value of the wrong type, changes
  nothing. Only the tools it names, and the tools after them, run again.
- **`EngineError`** - what a refused call returns: an `ErrorKind`
  (`unknownPhoto`, `badView`, `unknownTool`, `unknownSetting`, `badValue`,
  `badEdit`, `decode`, `noGpu`, `render`, `internal`) and a message. The
  `error` event carries the same kind.
- **`View`** - zoom, centre, and a size in device pixels (CSS pixels times
  the device pixel ratio).

The calls, which step 2 makes into Tauri commands (`src/engine.rs`):

| Call | Returns |
|---|---|
| `sections()` | every `SectionDescriptor`, in the order the panels are shown |
| `tools()` | every `ToolDescriptor`, in pipeline order |
| `open(path)` | `Opened { photoId, width, height, edit }`, without rendering |
| `apply(photoId, EditPatch)` | `Applied { ticket, edit }` at once: the render's ticket, and the edit as the engine took it |
| `view(photoId, View { zoom, centerX, centerY, width, height })` | a `RenderTicket`, at once |
| `frame(ticket)` | the frame's sRGB RGBA pixels, while it is the latest |

Renders happen on a thread of their own and are latest-wins: a newer ticket
replaces a render still waiting, and stops one already running at its next
step. Each finished render sends a `frame` event (`ticket`, `photoId`,
`width`, `height`), then a `histogram` event (`ticket`, `photoId`,
`HISTOGRAM_BINS` luminance bins of the frame); a failed one an `error` event
(`ticket`, `kind`, `message`). An overtaken render sends nothing.

Below 100% the pipeline renders the whole photo at the zoom and the view is a
cut of it, so panning re-renders nothing; at 100% and past it the view is cut
from the full-size render. The frame's encode is a plain sRGB curve until
display tools take over.

## A tool (`src/tool.rs`)

```rust
pub trait Tool: Default + Send + Sync + 'static {
    const ID: &'static str;      // "rt_capture_sharpen": origin prefix + name
    const STAGE: Stage;          // Raw | SceneLinear
    const SECTION: Section;      // the panel it is shown in
    const GPU: bool = false;     // true for a tool that runs on the GPU
    const PIXEL: Option<Pixel> = None;  // set for a per-pixel tool: its WGSL
    type Settings: Settings;
    fn defaults_for(&self, source: &Source) -> Self::Settings { Default::default() }
    fn process(&self, settings: &Self::Settings, input: &Image, cx: &Context)
        -> Result<Option<Image>>;                        // whole-picture tools
    fn pixel_params(&self, settings: &Self::Settings, cx: &Context)
        -> Option<Vec<[f32; 4]>>;                        // per-pixel tools
}

pub trait Settings: Serialize + DeserializeOwned + Default + Clone + PartialEq + ... {
    const VERSION: u32;
    const FIELDS: &'static [Field] = &[];
    fn migrate(from: u32, value: Value) -> Result<Value, String> { /* none */ }
}
```

- **Id.** Stable for ever once an edit has saved it. The first three
  characters say whose code it is: `dt_` darktable, `rt_` RawTherapee, `rr_`
  RapidRAW, `rm_` RAWmakase, `ag_` Argentum. Lowercase letters, digits and
  `_`. A tool whose id has no known prefix does not compile.
- **Stage.** `Raw` tools get the sensor data before demosaic (`Image::Raw`, a
  rawler `RawImage`: a mosaic, or three colours per pixel for sRAW). The
  pipeline then develops it (demosaic and the camera matrix, as the app does)
  and `SceneLinear` tools get linear RGB (`Image::Rgb`). The registry must
  list every `Raw` tool before every `SceneLinear` one.
- **GPU.** Only a tool with `GPU = true` may ask
  `cx.gpu()` for the engine's device - a wgpu device of its own, opened the
  first time it is asked for.
- **Settings.** A serde struct with `#[serde(default)]`, each field a number,
  a boolean, or a string that is one of its choices. Its JSON is what an edit
  saves and what the cache key is made of. `VERSION` is saved with it; an
  edit saved by an older version goes through `migrate`, a newer one is
  refused. `FIELDS` gives a panel what the struct cannot say - range, step,
  unit, choices, a log scale, `enabled_when`, `advanced` - in the order the
  panel shows them, and numbers outside a range are clamped whenever
  settings are read or set.
- **defaults_for.** The settings a photo opens with, when they are the
  photo's own rather than the struct's `Default`: white balance starts at the
  camera's as-shot light, DNG Standard at the body's exposure offset. The
  engine makes an edit of them when a photo opens, and `null` in a patch puts
  a setting back to them (`Registry::defaults_for`, `Registry::merge`). The
  descriptor's `default` stays the struct's. darktable's `reload_defaults`.
- **Per-pixel tools.** A tool that changes each pixel on its own, from its
  settings and the pixel, sets `PIXEL`: a WGSL function named after its id,
  `fn <id>(c: vec3<f32>, px: Px) -> vec3<f32>`, how many `vec4<f32>`
  parameters it reads, and whether it reads the guided filter of its input.
  It has no `process`; `pixel_params` packs its settings for the GPU, or
  says `None` when they change nothing. Each run of consecutive per-pixel
  tools is one GPU dispatch, the pixel pass (`src/pixels/`): its `main` is
  written from the registry, calling each tool's function in order, so a new
  per-pixel tool is still one file and one line. `Px` gives the pixel's place
  (`coord`), its colour as the pass received it (`input`) and where the
  tool's parameters start (`p`, into `ag_params`); `common.wgsl` has the
  colour helpers. Given parameters of all zeros a tool leaves the colour
  alone. Helpers a tool needs carry its id as a prefix, or the name of the
  app's function they were copied from.
- **process.** Takes the input by reference and returns a new image, or
  `Ok(None)` for "nothing to do": the pipeline then keeps the input as the
  output rather than a copy.
- **Context.** `cx.source` is the photo as decoded, before any tool, for
  measurements that need the sensor's own data (capture sharpening reads its
  radius there). `cx.input_key` names the input exactly, for a tool that
  keeps a measurement between runs.

## The pipeline (`src/pipeline.rs`)

```rust
let mut p = Pipeline::standard()?;
let photo = Source::open("photo.cr3")?;
let preview = p.render(&photo, Size::Fit(1600, 1600))?;
p.set_edit(&p.registry().defaults_for(&photo))?;         // the photo's own starting point
p.apply(&serde_json::from_value(json!({ "rr_presence": { "vibrance": 30 } }))?)?;
let again = p.render(&photo, Size::Fit(1600, 1600))?;   // runs the pixel pass only
```

The steps are the tools in registry order, with the develop between the last
`Raw` tool and the first `SceneLinear` one, and for a preview a resize right
after the develop. Each step's result is kept under a key made of its input's
key and its own settings. Changing a tool's settings changes its key and every
key after it, so a render finds the last step it still has and runs only what
follows. Setting something back finds the earlier results again. A preview and
a full-size render share everything up to the develop; after it, a preview's
tools run on the small picture, and a radius in sensor pixels is scaled by the
picture's `scale()`.

Each run of per-pixel tools is one step, `pixels`, run on the GPU. Its
input goes up once, as half floats - the app's input format - and stays,
with its guided filter worked out beside it, for as long as that input is
the same; a render after that writes the tools' parameters, dispatches, and
reads back. The engine's view of a photo comes straight out of the last
run: the pass cuts, zooms and encodes the frame itself (`render_frame`),
reading for each frame pixel the render pixel `frame.rs` picks, so a slider
tick sends parameters and reads back what is shown. The structure is the
app's: `gpu_processing.rs` keeps its input texture and guided filter the
same way, and `shader.wgsl`'s `main` calls its adjustments one after
another on each pixel. Ours: the calls are written from the registry, not
by hand. `run_pixel_tools` runs every per-pixel tool on a picture of one's
own, for tests.

Results are kept up to a memory budget (`set_cache_budget_bytes`, 2 GB by default),
least recently used first out. `last_render()` says which steps ran and how
long each took; `runs(name)` counts them; `render_unless` gives up between
steps when asked.

## Adding a tool

1. A file, `src/tools/xx_name.rs`, with the settings struct, the tool type and
   its `process`:

   ```rust
   use serde::{Deserialize, Serialize};
   use crate::{Context, Error, Field, Image, Result, Section, Stage, Tool};

   #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
   #[serde(default)]
   pub struct Settings { pub amount: f32 }
   impl Default for Settings { fn default() -> Self { Self { amount: 0.0 } } }
   impl crate::Settings for Settings {
       const VERSION: u32 = 1;
       const FIELDS: &'static [Field] = &[Field::new("amount").range(0.0, 1.0).step(0.01)];
   }

   #[derive(Default)]
   pub struct XxName;
   impl Tool for XxName {
       const ID: &'static str = "xx_name";
       const STAGE: Stage = Stage::SceneLinear;
       const SECTION: Section = Section::Effects;
       type Settings = Settings;
       fn process(&self, s: &Settings, input: &Image, _cx: &Context) -> Result<Option<Image>> {
           if s.amount == 0.0 { return Ok(None); }
           let rgb = input.as_rgb().ok_or_else(|| Error::Tool {
               tool: Self::ID, message: format!("was handed {}", input.describe()) })?;
           let pixels = rgb.pixels.iter().map(|p| p.map(|v| v * (1.0 + s.amount))).collect();
           Ok(Some(Image::Rgb(rgb.with_pixels(pixels))))
       }
   }
   ```

2. One line in the `registry!` list in `src/registry.rs`, where it should run:

   ```rust
   "tools/xx_name.rs" => xx_name::XxName,
   ```

A tool too big for one file is a folder with a `mod.rs` - `rt_capture_sharpen/` keeps its radius, contrast and GPU passes apart - and its line names `tools/<id>/mod.rs`.

That line declares the module and registers the tool. Nothing else changes:
the pipeline's order, the interface's panel, the settings in an edit and the
cache all come from the registry. `tests/fourth_tool.rs` proves it with a
tool in a file of its own.

A tool copied from somewhere says so above each copied function - source file
and commit - and gets a `CHANGELOG.md` entry naming the upstream file and
commit.

## The tools

| Id | Stage | Section | GPU | What | From |
|---|---|---|---|---|---|
| `dt_highlights` | Raw | tone | no | Rebuild clipped channels from the ones that survived, and make what nothing survived white rather than magenta | `mods/highlights.rs` - Argentum's code, after darktable's opposed reconstruction |
| `rt_capture_sharpen` | SceneLinear | detail | yes | Richardson-Lucy deconvolution of the luminance, through a contrast mask, with the radius read from the RAW | RawTherapee `capturesharpening.cc` and `rt_algo.cc`, via `mods/sharpen.rs`, `mods/sharpen_gpu.rs`, `shaders/sharpen.wgsl` |
| `ag_exposure` | SceneLinear | basic | pixel pass | Exposure, in stops | RapidRAW's `apply_linear_exposure` in `shader.wgsl` |
| `rr_white_balance` | SceneLinear | basic | pixel pass | Temperature in kelvin and tint, starting at the camera's as-shot light: Bradford cone gains from that light to the one set | RapidRAW 1.6.5's Kelvin engine: `white_balance.rs`, `read_as_shot_white_balance` in `raw_processing.rs`, `apply_white_balance` in `shader.wgsl` |
| `rr_basic_tone` | SceneLinear | basic | pixel pass | Contrast, highlights, shadows, whites, blacks; the last four move the light a pixel sits in and keep its detail, measured by a guided filter on log luminance | RapidRAW `apply_tonal_adjustments`, `apply_highlights_adjustment`, `sample_gf_tex` and `main`'s detail in `shader.wgsl`; the filter is the app's `guided.wgsl`, used in place, run as `build_guided_coeffs` runs it |
| `rr_presence` | SceneLinear | basic | pixel pass | Vibrance and saturation | RapidRAW `apply_creative_color` in `shader.wgsl` |
| `ag_dng_standard` | SceneLinear | tone | pixel pass | Argentum's default RAW look: the body's exposure offset and Adobe's ACR3 curve, applied hue-preserving (RGBTone) in linear ProPhoto | `mods/dng_standard.rs`, `mods/dng_baseline.tsv` (read in place), `rt_rgb_tone` and `ag_dng_standard` in `modules.wgsl`; RawTherapee, RAWmakase, the DNG SDK |

The order is the app's: capture sharpening first, since it undoes the lens
and the sensor, then exposure, white balance, the tone sliders, presence,
and DNG Standard, which takes the place of the app's tone mapper. The app's
sliders run from -100 to 100 and so do these; exposure is in stops, where
the app's slider moves 1.25 stops a step.

The per-pixel tools' WGSL is the app's functions as written, but for where
their inputs come from, which each file says. The colour helpers they share
are `src/pixels/common.wgsl`, and their CPU counterparts, for the histogram
and the tests, `src/colour.rs`.

## The tests

| Test | File | Needs |
|---|---|---|
| A fourth tool is one file and one registry line | `tests/fourth_tool.rs` | nothing |
| Changing a tool re-runs it and what follows, counted; the cache stays within its budget | `tests/rerun.rs` | nothing; capture sharpening runs for real with a GPU |
| The same, counted and timed on a real photo | `tests/rerun.rs` | `AG_RAW` |
| `dt_highlights` bit-identical to `mods/highlights.rs` | `tests/parity_highlights.rs` | nothing; real photos with `AG_RAW`/`AG_RAWS` |
| `rt_capture_sharpen`: automatic radius and contrast bit-identical, passes within one half-float step of `mods/sharpen_gpu.rs` | `tests/parity_sharpen.rs` | a GPU for the passes; real photos with `AG_RAW`/`AG_RAWS` |
| `ag_exposure` within 4 f32 steps of the app's `apply_linear_exposure`; through the pipeline, a half-float step of exact | `tests/parity_exposure.rs` | a GPU |
| `rr_white_balance`: as-shot light and cone gains identical to `white_balance.rs`; pixels within 2 x 10^-6 of `apply_white_balance` | `tests/parity_white_balance.rs` | a GPU for the pixels; real photos with `AG_RAW`/`AG_RAWS` |
| `rr_basic_tone` end to end on a picture - the app's guided filter, its detail, its sliders - within 2 x 10^-5 | `tests/parity_basic_tone.rs` | a GPU |
| `rr_presence` within 2 x 10^-5 of `apply_creative_color` | `tests/parity_presence.rs` | a GPU |
| `ag_dng_standard`: offsets identical; the curve within a tenth of a level at the app's 16 points; the whole within the app's spline | `tests/parity_dng_standard.rs` | a GPU for the pixels; real photos with `AG_RAW`/`AG_RAWS` |
| A per-pixel tool is one file and one line, and the pass takes it from the registry | `tests/pixel_tool.rs` | a GPU, but for the descriptor and the source check |
| The generated TypeScript and fixtures are current | `tests/contract.rs` | nothing |
| Open, apply, view, events, latest-wins, errors; the size `open` reports is the developed size | `tests/engine_api.rs` | nothing; real photos with `AG_RAW`/`AG_RAWS` |
| A slider tick at preview size, every tool on, stays within its budget (25 ms) | `tests/budgets.rs` | `AG_RAW`, a GPU |

`AG_RAW` names one RAW file and `AG_RAWS` several, separated by `;`, as the
app's own tests take them. Unset, the real-photo tests say they skipped and
pass; without a GPU adapter, so do the GPU ones.

A budget is set from its first measurement and never raised. The slider tick
is timed end to end - patch in, frame event out - in a test binary of its own,
since tests in one process share a thread pool.

The app's side of each parity test is the app's own file, compiled unchanged
by `reference/` - a test-only crate that points at `src-tauri/src/` and
stands in for the few things those files reach for in the rest of the app.
When one of them changes what it imports, that crate stops compiling, which
is the moment to check whether the engine's copy has fallen behind.

Every parity test of a per-pixel tool runs the engine's pixel pass and the
app's own function on the same GPU, on values a half float holds exactly,
since the pass reads its input in half floats as the app does. For the
app's shader functions the same holds on the GPU:
`tests/common/app_shader.rs` compiles `modules.wgsl` and `shader.wgsl` from
disk as the app does, adds an entry point that calls one of their functions
per element, and reads the answers back. Struct offsets in their uniform are
read off the WGSL by naga, never restated.

Golden renders are kept outside the repository, in `AG_GOLDEN` or the
target directory's `tmp/golden`: they are of local photos. The first run
records them; `sheet.html` beside them shows every case side by side.

The sharpening tolerance: the app reads and writes half floats, the engine
keeps f32 and gets back from the GPU only the factor each pixel's luminance
moved by. Given the same input, both run the same passes, and every channel
has to agree within one step of the app's half-float output. Measured, it is
tighter than that: on every value of every case tested, the app's output is
the engine's stored as a half float, rounded one way or the other.

## Not here yet

- **The app's decode fix-ups** in `mods/decode.rs`: Canon sRAW levels, old
  Canon white balance, the multi-exposure exception, camera profiles. The
  engine decodes with plain rawler, so sRAW levels in particular are not yet
  the app's.
- **Orientation.** The develop leaves the picture as the sensor saw it.
- **Closing a photo.** An open photo's decoded data stays until the engine
  goes.
- **A file's metadata.** `Source` carries the decoded image only, so DNG
  Standard takes every file's offset from its body's row: a DNG's own
  BaselineExposure and the app's stop off below a body's lowest native ISO
  wait for it.
- Masks, the editor's other tools, and wiring into the app - later steps.
