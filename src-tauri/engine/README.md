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
- **`Placement`** - where a frame sits in its view, in device pixels
  (`xPx`, `yPx`, `widthPx`, `heightPx`). Where the photo is smaller than the
  view on an axis, the frame is all of it, centred; where it is larger, the
  frame fills the view. The fixtures' `placements` pin it.
- **The transport** - the event name (`ENGINE_EVENT`), the frame block's
  header (`FRAME_HEADER_BYTES`: ticket, width and height as little-endian
  u32s, then the RGBA rows; the fixtures' `frameBlock` is one) and the markers
  of a frame handed over in shared memory (`FRAME_SHARED`,
  `FRAME_SHARED_KEY`). Defined once, here, and generated to TypeScript.

The calls, which step 2 makes into Tauri commands (`src/engine.rs`):

| Call | Returns |
|---|---|
| `sections()` | every `SectionDescriptor`, in the order the panels are shown |
| `tools()` | every `ToolDescriptor`, in pipeline order |
| `open(path)` | `Opened { photoId, width, height, edit }`, without rendering |
| `apply(photoId, EditPatch)` | `Applied { ticket, edit }` at once: the render's ticket, and the edit as the engine took it |
| `view(photoId, View { zoom, centerX, centerY, width, height })` | a `RenderTicket`, at once |
| `frame(ticket)` | the frame's sRGB RGBA pixels, while it is the latest; `block()` is the frame as it crosses to the interface |
| `close(photoId)` | the photo's edit; a render already asked for still reports, then the photo's decoded data and its cached results are freed |
| `set_cache_budget_bytes(bytes)` | nothing: the memory cached results may take, which the app sizes from the machine's |

Renders happen on a thread of their own and are latest-wins: a newer ticket
replaces a render still waiting, and stops one already running at its next
step. Each finished render sends a `frame` event (`ticket`, `photoId`, and
`placement`: where the frame sits in the view, in device pixels), then a
`histogram` event (`ticket`, `photoId`, `HISTOGRAM_BINS` luminance bins of
the frame); a failed one an `error` event (`ticket`, `kind`, `message`). An
overtaken render sends nothing.

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
    type Settings: Settings;
    fn process(&self, settings: &Self::Settings, input: &Image, cx: &Context)
        -> Result<Option<Image>>;
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
p.apply(&serde_json::from_value(json!({ "ag_exposure_wb": { "exposure": 0.5 } }))?)?;
let again = p.render(&photo, Size::Fit(1600, 1600))?;   // runs ag_exposure_wb only
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

Results are kept up to a memory budget (`set_cache_budget_bytes`, 1.5 GB by
default, for a 16 GB laptop), least recently used first out. The budget is
counted by what is held: a picture two steps share (one passed its input
through) is counted once, for as long as either holds it, and the photo's own
decoded mosaic is shared with the photo, not copied. Room is made for a result
before it is made, never by letting go of the step's input, which would free
nothing while it is read. The input of the tool changed last - where the next
tick of the same slider starts - and the result just made go last. What a tool
needs while it runs comes on top; `tests/memory.rs` holds the whole process's
peak to the budget plus a measured margin.

`last_render()` says which steps ran and how long each took; `runs(name)`
counts them; `render_unless` gives up between steps when asked; `forget`
lets go of a photo's results.

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
| `ag_exposure_wb` | SceneLinear | basic | no | Exposure in stops, times a red, green and blue multiplier, on top of the as-shot balance | Argentum |

Capture sharpening runs before exposure, as in the app: it undoes the lens
and the sensor, so it comes before anything that is an edit.

## The tests

| Test | File | Needs |
|---|---|---|
| A fourth tool is one file and one registry line | `tests/fourth_tool.rs` | nothing |
| Changing a tool re-runs it and what follows, counted; the cache stays within its budget | `tests/rerun.rs` | nothing; capture sharpening runs for real with a GPU |
| The same, counted and timed on a real photo | `tests/rerun.rs` | `AG_RAW` |
| `dt_highlights` bit-identical to `mods/highlights.rs` | `tests/parity_highlights.rs` | nothing; real photos with `AG_RAW`/`AG_RAWS` |
| `rt_capture_sharpen`: automatic radius and contrast bit-identical, passes within one half-float step of `mods/sharpen_gpu.rs` | `tests/parity_sharpen.rs` | a GPU for the passes; real photos with `AG_RAW`/`AG_RAWS` |
| `ag_exposure_wb` exact against a reference computation | `tests/parity_exposure_wb.rs` | nothing |
| The generated TypeScript and fixtures are current | `tests/contract.rs` | nothing |
| Open, apply, view, events, latest-wins, errors; the size `open` reports is the developed size | `tests/engine_api.rs` | nothing; real photos with `AG_RAW`/`AG_RAWS` |
| A slider tick at preview size stays within its budget (25 ms) | `tests/budgets.rs` | `AG_RAW` |
| The process's peak memory, fitted and at 100% with a slider dragged, stays within the cache's budget plus 1.5 full-size pictures; a closed photo's memory is given back | `tests/memory.rs` | nothing (a synthetic 61 MP photo); a real one with `AG_RAW` |

`AG_RAW` names one RAW file and `AG_RAWS` several, separated by `;`, as the
app's own tests take them. Unset, the real-photo tests say they skipped and
pass; without a GPU adapter, so do the GPU ones.

A budget is set from its first measurement and never raised. The slider tick
is timed end to end - patch in, frame event out - in a test binary of its own,
since tests in one process share a thread pool.

The app's side of each parity test is the app's own file, compiled unchanged
by `reference/` - a test-only crate that points at `src-tauri/src/mods/` and
stands in for the two or three things those files reach for in the rest of
the app. When one of them changes what it imports, that crate stops
compiling, which is the moment to check whether the engine's copy has fallen
behind.

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
- **Golden renders per tool** (north star 6), masks, the editor's other
  tools, and wiring into the app - later steps.
