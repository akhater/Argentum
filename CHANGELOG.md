# Changelog

Newest first.

**Based on RapidRAW `1.6.6` @ `52138689`** — updated whenever upstream is merged.

## 26.43.4 — 2026-10-23

### Fixed

- **Canon's Auto Lighting Optimizer is read on the EOS R6 Mark III.** Its maker note holds a value
  above the 32-bit signed range beside the Lighting Optimizer setting, and rawler's `get_i32`
  panicked on it (`todo!()`): the reader caught the panic, logged a PANIC line and counted the
  optimizer as off. Only the value used is read now, with `get_i64`, in
  `mods/highlight_protection.rs`; a test covers a maker note with `u32::MAX` beside it.

## 26.43.3 — 2026-10-23

### Added

- **Looks: LUTs suggested for each photo.** At the top of the Looks tab, "Suggested for this photo"
  shows the three LUTs most likely to be put on the open photo, each with why ("You picked it for 4
  similar lamp-lit photos"). It learns from every LUT put on a photo in the editor: the pick is kept
  with the photo's light, lamp-lit or daylight, and eight numbers about its colours. Lamp light is
  told from the camera's As Shot white balance, how bright the scene was (its exposure) and whether
  the photo stays warm after that white balance: no one of them alone tells lamps from daylight on
  leaves or a bright downlight, and together they sorted 210 5D Mark II RAWs (35 under lamps) with
  no mistake. The colours: how warm, how bright, how colourful, and how much green, sky, pink, warm
  tones and near-grey (OkLab hues). A photo's suggestions are the LUTs its eight nearest picks in
  the same light chose, most votes first. No model and nothing downloaded; suggestions
  start after three photos. One pick per photo, the last LUT put on; taking it off forgets it, and
  a photo's own pick never counts for it, so the list stays put while LUTs are tried. Nothing is
  read from photos edited before: it learns from picks made from now on. The picks are one small
  file beside Argentum's settings (`look-picks.json`); the group's ⋯ menu stops or resumes the
  learning, forgets every pick, and says how often the pick was one of the suggestions. The LUT
  folders below keep their A to Z order. `mods/look_picks.rs`, `src/argentum/looks/suggestions.ts`.
- **Each photo keeps its History.** Open another photo and come back, or restart Argentum, and a
  photo's History is as it was left, at the step it was at: undo and redo carry on, and steps undone
  before leaving can still be redone. It is kept in the photo's sidecar beside the edit, as
  `agHistory`, the way darktable keeps its history stack and `history_end` in its XMP
  (`src/common/exif.cc` @ `f973fb0f`); RAWmakase keeps one in its catalog
  (`crates/rawmakase-catalog/src/catalog/develop_history.rs` @ `bf5346d`). Each step keeps its
  name and only what it changed, so a 50-step edit adds about 11 kB, half of it a mask added along
  the way (the edit itself is about 10 kB). A History is used only with the edit it was saved
  beside: after a paste, a reset or a change made elsewhere the photo starts a fresh one. A photo
  opened and left unedited still gets no sidecar, and Copy settings, presets and sync carry the
  edit, never its History. `mods/history.rs`, `src/argentum/history/`.
- **Looks: move or delete several at once.** In the Looks tab, Ctrl-click adds one of your own
  LUTs or presets to a selection and Shift-click takes the run from the last one clicked, as in
  the photo grid; a plain click still puts a look on the photo. A right-click on the selection
  offers "Move 4 LUTs to" a folder and "Delete 4 LUTs" (after a confirmation); Esc clears it.
  LUTs and presets are never selected together, since they live in different folders. Looks that
  come with the app cannot be selected.
- **Linear RAW rendering, the base film-simulation LUTs are made for.** Profile > Rendering gets
  "Linear (for film LUTs)": DNG Standard's per-camera exposure offset without Adobe's tone curve,
  so the photo keeps the camera's linear contrast. A film-simulation LUT carries its film's own
  tone curve and is made to be applied to exactly that (abpy's FujifilmCameraProfiles ask for a
  linear camera profile, Stuart Sowerby's Fuji HaldCLUTs for a neutral RAW with no curve); on DNG
  Standard the two curves stack, and skies and skin blow out to white. Linear brings DNG Standard's
  sharpening. A preset can pick it, so one preset can hold the rendering and the LUT together.
  `mods/dng_standard.rs` (`LINEAR_MODE`, `linear_points`), mode 4 in `mods/raw_tone.rs`, drawn by
  DNG Standard's path in `modules.wgsl`.
- **A preset that brings a LUT fades it in with its Amount.** The LUT's strength used to start from
  the photo's own (full), so the Amount slider did nothing to a preset's LUT below 100%.

### Changed

- **Looks folders are listed A to Z**, in every section and in the Move to menus.

### Fixed

- **Ctrl + Whites or Blacks shows what a look hides.** Holding Ctrl while dragging Whites or
  Blacks shows only what is blown or crushed, and it judged the final picture alone. A faded look
  never prints black or white (one fitted to a faded film stock prints black as 17% grey and
  nothing above 98%), so with one on the view stayed empty at every setting. It now also lights a
  channel where the photo is blown or crushed on its way into the look. For a LUT imported into
  Looks, that is the picture it is handed after the Curves; for a film look that comes with the
  app, its input in ProPhoto where it is capped at white, taken before the camera's exposure
  offset, which the look keeps. The final picture is still judged as well, so a curve after a
  film look that crushes black still shows. Without a look, with one at 0%, or with a film look
  put on before looks were fed their declared input, the view is as before, and the Clipping
  button is unchanged. On a RAW under a faded imported look, the view went from nothing lit to
  7.7% (Blacks, at Exposure -2) and 7.8% (Whites), the areas that are black or white before the
  look. Lightroom's Alt view shows nothing under a faded profile, and darktable, RawTherapee and
  RAWmakase judge only the final picture; this goes further on purpose, because the point being
  set is the photo's, not the look's. `ag_point_view` and `ag_look_entry` in `modules.wgsl`.
- **Object selects what is inside a box again.** A box dragged on the photo with no click before it
  failed with *Invalid dimension #3*; a click worked. SAM 2.1's decoder is told "no points" with an
  empty points tensor, and ort 2.0.0-rc.10, the app's version, refuses to build a zero-sized tensor
  from data. The port of RAWmakase's selection code (`crates/rawmakase-inference/src/runtime.rs`
  @ `7c3c81a`) already allocated the empty tensor for "no box"; now the points and their labels
  get the same, in `rm_selection/src/runtime.rs`.
- **Blown highlights are found where a Canon file states its white too high.** rawler takes a
  Canon file's white level from its SpecularWhiteLevel. On some 5D Mark II frames that is above the
  point the sensor stopped (16383 and 16235, where every clipped photosite reads 16033), so no
  photosite reached the clipping threshold: highlight recovery and `highlights::settle_blown`
  never ran, and a blown sun or a white wall kept the magenta white balance gives clipped data.
  The renderings hid it by capping at white; at -2 EV it showed. A hard clip leaves its mark in
  the file, every clipped photosite on one value (278,198 on 16033 and 136,085 on 16032 on one
  frame, against at most 43 on any value below), and where the file proves a white below the
  threshold, that white now decides what is clipped (`highlights::proven_white`). Only that
  judgement moves: the white level that scales the picture is untouched, so nothing outside a
  clipped area changes. Of 4,513 Canon RAWs measured (5D Mark II CR2 and sRAW, R6 Mark III CR3),
  two change; on those, every changed pixel lies within 8 px of a clipped photosite, and six
  other RAWs render bit for bit as before in every rendering, with and without a film look. The
  idea is RawTherapee's (`rtengine/hilite_recon.cc` @ `1a3fce8c`, which judges clipping against
  the maxima an image reached), held to a spike so a photo with nothing clipped never moves. It
  costs 1 to 4 ms per decode.

- **Film looks get the colours they are made for.** The film looks that come with the app are
  spektrafilm LUTs, made for V-Log over V-Gamut primaries: the input their headers name, and what
  spektrafilm's LUT creator means by it (`spektrafilm_lut_creator/color_spaces.py` @ `3bb2c2d2`,
  the 0.3.4 the LUTs were made with). RapidRAW hands them the V-Log curve over the working RGB,
  linear sRGB, so every colour reached the film as a stronger one: 1.4 to 1.6 times the chroma
  (OkLab) for skin, sky and foliage, skin turned 11° toward red and blues 15° toward cyan. A film
  look put on from now on gets the picture converted to V-Gamut first (`ag_film_lut_input` in
  `modules.wgsl`, `mods/film_lut.rs`). An edit that put one on before keeps its look: a LUT is
  given the declared input only when its edit carries `agLutDeclaredInput`, which the Looks tab
  now writes whenever it puts a LUT on or previews one, and Copy settings and presets carry with
  the LUT (`src/argentum/looks/filmLookInput.ts`). RapidRAW's own code (main @ `211ef559`) still
  feeds sRGB.

- **Film looks take a RAW at the camera's exposure offset.** DNG Standard and Linear brighten each
  RAW by its camera's exposure offset (+0.2 EV at the table's median), but the offset rides in the
  rendering's curve, which a film look replaces whole, so every RAW reached the film darker than it
  shows without one, and switching the rendering did nothing to a film look. spektrafilm takes
  linear 0.18 as middle grey for a V-Log input (its commit `3bb2c2d2`, "fix: vlog midgray
  exposure"), which is where the offset puts a RAW's. A film look put on from now on takes a RAW in
  DNG Standard or Linear at the offset its rendering carries, read back from the edit's own curve
  (`dng_standard::offset_of`), so a later change to the camera table cannot move it; the
  rendering's tone curve still stays out, since the film's own curve is the rendering. An
  enlargement carries its RAW's offset as `agLutExposureEv`. Edits from before keep their look, as
  above.

- **Film looks print blown highlights white, as the rendering shows them.** On some 5D Mark II
  frames a sun or a white shirt came out pink under a film look and white without one. Canon's
  SpecularWhiteLevel sits above the point where red saturates there (16383 and 16235 against
  16034), so `highlights::settle_blown` never sees the area as blown and it keeps the magenta white
  balance gives it; DNG Standard hides that by capping each channel at the sensor's white in
  ProPhoto before its curve. A film look now starts from the same cap (`ag_prophoto_to_white` in
  `modules.wgsl`, shared with DNG Standard). Film looks put on from now on; edits from before keep
  their look.

- **Looks: hovering from one look to the next no longer flashes the photo between them.** Leaving
  a look put the photo back at once, and the next look showed only after its delay and render, so
  every move along the list blinked to the original. The look on screen now stays until the next
  one replaces it, and the photo comes back when the pointer leaves the list or a look is clicked.

- **One History step per slider drag, recorded on release.** History recorded a step 500 ms after
  the last change even with the mouse still down, so a drag that paused now and then left a step
  at every pause (eight "LUT Intensity" steps for one drag). While a slider, curve point, colour
  wheel or the crop's pan is held, the step now waits, and letting go records it at once; Undo
  takes the whole drag off. A click, a typed value or a reset keeps the 500 ms.
  `src/argentum/historyOnRelease.ts` replaces the store's `pushHistory` at import; no RapidRAW
  file changes.

## 26.43.2 — 2026-10-21

### Added

- **AI Models: Download and Clear.** Settings > AI and models gets a Download button, with its size,
  on every model that is not (or only partly) on this computer, a progress bar and Stop while it
  downloads, and a Clear button on the "other files" line that first lists what it would delete
  and how much. A download fetches the same files from the same addresses as the feature that uses
  them and keeps each only once its SHA-256 matches, so the feature finds it there and fetches
  nothing. Clear deletes only what no listed model claims (unfinished downloads, temporary files, a
  retired model, an old copy of a renamed one) and only what it showed; it never follows a link and
  never runs during a download. Adapted from Shoebox `src-tauri/src/models.rs` @ `3c82753`
  (`download`, `fetch`, the `.part` file kept only when its checksum matches) and its
  `src/components/ModelsTab.tsx` (the row's button and progress line); one downloader,
  `mods/model_download.rs`, now serves this page and the selection masks' first use. The tagging
  model's tokenizer, which RapidRAW's code never checks, is pinned to the file Hugging Face serves
  today (SHA-256 `b556ac8c`).

### Changed

- **A photo's first sharpen is about four times faster on Vulkan.** Capture sharpening's
  iteration check read each block's stop flag with an atomic load in every pixel of every
  iteration. On Intel's Vulkan driver that made the pass ten times slower than the three blurs
  around it, and it was most of a first sharpen. The pass that starts each iteration (`tick` in
  `shaders/sharpen.wgsl`) now copies the flags to plain memory once, and every pixel reads the
  copy. A flag set during an iteration never skipped anything in that iteration, so blocks stop
  where they did: renders match the old ones bit for bit on Vulkan, DirectX 12 and OpenGL, on a
  textured photo and on hard edges where the check stops blocks. On an Intel Arc 140V, a first
  render with sharpening went from 787 ms to 182 ms at 3328x2219 and from 2.7 s to 0.6 s at
  6960x4640 on Vulkan, what Processing Backend = Auto picks. DirectX 12 and OpenGL are unchanged.

### Fixed

- **DirectX 12 renders again.** In 26.43.1 the main shader failed to build on DirectX 12, on a
  graphics card and on WARP alike, so photos did not render with DirectX 12 chosen in Settings:
  FXC, the HLSL compiler DirectX 12 uses on every Windows PC, stopped with *unable to unroll
  loop*. Lightroom's colour grading (`rm_grade` in `shaders/modules.wgsl`, from RAWmakase
  `crates/rawmakase-engine/src/develop/gpu/develop.wgsl` @ `5a2414b4`) wrote each channel of its
  result through a loop index, a write FXC cannot address; it unrolled that loop and then could
  not unroll the per-mask loop around it. Each channel is now its own call, with RAWmakase's
  arithmetic in the same order: on Vulkan a graded photo, with the photo's own block and two
  masks' blocks, renders bit-identical to before. Vulkan and OpenGL were not affected.
  `shader_check` now fails any shader that writes a vector or matrix variable through an index
  known only when it runs.
- **AI Models counted the selection models as "other files".** The Subject, Object, Sky and
  Background masks' models (SAM 2.1 and DETR, 258 MB in `rawmakase-selection/`) now have their own
  row, Selection masks. The rows for RapidRAW's Subject selection and Sky detection say what uses
  them now: Quick Erase and Inpainting's Subject for the first; for the second, Foreground and
  Depth, which load it with their own models. Both are still in use, and Clear treats them so.

## 26.43.1 — 2026-10-21

### Added

- **B&W.** A Color | B&W switch at the top of Edit > Basic, as Lightroom's Treatment (V switches
  too). In B&W the photo turns grey after Saturation and before Color Grading, so grading tints
  the grey, and Edit > Color's Color Mixer gives way to the B&W Mix: eight sliders for how light
  each colour turns. The Color Mixer, Hue, Saturation and Vibrance are kept in the edit and left
  out while B&W is on. The grey is RAWmakase's measured mix: Camera Raw 18.7's own renders of a
  colour chart at a zero mix and with each band at four positions, kept as text in
  `src-tauri/engine/src/rm_black_white_chart.tsv`, generated from RAWmakase v0.4.1 by
  `scripts/rawmakase-gray-chart.mjs` (check:merge warns when a newer release is out). Through the
  shader, each colour comes out within 0.3/255 of the chart's grey. Looks ships RAWmakase's six B&W
  presets (Green Filter, High Contrast, Neutral, Red Filter, Sepia, Soft) in a B&W group, and Copy
  settings carries B&W in the Camera Profile group. From RAWmakase @ `a8c5d6d7` (v0.4.0, MIT):
  `crates/rawmakase-engine/src/develop/black_white.rs` (`gray_grid`, `gray`, the chart test) and
  its `black_white_chart.bin`, `crates/rawmakase-engine/src/develop/gpu/develop.wgsl` (`mixer`,
  `adjust`), `crates/rawmakase-interop/src/xmp/apply.rs` (`ConvertToGrayscale`, `GrayMixer`) and
  `assets/presets/BandW`. Unlike RAWmakase, which grades before its grey step and so renders Sepia
  untinted, Color Grading tints the grey, with Lightroom's measured grading for the Looks (see
  "Color Grading renders as Lightroom's" under Changed).
- **B&W Auto mix.** The first time a photo whose B&W Mix is all zero turns B&W (the switch or V),
  its sliders start at the photo's Auto mix, as Lightroom's "Apply auto mix when first converting
  to black and white" does, in the same History step; an **Auto** button beside the B&W Mix's
  Reset sets it again and is greyed while the mix is already Auto's. A mix set by hand or by a
  look is kept. The mix is RAWmakase's measured copy of Camera Raw 18.7's Auto: a fixed linear
  function of the direction in which the photo's colours spread most, in linear ProPhoto with the
  edit's white balance and camera profile, so a cooler white balance brightens Blue. From
  RAWmakase @ `5a2414b4` (v0.4.1, MIT): `crates/rawmakase-engine/src/develop/black_white.rs`
  (`ColorSpread`, `principal_axis`, `AutoMix`'s fit and its test), in
  `src-tauri/engine/src/rm_black_white.rs`; the open photo is measured in `mods/black_white.rs`.
- **DNGs get the lens correction they carry.** A DNG records the corrections Lightroom applies
  as opcodes: FixVignetteRadial (corner shading) and WarpRectilinear (distortion, and colour
  fringing when it has a plane per colour). Leica, DJI's drones, phones and Lightroom's own DNGs
  write them. The same switch in Crop > Lens Correction applies them, on by default as Lightroom
  does. FixVignetteRadial is read from OpcodeList2 and, when that has none, from OpcodeList3,
  where DJI writes it. Against each file's own camera preview, the edges of three raw.pixls.us
  files (Leica Q, DJI Mini 2, DJI Phantom 4 Pro) correlate at 0.61 / 0.58 / 0.73 corrected,
  against 0.34 / 0.41 / 0.29 uncorrected. A GainMap in OpcodeList3 (DJI's colour shading: red
  +16% at the corners) is still not applied. The opcode lists are read by one new module,
  `dng_opcodes.rs`, which the phone gain maps now share. From RAWmakase @ `a8c5d6d7` (v0.4.0,
  MIT): `vignette` and `warp` in `crates/rawmakase-model/src/dng.rs`.
- **Sony photos can use the camera's own lens correction too.** An ARW stores the lens's
  distortion, colour fringing and corner shading for the shot in its raw SubIFD (0x7032,
  0x7035, 0x7037). The same switch in Crop > Lens Correction applies it; it starts off, as
  Lightroom is not known to apply Sony's tables by default. Sony's tables carry the camera's own
  framing (a ZV-1's distortion is 1.077 at the centre), so the correction zooms only as far as the
  photo's edge needs, not to the whole frame's corners: against the camera's own JPEG in three
  raw.pixls.us files (ZV-1, RX100 VII, A6400) the edges correlate at 0.83 / 0.82 / 0.68 this way,
  0.15 / 0.60 / 0.54 with the whole-frame zoom, and 0.29 / 0.71 / 0.24 uncorrected. From RAWmakase
  @ `a8c5d6d7` (v0.4.0, MIT): the Sony reader in `crates/rawmakase-model/src/lens/embedded.rs`,
  `fill_scale_within` in `crates/rawmakase-model/src/optics/mod.rs`, and which fill each source
  takes in `crates/rawmakase-engine/src/develop/image_space.rs` (RAWmakase #533).
- **Fujifilm photos get the camera's own lens correction.** Every RAF carries the lens's
  distortion, colour fringing (lateral chromatic aberration) and corner shading for the focal
  length and aperture of the shot, and Lightroom applies them by default; Argentum read none of
  it. A new switch at the top of Crop > Lens Correction, "Camera's built-in correction", applies
  it in the lens warp, in place of the lensfun profile while it is on, with the Distortion,
  Chromatic Aberration and Vignetting switches and amounts acting on it. It starts on for a
  Fujifilm photo with no edit yet; an edit saved earlier keeps its look until the switch is
  turned on. The vignetting gain is raised to the power 0.85, which RAWmakase measured as
  matching Lightroom's corners. Distortion that pushes the corners outward zooms in just enough
  to keep them inside the sensor (5% on an X-S10 zoom sample). One module in the engine
  (`rm_lens_builtin.rs`), `mods/lens_builtin.rs` and `argentum/lensBuiltin.ts`. From RAWmakase
  @ `a8c5d6d7` (v0.4.0, MIT): `crates/rawmakase-model/src/optics/mod.rs` (the correction model
  and its fill scale), `crates/rawmakase-model/src/lens/embedded.rs` (the Fujifilm reader) and
  `crates/rawmakase-engine/src/develop/image_space.rs` (the position map and its inverse).
- **Relight and Fog, from RapidRAW 1.6.6.** Edit > Effects has two new parts after Lens blur.
  Relight places Lamp, Spot or Sun lights on the photo, dragged and aimed on the photo, lit
  along each surface's direction, with cast shadows. Fog thickens with distance and lies low
  on the ground. Relight's surface-direction model (MoGe-2 ViT-S, 141 MB) is fetched only
  after asking; Fog uses the depth model Lens blur already uses. `relight.rs`, `fog.rs` and
  `effect_maps.rs` are RapidRAW's as of `52138689`; the panel and handles are ported into
  Argentum's Effects panel and canvas.
- **Auto straighten.** Crop > Rotation has an Auto button beside Level, also Shift+S in the
  photo view and the right-click menus, for one photo or a selection. It levels the photo from
  its straight lines and refits the crop, as one undo step. RapidRAW's `auto_straighten.rs`
  (`52138689`), with the `lsdetect` crate.
- **Movable centre for Vignette and Centre.** Touch one of their sliders and a pin appears on
  the photo; drag it to move the effect's centre, double-click to put it back.
- **Print resolution in Export > Size**, for JPEG, PNG and TIFF, with the print size it gives.
  Argentum's TIFF writer stores it (`mods/export_precision.rs`).
- **Exports carry the rating, colour label and keywords** as XMP in JPEG, PNG and WebP when
  Keep metadata is on, with complete EXIF and the capture time zone for RAW sources (RapidRAW
  `44f77264`, `f090c3f9`, `0baaae42`).
- **Highlight-protecting camera modes are given back in DNG Standard.** Photos shot with
  Fujifilm DR200/DR400, Canon Highlight Tone Priority, Nikon Active D-Lighting or HLG, Olympus
  gradation or Pentax D-Range were recorded darker on purpose; DNG Standard now adds those stops
  back on top of its per-camera offset, and a note under Rendering says which mode and how
  much. Canon's Auto Lighting Optimizer is not counted, as Adobe does not count it. The reading
  is RapidRAW's (`baseline_exposure.rs` @ `bc7791a4`); `mods/highlight_protection.rs`.
- **{original_filename_number}** in file names: the camera's frame number (DSC08408 gives
  08408). A name pattern that could repeat now warns instead of silently adding a number.

- **Subject, Sky and Background masks, from RAWmakase.** One click: Subject finds the photo's
  people and animals, Background is everything else, and Sky finds the sky and stops where it
  ends, leaving white-washed and blurred backgrounds alone. They take the old Sky's place in
  the add-mask menus; Foreground stays, and masks made before keep drawing as they did.
- **Object selects by clicking, on the same model.** Click what you want; click again to add a
  part, Alt-click to leave one out, or drag a box round it. Green and red dots show the clicks,
  and Start over clears them. It answers from the same analysis as Subject and Sky, about a
  second and a half a click on a 24 MP photo. The old Object (SAM 1, one box) leaves the menu;
  masks made with it keep working. The code is RAWmakase's selection crate, taken whole
  (`crates/rawmakase-inference` @ `7c3c81a`, MIT, in `src-tauri/rm_selection`), running SAM 2.1
  Hiera small (Meta AI) and DETR ResNet-50 panoptic (Facebook AI Research), both Apache-2.0, on
  the app's own ONNX Runtime. The models (271 MB) are fetched only after asking and saying the
  size, from pinned commits, each checked against its SHA-256; files already in the model
  folder's `rawmakase-selection` are used as they are. They are let go after five idle minutes,
  like the other mask models. `mods/selection.rs`, `mods/selection_models.rs`,
  `argentum/selection/`.
- **The Modern window**, now the only arrangement: the whole app in one window built from
  today's panels, in Lightroom Classic's neutral greys (Grey, Deep or Light, in Settings >
  General > Theme). One bar per edge:
  - The top bar is also the title bar: folder back and forward, the path (click it or press
    Alt+D to type or paste one), the grid's sort, size and View options or the photo's undo,
    Before, Clipping, RGB and zoom, the panel toggles, a Settings gear that never hides, and
    the window controls.
  - The left column holds the Navigator while a photo is open, the folders (Favourites,
    Recent, and This computer with every disk and its free space), each with the number of
    photos directly in it, and the history, with a line above it that drags. The Navigator
    can float over the photo instead, staying wherever it is dragged. The folder tree works
    from the keyboard: Up and Down move and open, Right opens a folder, Left closes it or goes
    up, Enter opens a disk.
  - The centre shows the folder's grid or the open photo; opening a photo changes nothing
    else. The filmstrip runs under it in both, with the status line in its header.
  - One right panel holds the histogram, an Info line and six tabs over today's tools:
    Edit, Crop, Looks, Masks, Retouch and Export. The Info line shows the photo's ISO,
    aperture, shutter and focal length; clicking it, or I, shows the photo's whole metadata
    at the panel's full height, and any tab goes back. In the grid, Edit shows the selected photo's
    settings read-only; Enter opens it. The Crop tab's panes (Aspect Ratio, Rotation,
    Orientation, Transform, Lens Correction) fold by their headings and stay folded; the
    Transform and Lens Correction cards inside them no longer fold a second time.
  - There is no start screen: the window opens on the folder last open. Settings opens in
    the centre with its own sections, including AI and models, My Gear and About; Esc closes
    it and Ctrl+, opens it.
  - Settings > General > Panels sets the text size (Small, Normal or Large) and the row
    spacing. The panels are always compact; the Layout choice and the Compact panels switch
    are gone.

  `src/argentum/modern/`, `mods/drives.rs`, `mods/recent_folders.rs`,
  `mods/modern_start.rs`.
  - The disk list is adapted from Shoebox `src-tauri/src/volumes.rs` @ `c34c8bc` (`mounted`,
    `query`, `quiet_errors`). Network drives are left out; on macOS and Linux it lists
    `/Volumes`, or `/` and the usual mount folders, without free space for now.
  - A folder's number is the photos directly in it, read from its own listing
    (`mods/folder_counts.rs`), never by walking the folders below as the library's count
    does, so no disk is scanned and no file kept in the cloud is downloaded. Classic's
    photo-count setting does not apply.
  - The folder tree's keys are adapted from Shoebox `src/components/Sidebar.tsx` @
    `bccfd70` (`nav`, `onFocus`, `onKey`); moving onto a folder with the arrows also opens
    it, as Explorer's tree does.
- **Snapshots**, in the left column above History: + (or Ctrl+N) keeps the open photo as it
  is now under a name you type; a click puts it back as a History step, so Undo takes it off
  again; a double-click renames it; a right-click renames, updates or deletes it. Newest
  first, with when each was made. They are kept in the photo's sidecar beside the edit
  (`agSnapshots`), so undoing never removes one. In the grid the selected photo's show
  read-only. The line above it drags to make the list taller or shorter, as History's does,
  and the height is remembered. The behaviour is RAWmakase's (`src/app/snapshots.rs` @
  `7c3c81a`), which keeps them in its catalog. `argentum/snapshots/`, `mods/snapshots.rs`.
- **The Looks tab: presets, LUTs and film looks in one place.** Search, Import for every
  format (.rrpreset, .xmp, .lrtemplate, .cube, .3dl, .png, .jpg, .tiff), and chips for
  All, Favourites, Presets and LUTs. The groups are Presets · RAWmakase (20 that come with
  Argentum, in Color, Creative, Curve, Grain and Vignetting), My presets with their
  folders, and LUTs, whose first folder is Film · spektrafilm, the six film LUTs by Andrea
  Volpato (CC BY-SA 4.0) that RapidRAW ships; the names say where each comes from. Pointing at a look shows it on the photo; a
  click puts it on and a click on the one on takes it off; the star keeps it in
  Favourites. On this photo lists what is on, with a LUT's Strength and a preset's
  Amount (0 to 200%). Amount now starts from the photo as it was and moves only what
  the preset changes: dragging it to 0 used to put every default back, crop and masks
  included. The LUT block left Effects, whose eye, reset, copy and paste no longer touch
  the LUT. Community presets are in the tab's ⋯ menu, with New preset folder. Save makes a
  preset of the photo's settings, or a LUT file of them through the Export tab set to CUBE.
  A preset of yours has its whole menu on a right-click: Configure, Update with the current
  settings, Duplicate, Move to a folder, Export and Delete; a folder Rename, Export and
  Delete. A LUT folder renames and deletes the same way, a LUT moves to another folder, and
  New LUT folder is in the ⋯ menu. Renaming Imported moves its LUTs into a folder with the
  new name, and the next import starts Imported again. What comes with the app (Film and
  the preset groups) says so on a right-click, and a preset or a whole group copies into My
  presets, where it can be renamed and changed. LUTs are listed by name with numbers in
  order (Portrait2 before Portrait10). In the grid, a click puts a look on every selected photo, and the note that says
  so has an Undo. `argentum/looks/`,
  `mods/looks.rs`, `resources/presets/rawmakase` (MIT, `assets/presets` @ `7c3c81a`).
- **DNG Standard, a RAW rendering in the style of Lightroom's default** (RAW >
  Rendering). The default picture of a DNG renderer: the camera's exposure offset
  and Adobe's default tone curve, applied in linear ProPhoto RGB to each pixel's
  largest and smallest channel with the middle one kept between them, so a hue
  holds as the curve brightens it. RapidRAW's own RAW rendering - the sRGB curve, a
  1.1 gamma lift and a gentle S - is about a stop darker in the mid-tones and flat
  next to Camera Raw, and the decoder is not why. `mods/dng_standard.rs`.
  - The exposure offset is a DNG's own BaselineExposure, else the body's row in a
    table of 230 bodies across 10 makes from RAWmakase `data/cameras.toml` @
    `7c3c81ab` (MIT, `mods/dng_baseline.tsv`). A body not listed takes its make's
    median, then the table's. Below a Sony body's lowest native ISO a stop comes
    off, as in Camera Raw. Not read yet: Canon Highlight Tone Priority and
    Fujifilm DR modes; and the values were measured against LibRaw's white level,
    not rawler's (updated to RAWmakase 0.4.0's, see Changed).
  - The tone curve is the DNG SDK's ACR3 default curve as RawTherapee ships it,
    `rtengine/dcp.cc` @ `ca9234b1` (Adobe DNG SDK licence, `docs/licenses/`).
    RGBTone is ported from RawTherapee `rtengine/curves.h` @ `46b92d8e`
    (`rt_rgb_tone`).
  - The photo's curve travels as its 16-point RAW tone curve, within 0.45/255 of
    the exact one; on the GPU the whole render is within 0.77/255 of a CPU
    reference.
  - Enlargements carry it in the luma curve, per channel, so saturated colours
    land a little stronger there.
- **A tone stage in the shader** (`ag_stage_tone`): one call after RapidRAW's
  tone-mapper chain replaces the two RAW tone blocks that were inside their
  `main()`, so their chain is upstream's again, line for line. shader.wgsl hooks:
  2 to 3.

### Changed

- **Color Grading renders as Lightroom's, for new edits and Looks.** Today's Color Grading
  (RapidRAW's) adds a fixed tint, cut to a tenth in the shadows, so it showed only in the darkest
  tones: Lightroom's split toning at Saturation 25 left a mid-grey untinted. A new grading, the
  photo's or a mask's, and every Look now render with RAWmakase's measured copy of Lightroom's:
  Camera Raw 18.7's own renders of a colour chart under about 740 grading settings, fitted as a
  curve per channel of linear ProPhoto RGB, kept as text in
  `src-tauri/engine/src/rm_color_grade_curves.tsv` and generated from RAWmakase v0.4.1 by
  `scripts/rawmakase-grade-curves.mjs` (check:merge warns when a newer release is out). It runs
  where Lightroom grades, after the tone curve, the Curves panel and a LUT, with the same wheels and
  sliders in the same units. A grading saved before this keeps its look: the panel marks a Color
  Grading block (`agLightroom: 1` inside `colorGrading`) the first time one that grades nothing is
  changed, the Looks are marked as they are read, and an unmarked block that grades something
  renders with RapidRAW's grading as before. On an 18% grey, Sepia's warmth (red minus blue) goes
  from 0.0002 to 0.057. Through the shader, each test colour is within 0.001/255 of the engine's
  grading, and the engine's copy gives RAWmakase's own gains bit for bit on 4,000 random gradings.
  Looks it changes: Color > Cool, Matte and Warm, Creative > Faded Gold and B&W > Sepia;
  Creative > Concrete Haze has no grading. From RAWmakase @ `5a2414b4` (v0.4.1, MIT;
  these files are unchanged since v0.4.0): `crates/rawmakase-engine/src/develop/color_grade_curves.rs`
  and its `color_grade_curves.bin`, a test from `color_grade.rs`, `bracket` from `parametric.rs`,
  and `grade` from `gpu/develop.wgsl`.

- **The editor renders 20-45x faster on Vulkan and DirectX 12, so Processing Backend = Auto is
  fast again.** The main shader passed the tone curves (the RGB curves and the RAW tone curve of
  DNG Standard and Base Curve) and the eight HSL bands to its functions as arrays. wgpu copies
  such an array to the stack for every pixel, and on Intel's Vulkan and DirectX 12 drivers that
  copy was most of the render: on an Intel Arc 140T a 3328x2219 slider move took about 690 ms on
  Vulkan (what Auto picks) and 160 ms on OpenGL; it now takes 16 ms on Vulkan, 17 on DirectX 12
  and 24 on OpenGL, and 64 ms instead of 3.1 s at 6960x4640. The curves are now read point by point
  from the adjustments buffer, from RapidRAW `src-tauri/src/shaders/shader.wgsl` @ `cc8200a9`
  (after 1.6.6), with its unrolled noise-reduction taps from the same commit; the HSL bands travel
  as vectors (ours). A straight-line curve is now recognised once per render instead of on every
  pixel, and the curve passes share one call site, which DirectX 12's software renderer (WARP)
  needs to compile the shader. Renders match the old ones: bit for bit on OpenGL and DirectX 12
  for an untouched edit, and otherwise within one level in 8 bits, on a few hundred of 7.4
  million pixels with curves and HSL in use. A test now refuses any shader function that takes an array.
- **A photo you open and leave unedited no longer gets a sidecar file**, and one reset back to
  its starting values loses it, unless it holds Snapshots, a rating, a label or tags (RapidRAW
  `8f41c5ec`).
- **The crop starts locked to the photo's own shape** as a value of its own, so an untouched
  photo matches the defaults (RapidRAW `2fb1ce0d`, `a39f3302`).
- **Batch Auto adjust runs on RapidRAW's new batch code** and still turns on automatic lens
  correction.
- **Copying a section never carries a photo's depth or surface maps** (Lens blur, Relight, Fog):
  another photo works out its own.
- **Faster RAW opening**: post-processing and pixel conversions run in parallel (RapidRAW
  `61169527`).
- **Shortcuts follow the keyboard layout**, and newly dropped masks land on their target row
  (RapidRAW `bb98416d`, `5ccc9dab`).
- **Denoise Image blends its tiles seamlessly** (RapidRAW `f3dd79ae`; their live AI denoise is
  not taken).
- Lensfun database, Italian and Russian translations updated from RapidRAW 1.6.6.

- **Nikon High Efficiency and High Efficiency★ RAWs open.** These NEFs (Z 8, Z 9, Z f, Z 6III and
  other recent bodies) hold a JPEG XS codestream, which rawler refused. The rawler fork now
  decodes it with RAWmakase's decoder (`crates/rawmakase-raw/src/codec/nikon_he.rs` @ `a8c5d6d7`,
  v0.4.0, MIT), added to the fork as `decompressors/nikon_he.rs` with only its error type
  adapted; rawler is pinned to the fork commit `df7f3aec` (branch `feat/nikon-high-efficiency`,
  on `b83a9e07`). Checked on raw.pixls.us's Z 9 samples of both kinds: 8280×5520, black 1008,
  white 15892, the camera JPEG's picture.
- **Phone DNGs get their lens-shading correction.** Phones record the darkening and colour cast
  toward the corners as GainMap opcodes in the DNG and leave them to the renderer; Lightroom
  applies them, rawler did not. Argentum now applies OpcodeList2's gain maps to the decoded
  samples, around their black and clipped at white, before anything reads them. On a Pixel 6
  Pro DNG all four maps apply and the corner's four mosaic colours, from 0.39-0.48 of the
  centre's and uneven, come out within 0.06 of each other. One module in the engine
  (`dng_gain_map.rs`), called from the engine's decode and the app's (`mods/decode.rs`). From
  RAWmakase `crates/rawmakase-raw/src/container/dng/opcodes.rs` @ `a8c5d6d7` (v0.4.0, MIT):
  the list's layout, the GainMap's parameters and its bilinear gain; the other opcodes in that
  file wait for a file that needs them.
- **DNG Standard follows a Canon photo's levels, as Camera Raw does by ISO.** Adobe takes a
  Canon's white level by ISO speed and moves its exposure offset with it; RAWmakase 0.4.0
  records each body's sample levels and shifts the offset by the difference. Argentum now
  does the same with the levels the photo is actually decoded with, which also makes the
  Canon offsets hold with rawler's white level. An EOS R6 Mark III photo at ISO 640 moves
  from +0.25 to +0.26 EV. 58 Canon bodies carry levels (`mods/dng_baseline.tsv`, a fifth
  column); other makes are unchanged. Canon sRAW / mRAW keep the full-size row: Adobe's own
  sRAW offsets make up for Adobe's sRAW scale, and Argentum's sRAW measures within 0.03 EV of
  its full RAW (95 EOS 5D Mark II photos, each against its camera JPEG). The levels are read from the
  file's headers without decoding its pixels, only for a body whose row has them. The engine's
  copy (`tools/ag_dng_standard.rs`) takes them from the decoded image and gives the same
  number. From RAWmakase `white_shift` and `data/cameras.toml` @ `a8c5d6d7` (v0.4.0, MIT).
- **Colours on a Canon EOS 1000D and a Pentax K-S2 are corrected, and two Nikon white levels.**
  The rawler fork carried a dropped digit in the 1000D's colour matrix (`-0.0114` for Adobe's
  `-0.1140`) and a matrix on the wrong scale for the K-S2 (first row `5.29, -1.66, -0.75`). The Nikon
  Z 9 and Z fc had the older bodies' white levels (3880 / 15520) where Adobe's are 3972 / 15892.
  Values from RAWmakase `data/raw-matrices.toml` and `data/raw-cameras.toml` @ `a8c5d6d7`
  (v0.4.0, MIT), which carry Adobe DNG Converter 18.7's output. rawler is pinned to the fork
  commit `b83a9e07` (`fix/rawler-data-matrices-nikon-white`, one commit on the C50 data commit
  `16b9b010`, four data files).
- **DNG Standard's exposure offsets now follow RAWmakase 0.4.0.** 97 of the 230 bodies change:
  Canon 45, Nikon 27, Olympus 12, Fujifilm 9, OM Digital 2, Hasselblad 1 and the Panasonic
  GH5S. RAWmakase stopped adding a correction for LibRaw's white level when it began
  reading Adobe's levels, so its values are Adobe's own BaselineExposure. The table is now
  theirs row for row. The shifts are mostly -0.05 to -0.1 EV for Nikon, Olympus and OM,
  larger for Canon (median about -0.25 EV) and -1.1 EV for the GH5S. `mods/dng_baseline.tsv`,
  from RAWmakase `data/cameras.toml` @ `a8c5d6d7` (v0.4.0, MIT). Known limit: Adobe moves a
  Canon's offset with ISO speed and RAWmakase now follows that per photo; this table keeps
  one value per body. Not yet checked on rendered photos.
- **Copy settings, Paste and presets carry the camera profile, the RAW rendering and
  Argentum's sharpening.** They were left behind before. The Copy & Paste Settings dialog
  lists the first two as Camera Profile & RAW Rendering, and the sharpening with
  Sharpness; both are on for everyone. A profile reaching a photo from another camera
  is left out there, since a profile made for one body renders another wrong. The RAW
  rendering travels without the curve fitted to the photo it came from: each photo
  fits its own when opened, and a curve left from another rendering is no longer used.
  Auto white balance is carried too, and measured again on each photo it lands on: Auto
  measures the photo it is pressed on, so its numbers alone would give the rest that
  photo's light. Each photo pasted onto is decoded and measured in the background, a note
  counting them, and gets its own Auto; the open photo measures its own as soon as it holds
  one from elsewhere, which also covers a preset kept with Auto and any photo a paste
  missed. An Auto set before this version is taken as its photo's own; press Auto again on
  the photo to copy from. `mods/copy_keys.rs`, `mods/raw_tone.rs`,
  `mods/profile_correction.rs`, `mods/auto_wb_photo.rs`, `argentum/autoWhiteBalance.ts`.
- **The Edit panel follows the order a photo is corrected in.** A new Profile section
  comes first: the camera profile, the RAW rendering, the tone mapper (Basic or AgX)
  and highlight recovery, which were a RAW card inside Color and a switch inside
  Basic. Basic then starts with the white balance row (the preset menu, an Auto
  button and the picker) with Temperature and Tint, then Tone (Exposure to
  Brightness), then Presence (Clarity, Dehaze, Structure, Centre, Vibrance,
  Saturation). Color keeps Hue, the colour mixer, colour grading and calibration;
  Details keeps sharpening and noise reduction, with a Denoise… button under it, and
  gains Enhance with Restore & Enlarge…, which the Modern window had no way to open.
  The manual fringe sliders (Red/Cyan, Blue/Yellow) moved to Crop > Lens Correction
  as Manual fringe, where the lens reset puts them back and Details' eye no longer
  turns them off. Showing temperature in kelvin is a switch in Settings > General, no
  longer a K button on the white balance row. The panel's header gains ⋯: Copy
  settings, Paste settings, the Copy & Paste Settings dialog, Auto-sync and Customize
  Panels. Details' reset, copy and paste now take Argentum's sharpening too. Masks show
  the same arrangement. Each section's eye, reset, copy and paste follow what it
  shows: Basic's eye now turns off white balance and Presence too, so an edit saved
  with Color or Details turned off shows those sliders' effect again. Reset all also
  puts the profile, the RAW rendering and Auto white balance back. On a RAW the tone
  mapper is greyed out unless Rendering is Default, since every other rendering sets
  the tones itself.
  `argentum/editSections.ts`, `argentum/ProfileSection.tsx`, `argentum/DetailTools.tsx`,
  `components/adjustments/WhiteBalance.tsx` (their white balance code from
  `Color.tsx` @ `79c2a46b`, moved unchanged).
- **Effects > Vignette is now called Post-Crop Vignette**, in every language and
  in the edit history. Its rendering is unchanged: it was already drawn on the
  cropped photo, centred on the crop. The new name sets it apart from lens
  vignetting correction under Crop & Transform, which works on the whole frame.
- **A RAW starts in DNG Standard, with Camera Raw's default sharpening.** A RAW
  whose edit names no rendering - never edited, reset, or given settings pasted
  onto it unedited - renders in DNG Standard, and DNG Standard brings Sharpen 25 at
  radius 1 until an amount is set. On 200% crops of an R6 Mark III frame that
  matched RAWmakase 0.3.3's render of Camera Raw's defaults (Amount 40, Radius
  1.0), and added less grain than it at ISO 12800 and 32000. The editor,
  thumbnails, exports and enlargements read an edit the same way
  (`raw_tone::mode_of`, `sharpen::from_json`); thumbnails are rebuilt once
  (pipeline 7).
  - Edits that chose RapidRAW's Default keep it, and every edit saved since
    2026-09-22 names its rendering. Edits saved before then name none and now open
    in DNG Standard; Default under RAW > Rendering gives back the earlier look.

Registered as `dng-standard` and `raw-starting-look`, with 29 and 38 decisions
against the 1.6.5 review.

### Internal

- **Merged RapidRAW 1.6.6** (`79c2a46b..52138689`), the first release reviewed per release: one
  verdict per feature in `scripts/upstream-decisions.mjs`. Their files are taken as written; the
  render path and today's interface keep ours, with what was wanted copied in. Their staged
  preview pipeline replaces ours; capture sharpening keeps its full-resolution reference in
  `mods/input_stage.rs`. Argentum's starting values live in `src/argentum/defaultAdjustments.json`,
  over their `defaultAdjustments.json`.
- **The ratchet has one way up, by name** (`RAISES` in `scripts/upstream-ratchet.mjs`): the
  line in their `exif_processing.rs` that keeps a sidecar holding Snapshots.

- **The engine library, first step.** `src-tauri/engine/`, crate
  `argentum-engine`: a registry of tools, each one file in `src/tools/` and
  one line in `registry.rs`, and a pipeline that keeps every tool's result
  keyed by its input and its settings, so changing one tool re-runs it and
  the tools after it only. Its own workspace: it builds and tests from its
  folder without building the app, and the app does not use it yet. Three
  tools, copied from the app, which keeps its originals:
  - `dt_highlights`: highlight recovery and blown-highlight settling on the
    sensor data, from `mods/highlights.rs` @ `2bcb953d` - Argentum's code,
    after darktable's opposed reconstruction
    (`src/iop/hlreconstruct/opposed.c`). Bit-identical to the original.
  - `rt_capture_sharpen`: RawTherapee's capture sharpening,
    `rtengine/capturesharpening.cc` @ `c6d04960e14fed89e531cbbdf680021d131b4685`
    with the contrast mask of `rtengine/rt_algo.cc`
    @ `1c2b3f33772eaf491b7bd4d66323ac36d13aa344`, from `mods/sharpen.rs`,
    `mods/sharpen_gpu.rs` and `shaders/sharpen.wgsl` @ `2bcb953d`, on a GPU
    device of the engine's own. Automatic radius and contrast bit-identical
    to the app's; the passes within one half-float step of the app's output,
    the engine keeping f32 where the app stores half floats.
  - `ag_exposure_wb`: exposure and white balance multipliers. Ours.
  - The parity tests compile the app's own files unchanged, in a test-only
    crate (`engine/reference/`), and compare on the same input.
  - The contract the new editor interface talks to it through:
    `src/contract.rs` holds every type the interface sees (tool descriptors
    read off each tool's own settings, the edit document, edit patches,
    views, render tickets and events), and `src/shell/contract.generated.ts`
    is generated from it with ts-rs, with fixtures beside it; a test fails
    when they are stale. The engine's calls - `tools`, `open`, `apply`,
    `view` - return at once and render on a thread of their own,
    latest-wins. A slider tick at preview size is held to a measured budget
    of 25 ms by a test.
  - Then tidied against the code hygiene rules: `apply` returns the edit as
    the engine took it, errors are typed (`ErrorKind`), a patch is merged by
    one pure `Registry::merge` against the registry the render thread uses,
    `Source` and the cache have files of their own, capture sharpening is a
    folder (`radius`, `contrast`, `gpu`) with one `MAX_RADIUS_PX` and no
    second clamp, origins and the fit are each defined once, and the
    TypeScript generator builds only for the tests.
  - Then the frame's place in its view, the transport, and memory. The frame
    event says where the frame sits in the view (`placement`, in device
    pixels), so a fitted photo is not stretched to the viewport, and fixtures
    pin it. The event name, the frame block's header and the shared-memory
    markers are defined once in `contract.rs` and generated to TypeScript.
    A fitted view develops at half size - each 2 x 2 block of a Bayer mosaic
    one pixel, as LibRaw's half-size decode (RAWmakase's previews) and
    darktable's preview pipe do, with rawler's own superpixel demosaic - so a
    screen-sized result no longer takes a sensor-sized picture; highlight
    recovery still runs on the whole mosaic first. The cache counts each
    picture once for as long as any step holds it, shares the photo's
    decoded mosaic instead of copying it, and makes room for a result, and
    for the develop's working memory, before making it; capture sharpening
    keeps a band of the picture on the GPU instead of all of it; `close`
    frees a photo's data and results; the budget is set by the app, 1.5 GB
    until it does. Measured on a 32 MP CR3, held by tests: fitted, 0.57 GB at
    the peak (was 1.25 GB); at 100% with a slider dragged, 1.75 GB (was
    2.66 GB). On a 61 MP photo, 0.82 GB (was 1.83 GB) and 2.19 GB (was
    3.73 GB).
- **The engine's basic tools: white balance, DNG Standard, tone and
  presence.** Enough for the new editor to edit a photo. Each is copied from
  the app @ `c786c146`, whose RapidRAW files are RapidRAW 1.6.5 @ `79c2a46b`,
  and each is held to the app's own code by a parity test on the same input.
  In the app's order: exposure, white balance, tone, presence, DNG Standard.
  All five run on the GPU in one pass, as the app's do.
  - `rr_white_balance`: temperature in kelvin and tint, starting at the
    camera's as-shot light. RapidRAW 1.6.5's Kelvin engine: `white_balance.rs`,
    `read_as_shot_white_balance` in `raw_processing.rs` and
    `apply_white_balance` in `shaders/shader.wgsl`. The as-shot light and the
    cone gains are identical to the app's on every RAW tested.
  - `ag_dng_standard`: Argentum's default RAW look, from `mods/dng_standard.rs`
    and `rt_rgb_tone` / `ag_dng_standard` in `shaders/modules.wgsl`: the
    body's exposure offset (`mods/dng_baseline.tsv`, from RAWmakase
    `data/cameras.toml` @ `7c3c81ab`, read in place) and the DNG SDK's ACR3
    curve (RawTherapee `rtengine/dcp.cc` @ `ca9234b1`, Adobe DNG SDK
    licence) applied with RGBTone (RawTherapee `rtengine/curves.h`
    @ `46b92d8e`). The engine's pass reads the exact curve; the app's shader
    draws a 16-point spline of it. Not read yet: a DNG's own
    BaselineExposure and the stop below a body's lowest native ISO.
  - `rr_basic_tone`: contrast, highlights, shadows, whites and blacks.
    RapidRAW's `apply_tonal_adjustments` and `apply_highlights_adjustment`
    in `shaders/shader.wgsl`, with the log-space guided filter that gives
    them each pixel's detail: the app's `shaders/guided.wgsl`, used in place,
    run as `build_guided_coeffs` in `gpu_processing.rs` runs it (RapidRAW
    `66fa1600`), on the pass's input, before exposure and white balance, as
    the app measures it.
  - `rr_presence`: vibrance and saturation, RapidRAW's `apply_creative_color`.
  - `ag_exposure_wb` is now `ag_exposure`, exposure in stops; its red, green
    and blue multipliers are gone, white balance being `rr_white_balance`.
  - **The pixel pass.** A per-pixel tool declares its WGSL function
    (`Tool::PIXEL`) and packs its settings (`pixel_params`); each run of
    them is one GPU dispatch whose `main` the engine writes from the
    registry, calling them in registry order, so a new one is still one file
    and one registry line (`tests/pixel_tool.rs`). The input goes up once as
    half floats and stays, with its guided filter, as `gpu_processing.rs`
    keeps its input texture; a slider tick writes the parameters, dispatches,
    and reads back the view's frame, cut, zoomed and encoded in the same
    pass. The structure is the app's uber-shader; the calls are the
    registry's rather than a hand-written list. The engine's CPU frame cut
    and encode are gone.
  - The engine's GPU device takes memory in blocks of 8 to 64 MB (wgpu's
    `MemoryUsage` hint) rather than 128 to 256 MB. On an integrated GPU the
    blocks are the process's memory, and the pixel pass's textures and
    readback had each opened one: measured on a 32 MP CR3, the peak while a
    photo is fitted is 0.66 GB with it and 0.74 GB without, against a limit
    of 0.73 GB in `tests/memory.rs`. Closing a photo also lets go of the
    pixel pass's input on the GPU.
  - The pixel pass sends its input to the GPU a 64 MB band at a time, each
    band sent and waited for before the next. Written without a send
    between them, wgpu kept every band's staging copy until the render's
    dispatch: a second copy of the picture, 0.45 GB at 61 MP. The process's
    peak in `tests/memory.rs` for the synthetic 61 MP photo is 2.47 GB with
    it and 2.74 GB without, against a limit of 2.51 GB.
  - `tests/memory.rs` counts what the engine holds instead of reading the
    process's memory: every byte its Rust code allocates (a counting
    allocator in the test) and the GPU memory wgpu holds for its device
    (`Engine::gpu_bytes`, wgpu's allocator report, staging copies
    included). The limit is the design's arithmetic: the cache's budget,
    the mosaic (2 bytes a pixel), the pixel pass's input on the GPU (8) and
    256 MB of bands. Counted, it holds on any machine, so CI checks it on
    every pull request on a software GPU (Mesa's lavapipe). Measured: 2.12
    GB for the synthetic 61 MP photo (limit 2.31) and 1.49 GB for a 32 MP
    CR3 (limit 2.05); the whole process's peak, driver included, is printed
    beside it and no longer checked.
  - A tool can start a photo at the photo's own settings
    (`Tool::defaults_for`, after darktable's `reload_defaults`), and `null`
    in a patch puts a setting back to them.
  - The parity tests run the engine's pass and the app's shader functions as
    they are on the same GPU (`tests/common/app_shader.rs`); tone end to end,
    the app's guided filter included. Each tool has golden renders on local
    RAWs (`tests/golden.rs`). The engine's reference build compiles the
    app's `mods/raw_tone.rs`, `dng_standard.rs`, `profiles.rs` and `dcp.rs`
    as they are, in place of a stand-in for `raw_tone`.
- **Dev and test builds wait less.** Measured on Windows x64 after a one-line
  change in `mods/sharpen.rs`: `cargo build` 26-28 s → 20 s, `cargo test --lib
  sharpen::` 29 s → 26 s; a cold build into a new target directory 739 s →
  633 s. Shipped builds are unchanged.
  - The app library is built as an rlib only. Upstream's staticlib (iOS) and
    cdylib (Android) were written by every desktop build, the staticlib alone
    640 MB, and Argentum builds no mobile app. A mobile build needs them back
    in `Cargo.toml`.
  - Windows x64 dev and test builds link with rust-lld, which ships with Rust
    (`.cargo/config.toml`): the app relinks in 5.6 s instead of 7-10 s.
    Shipped Windows builds keep MSVC's linker.
  - Dependencies carry no debug info in dev builds: the app's PDB is 86 MB
    instead of 194 MB, and a full rebuild of the dependencies took 368 s
    instead of 528 s. A backtrace through a dependency keeps its function
    names but not file and line.
  - `build.rs` does not hash the ONNX Runtime library again once it has been
    checked, and a new git worktree copies it from the main checkout instead
    of downloading it.
  - `npm start` no longer runs the mergeability check, about 45 s before
    every dev window. It is still the required check in CI, and
    `npm run check:merge` runs it by hand.
  - The first build after this change rebuilds every crate once: cargo and
    sccache both key on the linker and the debug settings.
- **The new editor interface, first step.** `src/shell/`, opt-in with
  `?shell=new`; without it the app opens as before. Every tool panel is built
  from the engine's tool descriptors, a widget per setting kind, so a new
  engine tool gets its panel with no interface change (a test proves it).
  Edits are patches with their inverses for undo; renders are latest-wins.
  It reaches the engine only through `src/shell/engine/client.ts`, and until
  the engine is wired into the app that is a fake of the contract that
  paints a made-up landscape. The layout is Lightroom Classic-style. Tests
  with Vitest: `npm run test:shell`.
- **RapidRAW is reviewed once per release, and `check:merge` takes a second.**
  The checks follow Argentum's move to an architecture of its own
  (`docs/ARCHITECTURE.md`).
  - `npm run review:upstream` prints the review of their newest release since
    the last one reviewed: their shader diff first, their render-path changes
    to read, their changes to files our registry entries depend on, and one
    line per feature (from their pull requests) to give a verdict - take, copy,
    skip or decide - with the register entry to paste. Every commit of the
    release must be in a feature, a render-path change is never "take", and the
    list of their files still used when previewing, exporting or masking
    (`stillLive`) may not grow. The per-commit decisions recorded up to 1.6.5
    stay as history.
  - The render path - `gpu_processing.rs`, `lib.rs`, `image_processing.rs` - and
    today's interface - `App.tsx`, the editor and its panels, the adjustment
    sections and the views - are ours to change in place: no line budget, no
    anchor count. Their upstream changes there are read at the release review
    and copied by hand, never merged. A pull request touching any file from
    RapidRAW owes a registry entry, and no per-commit decisions.
  - A ratchet replaces the line budgets: our lines in each other file of
    theirs are recorded in `scripts/upstream-ratchet.json` and may only go
    down.
  - One git process per question instead of about 390: 45-76 s to about 1 s.
- **DNG Standard's camera table is generated.** `scripts/rawmakase-cameras.mjs
  <release>` writes `mods/dng_baseline.tsv` from RAWmakase `data/cameras.toml`
  at one release, named on the table's `# Source:` line; a value of ours that
  differs from RAWmakase's is one entry in the script's `EXCEPTIONS`, with its
  reason and what removes it (none today). A test proves the table is exactly
  what the script makes from the commit it names (v0.4.0 @ `a8c5d6d7`: all 230
  rows unchanged), and `npm run check:merge` warns when RAWmakase has a newer
  release.

## 26.41.12 — 2026-10-11

### Added

- **Restore: undo lens softness at the photo's own size.** For a lens that
  is soft wide open - a 100-400 L at 400 mm on an R6 Mark III - rather
  than missed focus. The model is OmniLens (Jiang et al., Zhejiang
  University): SwinIR pretrained on a library of simulated lens designs, so
  it corrects a lens it has never seen. Code `basicsr/archs/swinir_arch.py`
  @ `89dcbc7ff7e79edc6aea2b58f83750d575c2f4bf` of
  [zju-jiangqi/OmniLens](https://github.com/zju-jiangqi/OmniLens), weights
  `SwinIR_AODLib_EAOD.pth` @ `fb0ded25e63fc2c18f8f59e5519e3a659ea796a7`, both
  Apache-2.0. Converted to ONNX at a fixed 256 px tile and published in
  [akhater/argentum-models](https://github.com/akhater/argentum-models/releases/tag/omnilens-v1)
  (54 MB, SHA-256 `c28ec547…`), downloaded on first use; identical to the
  PyTorch original on random tiles. `mods/restore.rs`.
  - It runs through a **detail mask** (the shape of RawTherapee's contrast
    mask): trained on clean renders, it turns grain in a soft background to
    grit. Tight by default; a Mask slider loosens it, and an
    eye shows it, white where Restore applies, live as the slider moves -
    the mask comes from the photo, so it shows before Enhance. With Restore
    on its own, a new Mask re-blends the kept model output without running
    it again.
  - **Capture sharpening still applies to the restored file.** It is saved
    as a TIFF but is the RAW's light before any sharpening, so it carries
    `agRestore.captureRadius` - the radius measured on the RAW's mosaic,
    which the TIFF no longer has - and `sharpen::from_json` and the
    Sharpening card treat it as a RAW for Capture. Live and adjustable.
  - Chosen by side-by-side on real RAWs: over enlarging models shrunk back
    to size (their detail is under a pixel at 1x - no visible difference), GRL and
    Restormer defocus models (smears, or no change), and HAT (lab tests by a
    hair, 14 min on the GPU for a 700 mm crop). 2.9 s/MP on an Arc 140T with
    DirectML, about 1.5 min for 32 MP, measured through this code.

### Changed

- **Super Resolution is now Restore & Enlarge**: one ✨ window, Restore and
  Enlarge (Off, 2x, 4x) each optional, one file named for what was done -
  `_Restored`, `_Upscaled_x2`, `_Restored_Upscaled_x4` (was `_Upscaled`).
  Ticking Restore sets Enlarge to Off.
- **An enlarged RAW is capture-sharpened first, at the camera's size.** An
  enlargement is an ordinary image, which the editor never
  capture-sharpens, and it started from the RAW before Capture, so enlarged
  RAWs had none. The order was measured, not assumed: Capture after the
  enlargement doubled and outlined whiskers; Capture before Restore was
  softer on all six test photos. So Restore, then Capture, then Enlarge;
  output Sharpen stays live on the enlarged file. The editor's own engine on
  a texture of its own, 0.05 s/MP (`mods/capture_bake.rs`). Enlarge keeps
  Real-ESRGAN: a photo-trained RealPLKSR model was better on fur and
  invented texture in soft areas on 4 of 7 photos.

- **A mask's Sharpen has its own Radius, Mask and Threshold**, under More,
  and an eye that shows where it lands: inside the mask, where its Mask lets
  it. It used the main card's Sharpen radius and mask, which were hidden
  until the photo's own Sharpen Amount was raised, so a mask could not be
  sharpened differently from the photo. Each mask runs the same unsharp mask
  passes with its own settings, one after another, only in the tiles its
  pixels reach (`mods/sharpen_gpu.rs`, `LocalSharpening.tsx`). A new mask
  starts from darktable's Sharpen defaults; an existing one keeps its amount.
- The main Sharpening switch is the whole photo's: it no longer turns a
  mask's own Sharpen off.

## 26.41.11 — 2026-10-10

### Added

- **Sharpening, replacing RapidRAW's.** Theirs is a single unsharp mask with
  nothing to show or decide where it works. One card at
  the top of Details takes the place of their Sharpening section (one tag in
  `Details.tsx`; `src/argentum/sharpening.css` hides what follows it), with a
  switch for the whole thing, to compare before and after. Two stages:
  - *Capture sharpening*, RAW only: RawTherapee
    `rtengine/capturesharpening.cc` @ `c6d04960e14fed89e531cbbdf680021d131b4685`.
    Richardson-Lucy deconvolution with a gaussian PSF, a corner boost and the
    iteration check. **Auto radius** is RawTherapee's `calcRadiusBayer` /
    `calcRadiusXtrans`, read from the RAW's own photosites at decode - on the
    picture area only: the masked sensor border first gave 0.33 on every
    photo.
  - *Sharpen*: darktable `src/iop/sharpen.c` @ `39df8424e67b99c870fc346ea8f5f241a4620575`
    and `data/kernels/sharpen.cl` @ `d94025f78bea50d3499e5032aa6ea6576160b842`.
    An unsharp mask on L with a threshold, off until its Amount is raised; on
    a JPEG it is the only stage.
  - *Where it sharpens*: RawTherapee's contrast mask, `rtengine/rt_algo.cc` @
    `1c2b3f33772eaf491b7bd4d66323ac36d13aa344` (`buildBlendMask`,
    `calcContrastThreshold`), with its automatic threshold measured on the
    photo's flattest tile. Sharpen can use capture's mask or its own.
  - **An eye on each stage shows its mask**, white where it sharpens - the
    question "what is this actually sharpening". It closes when its stage or
    sharpening is switched off.
  - **A mask's own Sharpen**, -100 to 100, on the same engine
    (`src/argentum/LocalSharpening.tsx`), negative to soften.

  Radii are in full-resolution pixels, so 100% shows what the export gets;
  capture is skipped below a 0.25 px sigma, which is fit-to-screen on most
  photos. It runs on the GPU before their shader, through one new anchor in
  `gpu_processing.rs` (`mods/input_stage.rs`, `mods/sharpen_gpu.rs`,
  `shaders/sharpen.wgsl`; settings in `mods/sharpen.rs`), in 1024 px tiles.
  Exports are sharpened at full resolution; thumbnails and the LUT export
  are not.

  **What it costs**, measured on an Intel Arc 140T (`where_the_time_goes`):
  0.6-0.9 s on a 32 MP photo at 100% when a sharpening setting changes, and
  nothing otherwise - the result is kept per photo. Panning sharpens only the
  strip that comes into view, and the mask eye, their thumbnails and the
  smaller preview they draw while something moves no longer make it start
  again. It holds about 0.3 GB of graphics memory for a 32 MP photo, more
  while zoomed in or showing a mask; on a laptop without its own graphics
  memory that is RAM.

  It works on every backend RapidRAW offers - Vulkan, DirectX 12, its
  software adapter and OpenGL - and the GPU tests run on each. OpenGL needed
  two things: a texture array of at least two layers, and the work submitted
  in pieces, because wgpu panics in any thread that waits a second for the
  GL context, and the first version took the preview worker down with it. A
  failure in this stage now skips sharpening for that frame and says why in
  the log, rather than stopping the preview until a restart.

### Changed

- **RapidRAW's Base Pre-Sharpening goes to 0, once.** It sharpened every RAW
  as it loaded, at 0.35, and capture sharpening would stack on top of it. Set
  on the first start of this version (`retire_presharpening_once`);
  turn it back up and it stays.
- A photo edited with their old sharpening keeps it, and the card says so,
  with a button to remove it.
### Fixed

- **Lens auto-detection reads the lens from every kind of file**, not only CR2.
  An EF 100-400mm f/4.5-5.6L IS USM on an EOS R body said *Lens profile not
  found*, and the lens never reached My Lenses. The 26.37.6 fix read Canon's
  MakerNote through `kamadak-exif`, was tested on a folder of CR2s, and never
  ran on a CR3: kamadak cannot open ISO-BMFF. A CR3's lens came only from
  rawler's table of lens ids, which knows this lens under another id than the
  `183:0` the camera reports - *No lens definition found in database* - and
  dropped the name the camera had written in plain text beside it.
  - `mods/lens_name.rs` reads the standard EXIF `LensModel` wherever the
    format keeps its EXIF: a container kamadak opens (TIFF-based RAW, DNG,
    TIFF, JPEG, HEIF), a TIFF under a private magic number (ORF, RW2), or a
    TIFF block inside anything else (CR3, RAF). Canon's MakerNote stays as the
    last resort, now with offsets taken from the EXIF block, so it works in a
    JPEG too. It only ever fills a blank `LensModel`.
  - Their `exif_processing.rs` keeps one call into ours, moved from inside
    `extract_metadata` - which only RAWs kamadak opens reached - to around
    `read_exif_data_from_bytes` in `read_exif_data`, the point every format
    passes on a fresh read, before the result is cached.
  - EXIF cached before this fix is keyed on the photo, not the app, so it kept
    its empty lens - and their `resolve_lens_params_in_adjustments` detects the
    lens again from that cache on every save and deletes it from the edit when
    none is there (*Mount fallback ... giving up* in the log). When an opened
    photo's EXIF has no lens, `useAutoDetectOnLoad` calls `recover_lens_name`,
    which reads it from the file and writes it into the cache through their
    `update_exif_fields`, the path the metadata panel's edits take. Every other
    field, including any the user edited, is left as it was.
  - Verified on six EOS R6 Mark III CR3s through the whole chain to the lensfun
    match: `AG_LENS_DIR=... cargo test --lib mods::lens_name -- --ignored`.

## 26.41.10 — 2026-10-10

### Fixed

- **Argentum no longer holds 8-12 GB after an AI mask or an AI erase.**
  Measured on a Canon R6 Mark III raw (6960x4640): one AI erase took the
  process from 2.6 GB to 12 GB, and it settled at 8.5 GB, where it stayed after
  a change of photo. The eraser was blamed first; unloading it freed nothing.
  The AI mask models were 6.1 GB of it - SAM's encoder and decoder, U2-Net, the
  sky model and Depth Anything, which load together and which the eraser uses
  to select. ONNX Runtime's CPU memory arena keeps the most working memory a
  session has ever needed for as long as the session lives, and RapidRAW keeps
  its sessions until the app quits. Two changes:
  - Every model session is built without the arena (`mods/ai_session.rs`), so
    a run's working memory goes back when the run ends. Their
    `ai_processing.rs` imports it, on an anchor taken deliberately,
    and its eight `Session::builder()` calls became `session_builder()`, one
    line for one. The same test now settles at 3-4.5 GB with the models loaded,
    peaking at 7.2 GB for a few seconds while they run.
  - Models nothing is using are unloaded (`mods/memory.rs`, started from the
    startup anchor): the eraser, denoise and tagging after a minute, the mask
    models after five, because they are clicked again and again and take a few
    seconds to load. A model is in use while anything besides `AiState` holds
    its `Arc`; their `get_or_init_*` loads it again on the next click. With
    nothing loaded the same photo sits at 1.5-2.5 GB.
- **Freed memory goes back to Windows.** mimalloc keeps freed pages for reuse;
  `mi_collect(true)` every 20 seconds returns them, 1.3 GB at once in testing.
- **A photo no step changed is no longer copied.** `compute_patched_and_warped`
  and `compute_full_transformed_res` copied the 500 MB photo even when every
  step handed it back unchanged: no patches, lens correction or blur; no crop,
  rotation or flip. They share it now - two lines of their `lib.rs`, one for
  one. Worth sending upstream.
- **An enlarged raw opens looking like the raw.** Found on an R6 Mark III
  photo balanced to 4076 K: its 2x enlargement opened plainly blue, with
  crushed greens, without its three inpainted areas and without its masks.
  The enlargement holds the framed photo and its look travels in the sidecar,
  but the editor reopens it as an ordinary image, and none of the four
  survived that (`mods/super_resolution.rs`, no file of theirs changed):
  - *White balance* was copied as an absolute value. A raw is adapted from the
    camera's as-shot white, an ordinary image from D65, so the same number
    moved the enlargement further. It is restated as the white that adapts D65
    by the gains the raw got, relative sliders folded in.
  - *Tone was applied twice.* The enlargement held their CPU preview curve and
    the carried look ran the view transform again on top. A raw is now enlarged
    as linear light, encoded with the exact inverse of the editor's non-raw
    sRGB decode. The basic tone mapper's raw branch, or a raw tone curve, which
    ordinary images do not get, rides in the luma curve that follows it; AgX is
    the same for both. An unedited raw gets a sidecar too, for a raw's default
    look.
  - *Inpainting* was stripped with the other source-coordinate keys and never
    composited. It is composited before framing, as the editor does.
  - *Masks* were stripped the same way. Shapes move by the crop and scale with
    the enlargement, and stay editable. AI masks are re-rendered over the
    enlarged frame with the editor's own placement. Colour and luminance masks
    keep their sample point but select again from the enlarged pixels, so
    their edge can differ a little.

  What still differs: light above 1.0 clips in the 16-bit TIFF, a global
  brightness move lands slightly differently, and a scene-referred LUT keeps
  the raw's tone curve out.

### Added

- **The log says where the memory is.** A `[memory]` line whenever the total
  moves by 256 MB: the open photo's copies, the mask cache, results, which AI
  models are loaded, and the rest. It is how the mask models were found.

Registered as `memory-release`, `shared-unchanged-copies` and
`ai-sessions-without-arena`, with 35 decisions against the 1.6.5 review. The
enlargement's five new dependencies are under `ai-super-resolution`, with 27.

## 26.41.9 — 2026-10-09

### Changed

- **A RAW card at the top of the Color panel.** Camera profile, RAW tone
  rendering and highlight recovery were three cards of their own above White
  Balance, the only things in the panel without a heading. They decide how the
  file is read before any slider touches it, so they are now one section,
  "RAW", that folds like White Balance and remembers being folded. Inside it the
  rows read like slider labels, and "RAW rendering" is just "Rendering".
  `src/argentum/RawSection.tsx` uses their AdjustmentSubSection, with an id of
  ours (`argentumRaw`) in their `collapsedTools` setting. Their focus mode
  only folds tools on their own list, so the card follows it from our side:
  opening RAW folds the Color tools, and opening one of them folds RAW. No file
  of theirs changes. The dependencies are registered under `camera-profile`,
  with their 13 decisions against the 1.6.5 review.
- **The linear mask is two lines: full effect, and none.** RapidRAW draws it
  as a dotted centre line at half strength with a dashed line either side, and
  nothing says which side gets the effect; drawing one, the drag starts at the
  middle of the fade and the effect lands behind it. It is now a solid line
  where the effect is full and a dashed line where it has gone, with handles
  marked 100% and 0%. You draw it as a graduated filter: press where the
  effect should be full and let go where it should be gone. A handle moves its
  own end and the lines turn to follow, a line slides on its own (closer is a
  harder edge), and the band between them moves both. Stored exactly as
  RapidRAW stores it, so old masks open in it. `src/argentum/LinearMask.tsx`
  and `linearEdges.ts`. No line of theirs: presses on their mask stage are
  taken by a capturing listener, as the object brush did, and their stage
  canvas is hidden while a linear component is selected. The press also blurs
  whatever had focus, as a press on their stage would, or Delete is ignored
  while the last slider touched keeps it. In the masks panel and the AI panel.
- **The radial mask shows where its full effect ends.** A solid inner ellipse,
  inside their dashed outer one, that follows the Feather slider as it moves,
  and the outer ellipse while it is dragged or resized: that is read live from
  the Konva shape their Transformer holds, since the stored mask only moves on
  release. Their red mask preview is hidden while any slider is dragged, so
  Feather had nothing to show for itself. `src/argentum/RadialFeather.tsx`.
  Both are registry entry `mask-guides`, with its 42 decisions against the
  1.6.5 review.
- **Linear and radial masks fade out instead of stopping.** Both were a
  straight ramp clamped to 0..1, and the clamp leaves a corner at each end that
  the eye sees as a line. Both now fade with smoothstep, level at each end: the
  linear mask is exactly 100% and 0% on its two lines, and the radial feather
  uses the curve their brush feather already uses. darktable's gradient
  (`erf`) was tried first and dropped, because it carries on past its lines;
  its ellipse (`f * f`) keeps a corner at the inner edge. Feather 0 is still a
  hard edge. Edits that already have these masks render slightly softer.
  `src-tauri/src/mods/mask_falloff.rs`; their `mask_generation.rs` gets one
  import and two calls, its first anchor. Thumbnails are regenerated once
  (pipeline 6). Registry entry `mask-falloff`, with its 10 decisions against
  the 1.6.5 review.

### Fixed

- **The camera profile is always a dropdown.** With no profile installed it
  was the words "Built-in" under the label, and "Find one" a button beside it.
  Now Built-in is the first entry, installed profiles follow, and "Find one…"
  is the last until RawTherapee's profile for the camera is here. A profile it
  finds is selected; when none is published the dropdown stays on what it was,
  says so, and stops offering. The lookup now sends the camera's maker as My
  Gear's does, and a result that arrives after another photo has been opened
  is dropped. `src/argentum/CameraProfile.tsx`; no file of theirs changes.
- **Profile and Rendering sit on one line with their dropdowns**, as a label
  beside its value, rather than a heading above it. `CameraProfile.tsx` and
  `RawToneRendering.tsx`.

## 26.41.8 — 2026-10-09

### Added

- **White balance presets**, Lightroom's menu: As Shot, Auto, Daylight, Cloudy,
  Shade, Tungsten, Fluorescent, Flash, and Custom once a slider has moved. It
  is a Preset row at the top of the White Balance section, above Temperature,
  where Lightroom has its WB row; each entry shows its kelvin. It was first in
  the header beside K and the picker, which pushed "White Balance" onto two
  lines on a narrow panel even for Flash, the shortest name.

  The presets are Adobe Camera Raw's numbers (5500/+10, 6500/+10, 7500/+10,
  2850/0, 3800/+21, 5500/0), which carry over unchanged because RapidRAW
  1.6.5's kelvin and tint are the DNG SDK's: Robertson isotherms and a tint
  scale of -3000. A preset is written as an absolute kelvin in either slider
  mode, so Daylight is 5500 K on every photo it is pasted to.

  The label is worked out from the numbers on every render, so undo, paste and
  a slider drag keep it honest. Auto, the one entry the numbers cannot name, is
  remembered in the edit as `whiteBalanceAuto` and survives a reload because
  their loader keeps keys it does not know. JPEGs get the same menu for now.
  `src/argentum/WhiteBalanceMenu.tsx`. Their file gives Argentum one slot in
  that section, the `color-tools` marker in the header; the row is a container
  of ours put first in the section body, found from the marker by
  AdjustmentSubSection's markup, as AiModelsPlacement places its card. If that
  markup changes, the menu falls back into the slot. No file of theirs changes.
  Registry entry `white-balance-presets`, with its 37 decisions against the
  1.6.5 review.

### Changed

- **Auto white balance moved into the menu.** The wand button is gone; Auto is
  the second entry, detects exactly as before, and the button pulses "Auto"
  while it works. A result that arrives after another photo has been opened is
  now dropped rather than applied to the wrong photo.

## 26.41.7 — 2026-10-09

### Added

- **Compact panels**, in Settings > General under Font, off by default.
  Lightroom's layout, asked for by a tester who found the panels huge:
  - Sliders take one line, name, bar and value: about 20px a slider instead of
    56px, so a panel shows nearly three times as many. The bar is a thin line
    and the thumb a short upright marker centred on it, drawn in a box as wide
    as their 16px disc so their markers (as-shot white balance, other curve
    channels) stay on the value they mark. A Lightroom triangle under the line
    was tried first and hung into the row below. Long names end in "..." and
    show in full on hover; the name column is never narrower than 5.5rem.
    Colour grading sliders, half a panel wide under their wheels, keep two
    lines but get the thin bar and marker too.
  - **Row spacing**, a slider under the switch, 0-12px under each row (4 to
    start), applied live. Stored as `compactSliderGap`.
  - The tone curve's graph is 62% of the panel, centred, with smaller mode and
    channel buttons above it: about a third of its height. Their drags are
    measured against the graph's on-screen size, so points still follow the
    pointer.

  All of it is `src/argentum/compactSliders.css`, keyed on an attribute on
  `<html>`; no line of `Slider.tsx` or `Curves.tsx` changes, and drag, fine
  adjust, click-to-type and reset are theirs untouched. Stored as
  `compactSliders` in `argentum-processing.json`. Registry entry
  `compact-sliders`.

### Changed

- **The Subject mask is called Object**, Lightroom's name for a tool you draw
  a box round something with. Lightroom's Subject is one click and automatic,
  which is our Foreground. Only the label changes, in all 15 languages and in
  the Built-in AI card's list: the type is still `ai-subject`, so saved masks
  load as before, and a mask already named "Subject" keeps its name. Replaced
  in i18next at startup (`src/argentum/locales/renames.ts`), so no locale file
  of theirs is edited. Registry entry `object-label`.

## 26.41.6 — 2026-10-07

Merges RapidRAW 1.6.5 (`71a07921..79c2a46b`, 149 commits). The review is the
`79c2a46b` entry of `scripts/upstream-decisions.mjs` (818 overlaps); the
reasoning and the open ends are in `docs/UPSTREAM_CATCHUP.md`.

### Changed

- **White balance is RapidRAW 1.6.5's.** Their engine adapts with Bradford
  gains in log-LMS, reads a Kelvin value from the camera's as-shot data (A/D65
  matrix interpolation), balances per mask, and has an area picker on the
  original image, with a Kelvin/relative switch. It replaces `dt_white_balance`
  (retired from `modules.wgsl`) and our picker (`src/argentum/whiteBalance.ts`,
  `solve_white_balance_at_point`). The scene-linear anchor keeps the camera
  profile and runs just before their white balance.
- **Auto white balance stays ours** and answers in their units:
  `auto_wb::removing_illuminant` hands the detected illuminant to their
  `pick_white_balance`, and the button writes the result the way their picker
  does.
- **Old white balance edits are converted** (`mods/wb_legacy.rs`,
  `WhiteBalanceUpgrade.tsx`). An edit with no `whiteBalance` key predates the
  merge; it is converted exactly, relative to a tenth of a step where it fits
  their sliders and Kelvin where it does not, masks included. Folders are swept
  when the library lists them, the open photo is corrected once their loader
  has it, and presets are converted once at start-up with
  `presets.pre-1.6.5.json` kept.
- **AI Models moved to Settings > General**, under their Generative AI card,
  which 1.6.5 moved there from Processing. `ProcessingTabs.tsx` is gone;
  `AiModelsPlacement.tsx` places the card from the existing slot.
- **AI-Free mode hides Enlarge**, with their AI controls.
- From upstream, taken as they are: guided-filter shadows, highlights, whites,
  clarity and dehaze, and a rewritten Whites; HSL hue in a perceptual space;
  pick and reject flags (stored in `.agdata`); export border and pad to aspect
  ratio; an sRGB ICC profile in JPEG and PNG; Nikon lenses read from the
  MakerNote; embedded-preview RAW thumbnails; reorderable, collapsible
  adjustment sections and Tool Focus mode; curve fine adjust; saved crop
  ratios; Tauri 2.12; Czech and Dutch.
- Czech and Dutch say Argentum and `.agdata` where the other locales do, and
  carry our six strings.
- Thumbnail cache version 5: every rendering changed.

### Removed

- **RapidRAW Cloud.** Since the 1.6.4 merge Argentum had been starting a Clerk
  sign-in with RapidRAW's development key on every launch; 1.6.5 replaced it
  with their production key and a paid Cloud option. `noCloud.ts` marks their
  cloud store unsupported before their launch effect runs, `NoCloudTile.tsx`
  hides the tile, and the Clerk plugin, its session store, the desktop-cloud
  capability and the getrapidraw.com HTTP allow-list are out of the build.

### Fixed

- **Highlights above white survive a white balance change.** The old engine
  decoded the RAW a second time and clipped everything above white whenever
  Temperature or Tint was off zero; its correction was also weaker than its own
  maths (about three quarters in the midtones). Converted edits get what their
  sliders asked for.
- `custom_aspect_ratios` arrived without `#[serde(default)]` beside our own
  field; without it, every settings file written before this release would
  have failed to parse and the app would have started on defaults.

### Known issues

- Canon EOS-1D and 1Ds: the Kelvin readout is off, because RapidRAW's as-shot
  reader never reaches our MakerNote fix (a line in `raw_processing.rs`, which
  has no anchor allowance). The picture and the corrections are right.

## 26.41.5 — 2026-10-07

### Removed

- **The Object mask (the paint-to-select brush from 26.41.4).** On the first
  real test it lost to the Subject mask it was built beside. A stroke along an
  eye came back as the eye, the lids and part of the brow, while a Subject box
  drawn round the other eye came back as the eye. A box tells SAM how big the
  thing is; paint only says roughly where. The brush made up for that with five
  differently shaped prompts and rules tuned on about a dozen painted cases,
  and an eye sits close to the edge of one of those rules. Use Subject: draw a
  box, and add a second Subject set to subtract to leave part out.

  Gone: `mods/object_brush.rs`, `mods/matting.rs`, `ObjectBrush.tsx`,
  `objectMask.ts`, the `object_brush_mask` command, its strings, the ViTMatte
  credit and its row on the AI Models tab. Masks already made with it were
  stored as ordinary Subject masks, so they keep rendering and exporting
  unchanged.

  Its edge model, `vitmatte_small_composition_1k.onnx` (about 100 MB), would
  otherwise sit in the models folder with nothing listing it. It is now in
  `model_catalog::RETIRED`, and `model_manager` deletes it at the next start
  under the same checks as any deletion: a plain file, directly in the models
  folder, not reached through a link.

  The `object-brush` registry entry is retired in the `71a07921` review with a
  `retire:object-brush` decision, rather than deleted. `photoBox.ts` stays: the
  RGB readout uses it, and its dependency on `ImageCanvas.tsx`'s overlay svg
  moves to the `rgb-readout` entry. The code is in git at `c15e53f0`.

### Changed

- **AI models moved to Settings > Processing, as a tab beside Processing
  Engine.** It was a section of About, which is not where anyone looks for
  downloaded models. The slot their settings panel already renders for About
  and My Gear now renders for Processing too, just before the page;
  `src/argentum/ProcessingTabs.tsx` puts the two tabs there and hides their
  page while AI Models is picked. The panel is still
  `src/argentum/AiModels.tsx`.

## 26.41.4 — 2026-10-06

### Added

- **The Object mask: paint roughly over something and the mask snaps to it.**
  Lightroom's Select Object, in brush form. It is a new tile in the masks
  toolbox, after Subject. Paint over a thing and let go; paint again to add to
  it, hold Alt to paint what to leave out, and Start over to clear. Brush size
  is on the bar above the photo and on Ctrl+Up / Ctrl+Down, as for the brush.

  A stroke is put to SAM, the model RapidRAW's Subject mask already uses, five
  ways: points spread along the paint inside a box round the paint; inside a
  box round the brush's *path*; inside a box half as big again; with no box;
  and the box alone. The rules that choose, each from a failure on a real
  photo:

  - **The brush selects what was painted.** The answer inside the paint's box
    is the default. Covering more of the paint is not evidence for a bigger
    answer: rough paint over a see-through lamp is mostly the wall behind it,
    and the first version selected the wall.
  - **Spill is not meant.** Rough paint along an eye also covers lid; given a
    box that holds the spill, SAM returns eye and lid. The answer in the box
    round the brush's path is taken when it is smaller, covers the path, and
    explains nearly as much of the paint (within 10%). The last condition
    keeps a shoe's strap from winning over the shoe.
  - **Cut off means grow, but only so far.** When the answer runs into a third
    of its box's edge — a line drawn across a mug, a stroke down a lawn — the
    box grows by half the paint's size on every side. The unbounded answer is
    used only for a single dab, which is a click: unbounded, a line along an
    eye on a close-up became the whole face.
  - Parts of the answer the paint does not touch are dropped, and Alt-painted
    exclusions put the boxed answer in doubt.

  Then the edge. SAM sees the whole photo at 1024 pixels and draws its mask on
  a 256-pixel grid, so a small thing came back with a halo. Anything under a
  fifth of the photo gets a second SAM pass on a crop around it, where it has
  the 1024 pixels to itself; and every mask's edge is redrawn at full
  resolution by a matting model, ViTMatte, in a band one and a half grid cells
  either side of SAM's edge. Wider was tried and let a hair mask take the
  face. The model, about 100 MB, downloads the first time the Object mask is
  used, is verified by SHA-256, and is credited in About. Without it the brush
  falls back to the Subject mask's edge. Measured on this machine's CPU: about
  1-3 s a stroke for large things and 2-7 s for small ones, which the second
  pass costs.

  It edits none of RapidRAW's files. The tile is added to their toolbox array
  at startup; the component it creates is turned into an ordinary Subject mask
  carrying its strokes, so their renderer, export, Grow and Feather, and
  embedding warm-up all apply unchanged; and the canvas takes a press on the
  mask stage before their box tool sees it. Everything this leans on is listed
  in the `object-brush` registry entry. `mods/object_brush.rs` carries unit
  tests and an ignored test that runs the real model on a photo and writes
  overlays; on two test photos a stroke takes about 0.2 s once the photo is
  encoded.

- **Settings > About > AI models.** Each AI feature downloads its model the
  first time it is used, and nothing showed what had arrived: a few features in,
  the models folder holds well over a gigabyte. The new section lists every
  model the app knows about, what it is for, whether it is on this computer and
  how big it is (or how big the download is), with the total, and a Delete
  button per model. A deleted model downloads again the next time its feature is
  used; there are no download buttons.

  The list is `mods/model_catalog.rs`, and adding a model is one entry there.
  RapidRAW's file names and addresses are private constants in its
  `ai_processing.rs`, so they are copied rather than made public, and its tests
  read that file: they fail if a copy drifts or upstream saves a model the list
  does not know. Deleting goes through `mods/model_manager.rs`, which takes an
  id, never a path, and only removes names the list gives, directly inside the
  models folder and not through a link.

  Something Windows will not delete while it is open, and the graphics card
  runtime once a run has picked it (ONNX Runtime loads it lazily from that path
  and would crash the next AI feature if it vanished), is not reported as
  deleted. It is marked, shown as waiting, and removed by `startup::init` at the
  next start, before anything can load it.
  The edge model the Object mask downloads is on the list too.


## 26.41.3 — 2026-10-06

### Changed

- **AI enlargement runs on the graphics card on Windows.** On the CPU, 2× on a
  32 MP photo took around half an hour: 15.8 s for each 512px tile on an Intel
  Core Ultra 9 288V, 187 tiles. On that machine's Arc 140V the same tile takes
  0.9 s at 2× and 3.7 s instead of 49 s at 4×, with output identical to the
  CPU's.

  It uses Microsoft's DirectML build of ONNX Runtime 1.22, the release RapidRAW
  already ships, which runs on any DirectX 12 GPU. It is not bundled: like the
  models, it downloads the first time enlargement runs, from Microsoft's NuGet
  package (`Microsoft.ML.OnnxRuntime.DirectML` 1.22.0, SHA-256 `29f9872d…`),
  and only the runtime for the machine is kept (16 MB). `mods/gpu_runtime.rs`
  pins it from `startup::init`, before anything loads ONNX Runtime, so it
  becomes the one runtime the process uses; RapidRAW's other AI features ask
  for no GPU and keep running on its CPU path. DirectML.dll is not downloaded:
  the runtime only loads it for a GPU session, and Windows 11 has a recent
  enough copy. Without one, or on macOS and Linux, enlargement runs on the CPU
  as before. If the GPU fails partway through a photo, out of video memory
  on a large 4× tile or a driver reset, that photo is redone on the CPU. The
  progress line says which device is working.

- **AI enlargement does about 20% less work.** Tiles overlapped by 3/16 of
  their width; they now overlap by 3/32, 48px on a 512px tile. Against a single
  whole-image pass, the mean error inside the blended seams stays under one
  level in 255. A 32 MP photo goes from 187 tiles to 150.

- **AI enlargement works on the framed photo.** It enlarged the whole
  uncropped image, so with a tight crop most of the work went on pixels that
  were thrown away. It now applies the crop together with the straightening,
  rotation, flips, perspective and lens correction it is measured in, through
  the export's own `apply_all_transformations`, and enlarges only that. The
  enlarged photo keeps the look, exposure, colour, curves and the rest, as
  editable settings, and none of the framing, which is already in its pixels:
  every key that call reads is left out of its sidecar, now including the lens
  profile and corrections. A virtual copy is enlarged with its own crop and
  edits; the sidecar used to come from the original.

### Fixed

- **The Super Resolution window shows its before/after comparison.** The
  comparison box sized itself as a percentage of a parent that had only a
  minimum height, and with both photos positioned absolutely it collapsed to
  nothing: the labels and the slider appeared over an empty area, while saving
  worked. The box now fills a 60vh preview area.

### Decided

- **Real-ESRGAN stays.** Four newer 2× models were measured on R6 Mark III
  photos against `RealESRGAN_x2plus`: ParagonSR2 Photo and Stream (Phhofm,
  2026-01), 2xParagonSR Nano GAN (2025-11) and 2xPublic RealPLKSR real_nn
  (2025-06). The two fast ones (12× and 16×) look like a plain resize or break
  thin lines into dashes; ParagonSR2 Photo is softer at the same speed; RealPLKSR
  invents hair strands and is slower. On the GPU the speed argument for
  switching goes away.

## 26.41.2 — 2026-10-05

### Fixed

- **The Highlight Recovery switch now changes the photo on screen.** It said
  "applies to the next photo you open" and did not manage even that: recently
  opened photos are served from memory without decoding again, so for any of
  them, including the one on screen, the switch did nothing.

  Flipping it now decodes the open photo again with the new setting and
  re-renders it with the same edits. `mods/redecode.rs` decodes to one side
  while the old version stays on screen, swaps it in only if the photo is
  still open and no newer flip superseded it, and only then drops the caches
  built from the old pixels. That ordering is deliberate. Calling `load_image`
  empties the open image first, and the four failed attempts at a live
  camera-profile switch broke on exactly that window: a black flash, a preview
  worker finding nothing. Masks, AI patches and AI results are kept. The other
  recently opened photos are dropped from memory, so they decode under the new
  setting too.

- **With Highlight Recovery on, a darkened blown window is no longer
  lavender.** Where only green clipped, red and blue are real, and on a window
  behind a portrait they carried the window's cool light. Recovery kept that
  colour in every rebuilt pixel up to the level where the *last* channel clips,
  more than a stop above the first on daylight white balance. Beside the
  neutral blown core, it read as a lavender band once the photo was darkened.
  Rebuilt pixels now keep their recovered brightness and are neutral by half a
  stop above the first clip. Measured on that frame: fades ending at 1.1×,
  1.25× and 1.5× all removed the band without bringing back outlines at the
  clip edge, and half a stop sits between the two longer ones. Thumbnails
  regenerate once.

## 26.41.1 — 2026-10-05

### Fixed

- **Blown highlights are white again, not pink.** A window behind a portrait,
  or any area where the sensor clipped in every channel, came out flat magenta
  in the editor, the thumbnails and exports. It started with the RapidRAW
  1.6.4 catch-up on 2026-09-23.

  Every photosite in such an area sits at its ceiling, and white balance then
  multiplies red and blue by two or more. A compression pass in
  `raw_processing.rs` used to desaturate everything above white and hide that.
  RapidRAW removed it (`85bf424a`) and covered the gap with a post-demosaic
  colour correction (`40cfa3df`). The catch-up took the removal and declined
  the correction, because it would stack on Argentum's own recovery, and
  nothing replaced it.

  `highlights::settle_blown` now does that job before demosaic, from the
  photosites that are actually at their ceiling rather than from a colour
  guess. With Highlight Recovery on, fully blown blocks are set neutral at the
  brightest level recorded, so the core is never darker than its reconstructed
  edge. With it off, clipped areas are clipped to white, as any RAW developer
  does. Saturated colours that did not clip are untouched, and a photo with
  nothing clipped decodes exactly as before. Cached thumbnails regenerate once.

- **Highlight recovery no longer draws outlines round blown areas.** Two
  causes. Where red and green clipped but blue survived, recovery estimated
  brightness from blue alone. When that came out lower than the clipped red
  already proved, green was rebuilt too low while red stayed at its ceiling,
  so the reconstruction was itself magenta. A clipped channel is now a floor
  on the estimate. And the rebuilt edge, in the photo's measured highlight
  colour, met the white blown core in a hard step: a cyan or pink line round
  anything seen through a blown window. Rebuilt pixels now fade from their
  reconstructed colour just over the clipping point to neutral as they near
  full clipping.

## 26.39.2 — 2026-09-23

*Backfilled on 2026-10-05: the release shipped without a changelog entry. Written
from its published release notes and PRs #24–#27.*

### Added

- **Local AI super-resolution, 2× and 4×.** The model is downloaded and
  verified on demand, and processing adapts to the machine. Supported edits
  carry over; unscaled crop and mask data does not leak into the enlarged
  output. (#25)
- **Selectable RAW tone rendering: Default, Base Curve and Auto-Matched.**
  Chosen per photo in Basic, under the Tone Mapper. Default is RapidRAW's path
  unchanged. Base Curve is a darktable-style curve, currently a generic
  Canon-style fallback. Auto-Matched is RawTherapee-style, derived from the RAW
  and its embedded JPEG. The curve is stored per photo and applied in the
  existing GPU pipeline. (#24)
- **Import dialogue improvements, ported from RapidRAW #1714.** Settings
  persist between sessions. Import can apply automatic edits, presets and lens
  correction, and reads rating, label and keywords from sibling or embedded
  XMP. Argentum's `.agdata` / `.agexif` sidecars are kept; that boundary was the
  merge's only conflict. (#27)
- **RapidRAW 1.6.4 catch-up**, with every overlap decision recorded in
  `docs/UPSTREAM_CATCHUP.md`: the final Brightness implementation, scene-linear
  headroom above white, the neutral-grey canvas, persistent Quick Filter,
  Vibrance and RGB curves. Argentum's highlight recovery and 32-bit-float TIFF
  output are kept, and rawler is pinned to the C50 data commit `16b9b010`. (#26)

### Fixed

- Associated sidecars are cleaned up when a photo is deleted, and folder counts
  refresh afterwards. (#27)
- Enlarged images keep their full-image fit, and mouse-wheel zoom works on
  them. (#25)

### Known issue

- **Fully blown highlights render magenta.** The catch-up removed the
  compression pass that had been making them white. Fixed in 26.41.1.

## 26.39.1 — 2026-09-22

### Added

- **Custom LUTs can now be organized into libraries.** Create named groups,
  import into a chosen group, collapse groups, and move, rename or delete them
  without moving the underlying LUT files.

### Fixed

- **Bright RAW detail is preserved further through editing.** Highlight values
  are no longer cut down too early while noise reduction and sharpening run,
  leaving the final display mapping to handle them.

## 26.38.3 — 2026-09-17

### Fixed

- **Canon EOS C50 `.CR3` files are now recognised.** RapidRAW issue [#1735](https://github.com/CyberTimon/RapidRAW/issues/1735)
  reported that stills from the C50 opened with an unknown-camera error because
  the rawler fork did not yet contain the camera's data. Argentum now pins an
  Ag-owned rawler revision containing only the Canon EOS C50 camera definition.
  No decoder or UI changes were needed.
- **Canon 1D and 1Ds raw `.TIF` files are routed to the raw decoder.** These
  cameras wrote real Bayer raws in TIFF containers before `.CR2` existed, so
  treating every `.TIF` as an ordinary image opened the small embedded preview
  instead of the photograph. Argentum now asks the decoder whether an ambiguous
  TIFF is actually a raw file, while leaving ordinary TIFFs on the normal image
  path.

## 26.38.2 — 2026-09-15

### Fixed

- **Canon EOS-1D and 1Ds files are no longer green.** Reported against upstream
  as [RapidRAW #1678](https://github.com/CyberTimon/RapidRAW/issues/1678), with
  a sample frame from the reporter; it reproduced here exactly.

  Those bodies predate Canon's ColorData block, so they keep their white balance
  in MakerNote `0x00a4`. rawler knows the tag — `Cr2Decoder::get_wb` falls back
  to it by name as `Cr2OldWB` — but looks for it with `self.tiff.get_entry`,
  which walks the root IFD chain, and `0x00a4` is a *MakerNote* tag. Every other
  makernote read in that decoder goes through `self.makernote`; this one does
  not. So it is never found, `get_wb` returns `[NaN; 4]`, and rawler's
  `develop_intermediate` substitutes `[1, 1, 1, 1]`. A Bayer frame developed
  with no white balance at all is green — that was the whole of it. It also
  explains why the reporter could not dial it out: the slider is a relative
  adjustment on top of a frame that never received the camera's multipliers.

  `mods/canon_old_wb.rs` reads the table itself and green-normalises it — the
  stored triple is scaled to green = 512, and rawler's `0x00a4` branch does not
  normalise, so passing it through unchanged would be a uniform 512x gain. On
  the reporter's `91BX8040.TIF`: `[842, 512, 633]` → `[1.645, 1.0, 1.236]`.
  Gated on rawler having come back with NaN, so it names no camera and cannot
  override a body that already has coefficients.

- **The Canon MakerNote reader now honours the file's byte order.** It came out
  of `sraw_levels` and read little-endian, which CR2 always is. The 1Ds writes
  `MM` — a big-endian TIFF older than the CR2 container — and a big-endian IFD
  read the wrong way round does not fail: the entry count reads as nonsense, the
  walk finds nothing, and the answer is a polite "no white balance here", which
  is precisely the symptom being fixed. Now in `mods/canon_makernote.rs`, shared
  by both callers, with the order taken from the file's own magic.
- **A `.TIF` that is really a raw now opens as one.** Canon's original EOS-1D
  and 1Ds predate the CR2 container: they wrote raws as TIFF files with a `.TIF`
  extension, a real Bayer mosaic reached through a MakerNote offset, with two
  small RGB previews sitting in front of it in the IFD chain.

  `formats.rs` decides raw-ness by extension and `tif` is in
  `NON_RAW_EXTENSIONS`, so those files never reached the raw decoder. They went
  to the ordinary image loader, which read the first image in the chain — on the
  file from [RapidRAW #1678](https://github.com/CyberTimon/RapidRAW/issues/1678)
  that is a 288x192 thumbnail — and the app presented it as an 11 megapixel
  photograph. No error, no warning: just the wrong picture, small.

  The extension cannot settle this and has not been able to since 2002, so
  `mods/tif_raw.rs` asks the contents instead. Not "is the Make Canon" — an
  ordinary TIFF exported from a Canon photo inherits that, and can inherit a
  whole MakerNote with it — but whether a raw decoder will actually open the
  file. `rawler::get_decoder` parses the container and checks the camera against
  its own table without decoding a pixel, so the test and the consequence are
  the same code path and cannot drift apart. Verified both ways: it accepts the
  1Ds file and rejects an ordinary TIFF. Only `.tif` and `.tiff` are ever
  sniffed, the answer is cached against size and modification time, and every
  other extension still answers without touching the disk.

  This also covers the Kodak, Leaf and Phase One backs that named their raws the
  same way; each costs no further line of upstream's.


### Internal

- **Made the three roadmaps agree with what has shipped.** Reviewed against the
  three releases that exist (`26.37.19`, `26.37.20`, `26.38.1`) rather than
  against the plan:

  - The Releases page in the app had stopped at `26.37.20` while the application
    reported `26.38.1` - the one drift a user can see. It now carries the TIFF
    metadata, the bit-depth choice and upstream's Ctrl crop gestures. `26.37.21`
    correctly gets no entry: a bumped number between releases is a placeholder,
    and every number on that page is a real release.
  - The 8/16-bit selector shipped in `26.38.1` with no changelog entry at all,
    and `docs/ROADMAP.md` still had it as not started. Both fixed; the entry is
    under `26.38.1`, where the work actually landed.
  - **Upstream has built half of a roadmap item and nothing said so.**
    `useSortedLibrary.ts` groups RAW/JPEG pairs through `utils/imageGrouping.ts`,
    so "the grid is one flat list and nothing groups" was wrong; and sorting by
    capture time, which the in-app roadmap still listed as a reason to want
    grouping, arrived in upstream's `0e8029bd`. Grouping by date, camera or lens
    is still not done - pair grouping is a different thing - but the argument for
    it no longer rests on a fault that has been fixed. This is the failure mode
    `featureReview` exists for, found by reading the tree rather than the diff.
  - The #1466 triage row said its encoder "was not needed". True when written,
    and no longer: its encoder arm is what the 8-bit mode borrows.
  - The memory row now names the cost `26.38.1` added - a TIFF metadata write
    re-serialises the whole encoded file, 3-4x its size, about 2.5GB on a 60MP
    16-bit export.

## 26.38.1 — 2026-09-14

### Added

- **Choose 8 or 16 bits for a TIFF.** Every TIFF Argentum wrote was 16-bit
  whether that was wanted or not, and a file going to a client is a delivery
  rather than a master. The control appears under Export for TIFF and no other
  format.

  The encoder arm is upstream [#1466](https://github.com/CyberTimon/RapidRAW/pull/1466)'s,
  marked between `// upstream #1466` and `// end upstream #1466` in
  `export_processing.rs`: an 8-bit image writes `Rgb8`, everything else `Rgb16`.
  What is *not* theirs is where the depth lives. Theirs puts `tiffBitDepth` on
  `ExportSettings`, into their presets, and threads a fourth parameter on
  `encode_image_to_bytes` through six of their files and three strings in
  thirteen locales - the counter-example CLAUDE.md names. Here it is Argentum's
  own preference, stored beside the profile library as highlight recovery is,
  and the encoder reads the depth from the image it was handed: an 8-bit export
  renders `ImageRgba8`, a 16-bit one `ImageRgba32F`. `src/argentum/ExportPrecision.tsx`,
  one marker in their panel. See the `export-precision-selector` registry entry.

  **Two bugs this would have shipped, both found in review.** Dispatching on the
  image type means a size estimate has to render at the precision it will encode
  at, or a 16-bit TIFF is quoted at the 8-bit size; both estimate sites now go
  through `render_for_export`. And `argentum-processing.json` was written whole
  on every save, which is correct for one setting and silently destroys the
  rest - saving an export depth would have switched highlight recovery back on,
  and the bug report would have been about highlight recovery. Reads and writes
  go through `mods/ag_settings.rs` now, which changes one key and leaves the
  others alone; highlights moved onto it.

- **An exported TIFF keeps its EXIF.** Camera, lens, exposure, date, copyright
  and — unless you ask for them to go — GPS. The Keep metadata switch is now
  shown for TIFF and means it; before, it was ticked by default, hidden for
  TIFF, and silently did nothing on exactly the format chosen for handing work
  to another editor.

  Upstream returns early for TIFF behind a `FIXME: temporary solution until I
  find a way to write metadata to TIFF`. The reason is structural:
  `little_exif` 0.6.23 treats a TIFF as one big EXIF structure, so writing to
  one replaces the whole file with an encoding of the metadata, strip data and
  all — and the fresh `Metadata::new()` that is right for JPEG holds no strips
  and none of the ten structural tags a TIFF must carry. So
  `mods/export_metadata.rs` seeds from the TIFF we just encoded instead, and
  merges upstream's tags into that. Their tag gathering is reused rather than
  reimplemented, by handing their own writer a one-pixel JPEG and reading the
  tags back out of it. `exif_processing.rs` is not touched.

  Also fixes a gap the bit-depth work opened: their encoder accepts `.tif` as
  well as `.tiff`, and the metadata writer matched only the longer spelling.

  Exported TIFFs are stamped `Software = Argentum`. Other formats still say
  `RapidRAW`, which is a rebrand question rather than part of this.

- **Use less memory** to the roadmap. The pipeline keeps gaining
  full-resolution copies of the photo and has never been measured; the 16-bit
  export target is four bytes a channel where the preview is two, and going f32
  on the input doubles that too. Measure a 45MP file first — deciding what to
  drop before knowing what it costs is guessing.

- Hold Ctrl while dragging on the photo to move its crop, or hold Ctrl and use
  the wheel to resize the crop. Ctrl-double-click resets crop and rotation.
  These gestures also work outside crop mode. Upstream `97cc7d5b`.

### Changed

- **16-bit TIFF export is in progress, not done.** It shipped in `26.37.20` and
  both roadmaps called it finished. The container is 16 bits; the content is
  about 11, because the photo is still uploaded as `Rgba16Float`. A row that
  claims a depth the pipeline cannot fill is the kind of promise this roadmap
  exists to avoid making.

### Internal

- Prevent the mask overlay from drawing a zero-sized canvas when returning to
  the library. The user confirmed the back arrow is stable after this fix.

- Merged RapidRAW through `5ad3ba0b` and recorded the dependency review. The
  later highlight recovery commit `40cfa3df` remains pending review. See
  [the catch-up rationale and next steps](docs/UPSTREAM_CATCHUP.md).
- Preview cropping reuses the patched/warped image, with upstream's follow-up
  correction preserving lens blur and transformation order.
- Upstream reviews now consider the interface and workflow separately from
  processing, including when Argentum keeps its own implementation.

## Versioning: `yy.isoWeek.release`

```
26.37.1
 │  │  └── release within that week, counting from 1
 │  └───── ISO week number
 └──────── ISO year, two digits
```

No arguments about what counts as major or minor. The version says when it was
built, which is the thing you actually want to know a year later.

Current week:

```bash
date +%g.%V
```

**Bump immediately after releasing, never before.** The moment `26.37.19` was
published, the working version became `26.37.20`. So a number that exists on the
Releases page is a release, and a number one ahead of it is a build from source
- which answers "which version am I actually running?" without a suffix, a
build-metadata tag, or anything the Windows installer might refuse.

It also makes the in-app update check behave by accident: a development build is
*newer* than the latest release, so it stays quiet instead of offering to
"update" you to something older than what you are running.

**The bumped number is a placeholder, not a prediction.** It says "unreleased,
and ahead of what shipped" - nothing more. It cannot say which week the next
release will land in, because nobody knows: bump to `26.37.21` on a Saturday and
release the following Tuesday and the real number is `26.38.1`. So **the release
commit sets the true `yy.ww.n`**, and the number sitting in the tree between
releases is only ever a marker.

Nothing breaks when the week rolls over, because `compareVersions` in
`MainLibrary.tsx` compares the parts numerically, left to right: `26.38.1` is
newer than `26.37.21`, and `27.1.1` is newer than `26.52.9`. Both properties the
convention exists for survive it - a development build stays newer than the last
release, so the update check stays quiet, and every number on the Releases page
is still a real release.

**Two digits, not four, and that is not a style choice.** It was `yyyy` until
`26.37.19`, whose Windows build failed with

```
failed to bundle project: `app version major number cannot be greater than 255`
```

Windows installers cap the major version at 255, so `2026.x.y` can never be
packaged for Windows - the platform Argentum is actually developed on. macOS and
Linux have no such limit and built all twenty artefacts happily, which is why it
took a real release to find out.

Every historical version here was renumbered with it. That is safe because none
of them were ever published, and it is *necessary* because the in-app update
check compares version numbers: leaving the old entries at `2026.x` would make
every future release look older than the past, and the check would never fire
again.

**This file is the engineering record. The app does not show it.** What a user
sees is written separately, in plain language, in `src/argentum/releases.ts` —
one line per release answering "what changed for me?", with no file names.
Rendering this file in the app was tried and abandoned: filtering an engineering
log into user-facing notes does not work, because the split is not structural.

So a release touches two files. Everything here, and — only if a person would
notice the difference — a line there. Likewise `src/argentum/knownIssues.ts`
holds what is broken *now*, and `roadmap.ts` what is planned; a `### Known
issues` list in a release entry goes stale the moment it is fixed.

`### Internal` marks build tooling, dev environment and repository housekeeping.
Worth keeping, never user-facing.

**Every harvested module gets an entry naming its source file and upstream
commit hash.** That line is the only thing connecting our code back to where it
came from — without it there's no way to tell later whether upstream moved on.

---

## 26.37.20 — 2026-09-13

### Internal
- **`pr-ci.yml` is manual now, like `ci.yml`.** It still carried RapidRAW's
  `on: pull_request` with no path filter, so opening the first pull request this
  repository has ever had compiled the application on twelve desktop targets to
  land a twenty-seven line roadmap note. `ci.yml` was made manual in 26.37.19 for
  exactly this reason and `pr-ci.yml` was missed, because until branch protection
  arrived nothing here went through a pull request at all. `lint.yml` still runs
  on every push and pull request, and `upstream.yml` — the required check — is the
  one that decides whether a change is safe to merge.

### Fixed
- **A 16-bit TIFF export contained 8-bit data.** `encode_image_to_bytes` wrote
  `ImageRgb16(image.to_rgb16())` over an RGBA8 buffer, so every value in the file
  was an 8-bit number multiplied by 257. The header said 16-bit, Photoshop said
  16-bit, and nothing anywhere said otherwise - which is worse than exporting
  8-bit honestly.

  The encoder was never the broken part and has not been touched: it was being
  handed 8-bit data. What changed is the render. Exporting to `.tiff` now
  builds a second compute pipeline whose storage texture is `rgba32float` instead
  of `rgba8unorm`, and quantises once at the encode - clamp, scale, round, no
  dither - the way darktable goes from its f32 pixelpipe to a format plugin. What
  that removes is the 8-bit *output* bottleneck and only that: the upload and the
  intermediates are still f16, so "quantises once" describes the path from the
  compute pass to the file rather than the whole pipeline.

  Two things the first draft missed, both found in review, both fixed here. The
  per-mask export wrote its `_mask_N_image.tiff` companions through the old 8-bit
  path, so a TIFF batch with masks on produced one honest file and N dishonest
  ones. And the watermark composited through `Rgba<u8>`, quantising every pixel in
  its bounding box - transparent ones included, so a watermark at zero opacity
  demoted a rectangle of the photograph and put nothing there in exchange. An
  export render also now takes a lock of its own: it stopped holding the shared
  processor mutex, and the batch scheduler sizes its workers from system RAM, not
  from VRAM.

  The approach is **dimafa's**, from upstream pull request
  [#1466](https://github.com/CyberTimon/RapidRAW/pull/1466) @
  `0e8cd15977001cee9f86d5efb6adccc105db4cb1`: build the export pipeline by
  rewriting the storage format in the shader source, and gate the 8-bit dither
  behind a pipeline constant. That gate is marked in
  `src-tauri/src/shaders/shader.wgsl` between `// upstream #1466` and
  `// end upstream #1466`. The *idea* is theirs, not the text - their override is a
  `u32` tested with `== 0u`, this is a `bool` and a negation - so the markers
  record where the thinking came from rather than promising a clean merge. Their target is `rgba16float`; ours is `rgba32float`, because
  half-float carries 11 significant bits and so lands the brightest stop of an
  encoded signal on a grid of roughly 32 steps out of 65535 - a large improvement
  over 8-bit, and still not what the container promises. Nothing of
  [#1395](https://github.com/CyberTimon/RapidRAW/pull/1395) is used: its bounded
  intermediate textures already exist here as `clamped_tile_size`, and its
  capability gate belongs to the half-float design this did not follow - not
  re-read against a 32-bit target, so it is the first thing to look at if the f32
  input on the roadmap is ever attempted.

  Everything Argentum added is in `src-tauri/src/mods/export_precision.rs`, and
  the three files of upstream's that take part get one line each that *mentions*
  us - an import of `Precision` in the two Rust files, from which the storage
  format, the shader text, the dither constant and the bytes per pixel of the
  readback all follow, and in `shader.wgsl` the marked block rather than an
  import, since WGSL has none. That is the anchor count, and it is not the
  maintenance cost: their constructor gained a
  parameter, four texture and pipeline descriptors read their format from that
  value, the readback strides multiply by it, and four export call sites pass it.
  The registry entry lists all of it, because "one import" is the cheap half of
  the story.

  **What it does not do, stated plainly:** the input is still uploaded as
  `Rgba16Float`, exactly as upstream does it, so this is not f32 end to end. The
  flare pass samples that texture through a filtering sampler, and a filtered
  `Rgba32Float` needs the `FLOAT32_FILTERABLE` device feature - which would mean
  either the same export producing different precision on different machines, or
  changing a texture every preview also uses. That is a separate change and is on
  the roadmap, as is EXIF in a TIFF - which this does not add, because
  `write_image_with_metadata` returns early for TIFF behind an upstream FIXME that
  wants a round-trip test of its own before it comes out.

  An export builds its own processor and drops it, so a preview never pays for a
  32-bit target, and the two full-resolution scratch textures that only the
  display path writes to are allocated one pixel each instead of 407 MB each.
  That last part was wrong when first written - a comment claimed the whole cost
  was "about 50 MB" while an export was still asking for 814 MB of textures
  nothing touches. A review caught it, along with a `u32` overflow in the
  readback sizing that a 16384x16384 image - exactly what the dimension guard
  permits - would have turned into a panic.

  Tested on a GPU, and without one. Without: naga compiles and validates the rewritten shader, the
  storage-format rewrite fails loudly rather than silently if upstream respells
  the declaration, and the quantisation is checked against two negative controls
  - 8-bit widened to 16 (every value a multiple of 257) and half-float rounding -
  either of which would pass a test that merely asserted "16-bit". With: a real
  device renders a ramp finer than 8 bits can carry and the result is checked for
  being off the 8-bit lattice, for not reversing (a wrong stride is a sawtooth)
  and for every row agreeing (a wrong *row* stride is invisible in one row).
  Those are `#[ignore]`d, because CI has no adapter and a test that fails for
  want of a GPU teaches nobody anything: `cargo test --lib -- --ignored`.

  **What none of those tests asked, and should have:** how many bits actually
  arrive. Every one of them asks "better than 8-bit?" or "f32 rather than f16?",
  which is the right question for catching a fake and the wrong one for measuring
  a real thing - and a 16-bit headline shipped over an 11-bit pipeline with a
  green board behind it. There is now a test that renders a full-range ramp at
  four sampling densities and asserts the result is *equal* to a CPU half-float
  round-trip of the same ramp: 2048 inputs give 2048 levels, 4096 give 3073, 8192
  give 4097, 16384 give 5121. Exact agreement at every density, which says two
  things - the precision of an export is exactly the precision of its upload, and
  the rest of the render adds no error of its own.

  It also kills a misreading. Those counts are not a ceiling: half-float holds
  1024 values in *every* octave, so the count climbs by 1024 on each doubling of
  the ramp, and `log2` of one of them is a property of the sampling rather than a
  bit depth. The honest number is the one this file already carried - 11
  significant bits, 1024 levels per stop. The test is expected to fail the day the
  export input becomes f32, which is the point of writing it as an equality.
- **Two AI patches of the same base64 length shared a cache entry**, and the
  first one rendered was served for both. `calculate_transform_hash` hashed
  `.len()` of the patch data rather than the data. Carried early from upstream
  pull request #1307, marked in `src-tauri/src/cache_utils.rs` between
  `// upstream #1307` and `// end upstream #1307`; still unmerged upstream at
  `5ad3ba0b`.
- **sRGB decoding used an exponent of 3.0 where the standard says 2.4**, which
  darkened the midtones of every non-raw image. From upstream pull request
  #1633, marked the same way in `src-tauri/src/raw_processing.rs`.

### Internal
- **The mergeability checker could not see a deleted file, and reviewing what
  upstream sends was not enforced at all.** Both found by a second model, both
  reproduced before being fixed. Deleting a whole 257-line upstream component
  changed the checker's output by nothing: `+++ b/…` does not match `+++
  /dev/null`, so a deleted file's lines were charged to whichever file preceded
  it in the diff, or dropped when that file was one of ours.

  The deletion gate is now a **removal** gate with no allowance. A `-` line
  answered by a `+` is a replacement and does not count, which is why the
  `.rrdata` → `.agdata` rename went from 31 deletions to zero and gave up its
  exception. Everything genuinely removed is written down: their eyedropper
  white-balance maths, their clipping block in `shader.wgsl`, the Ko-fi link,
  the Android matrix entry in three workflows.

  Overlap review replaces the stale-sha line that was meant to force it. The
  commit we have reviewed through, and a verdict for every upstream change that
  lands on code of ours, live together in `scripts/upstream-decisions.mjs`;
  `check:merge` re-derives the overlaps from git and fails until each has a
  decision and a reason. `CHANGELOG.md`'s base sha is checked against that
  register instead of being written by hand.

  The review now starts from the register rather than the merge-base, so merging
  no longer erases the window, and it watches the *files* our borrowed fixes sit
  in rather than grepping commit subjects for `#1307` — upstream `8737fc4e`
  rewrote the hashing module one of them is inside and named no pull request at
  all. Its own detection was the first thing to catch it.

  `scripts/upstream-diff.mjs` and `scripts/upstream-overlaps.mjs` split the
  logic out of the checker so it can be tested; `npm run test:checks` drives it
  against throwaway repositories, covering whole-file deletion, upstream editing
  borrowed code with no PR number, a landed and a pending borrow together, and
  reviewing after the merge. `.github/workflows/upstream.yml` runs all of it and
  fails — rather than skipping quietly — when the upstream remote or the full
  history is missing.

- **Then the line counts stopped being evidence, and an inventory took over as
  the gate.** The removal count had become the thing standing between us and a
  bad merge, and it cannot bear that weight: `get_all_adjustments_from_json`
  gained a fifth parameter and five call sites across four of their files were
  rewritten one line for one. Nothing added, nothing removed, and every export
  path now depends on a signature upstream owns. Equal counts prove the diff is
  tidy and nothing else, so all three counts are warnings now.

  `scripts/upstream-registry.mjs` replaces them: seventeen entries covering every
  Argentum feature, borrowed fix and deliberate behaviour change, each naming the
  upstream files, symbols and keys it depends on and what proves it still works —
  including where the answer is nothing, which is recorded rather than tidied
  away. Touching a file of theirs that no entry claims now fails the build, and
  overlaps are derived from that inventory rather than from two hand-kept lists.

  Two records were wrong and are corrected: the `exif_processing.rs` exception
  described a call into a `mods::sidecar` that has never existed and claimed
  legacy `.rrdata` files are still read, which they are not — the orphaning is
  deliberate and is registered as such; and the `file_management.rs` note read
  "31 replaced, nothing missing" about a rename that deliberately orphans every
  sidecar RapidRAW has ever written.

  A review requirement can no longer be erased by removing what created it.
  Deleting a `// upstream #NNNN` marker used to delete the reason anyone had to
  think about it; the tree and the registry must now agree, entries are retired
  rather than deleted, and an entry retired *during* the window under review
  still owes that window its decision.

  Last, the part no detector does: a file match finds upstream changing something
  we registered, never upstream building the same feature somewhere we have never
  touched. Every review entry now carries a `featureReview` verdict and reasoning
  — a person's claim that the batch was read for duplicates. Nothing verifies the
  sentence. It is required so the claim is made rather than assumed, which is the
  same mistake as reading a clean merge as proof of safety.

  `npm run test:checks` covers all three gaps plus post-merge review and a mixed
  batch of landed and pending borrowed fixes.

- **Two ways round the new gates, found in review and both reproduced first.**
  A retirement worked for exactly one window. `activeFor` compared `recordedIn`
  against a single sha, so an entry retired in review 2 went quiet for review 3
  and was live again for review 4 and everything after it — a resurrection, in a
  mechanism whose whole job is not to forget. Retirement is ordered against the
  review history now: live up to and including the review that retires it, dead
  in every window after.

  And deleting a registry entry together with its markers erased the requirement
  outright: nothing in the tree, nothing in the registry, nothing left to
  disagree. The inventory is append-only, and our own git history is what says
  so — every id the file has ever held must still be in it. A retirement must
  also name a review that exists and carry a matching `retire:<id>` decision in
  it, because a retirement is how an entry stops generating work and was
  therefore the obvious thing to fake.

  `scripts/test-upstream-e2e.mjs` builds a fork in a throwaway repository, writes
  a real registry and review register into it, and runs the actual checker as a
  subprocess: seven scenarios asserting it fails for each bypass and passes for
  the cases that are legitimate, including a retirement tracked across four
  review windows. Both regressions were confirmed by putting the old code back
  and watching the matching test fail. 27 tests in total.

---

## 26.37.19 — 2026-09-12

### Internal
- **The version scheme lost two digits, because Windows cannot package the
  other two.** The first real release built twenty artefacts for macOS and Linux
  and failed on both Windows jobs, four retries each, with:

  ```
  failed to bundle project: `app version major number cannot be greater than 255`
  ```

  `26.37.19` was `2026.37.19`, and 2026 is the major number. Windows installers
  cap it at 255. So `yyyy.isoWeek.release` could never have produced a Windows
  build - on the platform Argentum is developed and used on - and nothing short
  of cutting a real release would have said so, because every other platform
  packages it without complaint.

  The scheme is now `yy.isoWeek.release`. It says the same thing, `26` holds
  until the year 2255, and the alternatives were worse: a separate Windows-only
  version means two numbers to keep in step, and abandoning calendar versioning
  throws away a decision made deliberately.

  Every past version was renumbered to match. Safe, because none of them were
  ever published - and necessary, because the in-app update check compares
  version numbers, so a history left at `2026.x` would make every future release
  look older than the past and the check would never fire again.

  The failed release and its tag were deleted rather than left as a half-built
  first impression.

- **Argentum is published.** The repository goes to GitHub as its own project
  rather than a GitHub-level fork of RapidRAW: own name, own issues, no "forked
  from" banner. The debt is stated where it belongs — the README's first
  paragraph, the AGPL-3.0 licence inherited unchanged, and the "Built on" group
  in `src/argentum/credits.ts`, which still names Timon Käch first.

- **Personal data removed from the source and from the history.** Publishing a
  repository publishes every version of every file it has ever held, so editing
  the current copy is not enough:

  | What | Where it was | What replaced it |
  |---|---|---|
  | A work email address | the author field of 87 commits | the GitHub noreply address |
  | An employer's name | a backup path, in four files | the bare service name |
  | An account name | a home directory, in seven files | a neutral placeholder |
  | Working rules | `CLAUDE.md`, tracked | untracked, kept locally |
  | Machine setup | `setup.ps1`, tracked | untracked, kept locally |

  The table describes rather than quotes on purpose. This file is rewritten by
  the same pass, so a literal here would be replaced along with the real ones
  and the row would end up saying that a thing was replaced by itself. It did,
  on the first attempt.

- **The fork point had to be rebuilt, and this is the note for the next person
  who tries this.** Rewriting history renumbers every commit it touches. The
  intent was to touch only Argentum's, leaving RapidRAW's hashes alone so the
  shared ancestry survived — and that is not possible here, for two reasons
  found the hard way.

  This clone is **shallow**, so its history has no single root: the shallow
  boundary makes several, and a rewrite renumbers across all of them. And the
  rewrite also renumbers the *remote-tracking* ref, so `git merge-base HEAD
  upstream/main` answers confidently with a commit that exists only locally.
  The first run looked like it had worked. It had severed the fork: no common
  ancestor with the real RapidRAW, and every future upstream merge a whole-tree
  conflict.

  What made this survivable was a full mirror clone taken before the first
  rewrite, and checking `git merge-base` against the real upstream tip rather
  than the local ref. The repair is two steps: fetch the full history so the
  shallow boundary stops inventing roots, then record `97fada3` — the upstream
  commit Argentum had actually merged — as a second parent with `git merge -s
  ours`, which keeps our tree byte for byte. That is truthful rather than a
  trick: everything up to `97fada3` is already in this tree. The eleven upstream
  commits made since are still unmerged and still arrive on the next
  `git merge upstream/main`.

- **Test fixtures read their paths from the environment.** The colour-comparison
  and MakerNote tests pointed at one photo library on one disk. They now take
  `AG_RAW_ROOT`, `AG_COMPARE`, `AG_CANON_SAMPLE` and `AG_CANON_DIR`, so the
  ignored tests still run for whoever has the files and the repository carries
  nobody's folder layout. `mod fixtures` in `colour_compare.rs` documents them.

- **The splash points at Argentum.** It asked RapidRAW's releases what the
  newest version was and compared it against ours — different projects,
  different version schemes, so it could announce an update that does not exist
  for this app and then offer an installer that replaces it with a different
  one. Three URLs in their `MainLibrary.tsx` now name Argentum: the releases API
  the check reads, the download it opens, and the contribute link. Written
  inline rather than imported from `src/argentum/`, because that file has no
  anchor and a link is not a reason to give it one.

- **`package.json` names Argentum's author.** It said `Timon Käch`, inherited
  from RapidRAW, which meant every build blamed him for our bugs. The credit for
  his work is not diminished by this; an authorship field is about who to report
  a crash to.

  `Cargo.toml` keeps his name, and that is the mergeability rule working rather
  than an oversight: the same edit there took the file to 9 added lines against
  a budget of 8, and the budget is not raised. One line of vanity is not worth
  the room a real change will need.

- **README rewritten for a new reader.** The machine-layout and
  backup sections described one disk and meant nothing to anyone else.
  In their place a `Known limitations` section says plainly that the screen
  conversion is Windows-only and that the macOS and Linux builds are untested —
  the same wording as `knownIssues.ts`, so the app and the repository do not
  disagree.

- **The repository has a front page, and a credits file.** The README was written
  for one reader who already knew everything. It now says what Argentum is, which
  platforms it is tested on, where to get a build, what it adds over RapidRAW, and
  where the app keeps its files.

  Two things in it were simply wrong and are corrected: it described the app data
  as living in one configurable folder chosen in Settings, which is the roadmap's
  portable install and not what the code does — the real location is the operating
  system's application-data directory under `co.argentum.editor`, and there is no
  setting for it. And it presented the Windows build toolchain as the way to run
  Argentum, when an installer needs none of it.

  `CREDITS.md` is new. The app carries two lists — Argentum's own in
  `src/argentum/credits.ts` and RapidRAW's inherited Special Thanks — and neither
  is visible to someone reading the repository. It merges both, states what was
  taken from each project rather than only naming it, and records two decisions:
  darktable's sigmoid curve is credited even though nothing calls it, because the
  code ships either way; and RapidRAW's closing thank-you to his users is left out,
  because it is a message rather than an attribution.

- **The roadmap said three shipped things were unfinished.** Highlight recovery
  was marked in progress, the clipping preview not started, and capture-time
  sorting outstanding - all three had shipped, the last of them in
  `useSortedLibrary.ts` before any of this. A roadmap that disagrees with the
  changelog makes a reader doubt both, so the statuses are corrected and the
  entry for capture-time sorting now describes what the code does.

  Also removed from it: a pointer to two decision records in a private notebook,
  which a reader of the public repository cannot open.

- **CI was red on the day the repository went public, and every warning was
  ours.** `cargo clippy -D warnings` fails the build on any lint, and thirty
  fired - all thirty inside `src/mods/`, none in a file of upstream's. That is
  the architecture working: the code being judged is the code we wrote.

  Most were mechanical and `clippy --fix` took them: floats carrying more digits
  than an `f32` holds, `chunks_exact` with a constant size where `as_chunks`
  says it better, `repeat().take()` where `repeat_n` exists, a `min`/`max` pair
  that is `clamp`, a `trim()` before `split_whitespace` which already trims.

  Three needed a decision, and one of those decisions was wrong first time:

  | | |
  |---|---|
  | `conversion_for_window` | Deleted. Superseded by `shader_rows_for_main_window`, and only a test still called it - a test whose real assertion, that a nonsense window handle does not crash the renderer, survives without it |
  | `forget` | **Kept**, gated to tests. It was called by six of them; "never used" was the lint reading the library build, not the truth. Deleting it broke the test build immediately |
  | `IDENTITY` | Kept, gated to tests. `SHADER_IDENTITY` is what the shader takes; this is now the fixture the test uses to check the two agree |

  Formatting is now checked on `src/mods/` rather than the whole crate, with the
  reason written into `lint.yml`: upstream's files fail `rustfmt`, and fixing
  that would spend budget meant for real changes on whitespace.

- **Building every platform on every push is now manual.** `ci.yml` ran the full
  matrix - twelve builds producing installers - on every push to `main`. Pushes
  are many and releases are rare, so a documentation fix or a chore did the same
  work as a release, and five pushes in an afternoon queued sixty-five jobs that
  stacked faster than they cleared.

  It is `workflow_dispatch` now: runnable by hand from the Actions tab when a
  build needs proving between releases, and otherwise silent. Nothing is lost -
  `release.yml` still fans out across every platform when a release is created,
  which is where installers are actually wanted, and `lint.yml` still runs on
  every push, which is where fast feedback belongs.

  The backlog was cancelled rather than left to drain.

- **The Android build is dropped, and the earlier explanation for it was wrong.**
  The first diagnosis blamed a `.gitignore` line for untracking upstream's
  Android project. That was real and worth fixing on its own, but it was not why
  the job failed, and the job failed again after it. The actual message:

  ```
  Project directory .../gen/android/app/src/main/java/co/argentum/editor
  does not exist. Did you update the bundle identifier in tauri.conf.json?
  ```

  The Android project is laid out for *RapidRAW's* identity all the way through -
  `io.github.CyberTimon.RapidRAW` as the package path, the gradle `namespace` and
  the `applicationId`. Argentum changed its identifier to `co.argentum.editor`,
  and Tauri looks for a Java package matching it.

  Making it build means rewriting those three things inside upstream's files and
  keeping them rewritten, so every Android change he makes conflicts with ours
  forever - for a platform Argentum does not ship, cannot test, and whose pull
  requests it has already declined. That is the divergence the whole architecture
  exists to prevent, spent on the one platform with nothing to gain.

  So the job is removed from all three workflows. The Android project itself
  stays tracked, byte-for-byte his, so a future upstream merge still receives his
  changes to it and nothing has to be un-deleted first.

- **One line of `.gitignore` had silently untracked upstream's Android project.**
  The Android CI job failed with `could not find Cargo.toml`, which reads like a
  broken workflow and was not: 42 files that RapidRAW tracks under
  `src-tauri/gen/android` were not in this repository at all.

  Argentum had added `src-tauri/gen/` to ignore the generated Tauri schemas.
  Upstream already handled that further down, with `src-tauri/gen/*` followed by
  `!src-tauri/gen/android/` - and **git cannot re-include a file whose parent
  directory is excluded**, so our line killed their negation. Nothing warned:
  the files simply stopped being tracked, and the only symptom was a CI job
  failing on a platform nobody was looking at.

  Our line is gone and upstream's pair does the job, with a comment above it
  saying why the obvious-looking rule must not come back. The Android project is
  restored from `upstream/main`, so those 42 files are byte-for-byte his.

  Argentum does not target Android and rejected the Android-specific pull
  requests. That is a reason not to *work* on it, not a reason to break a build
  upstream maintains for free.

- **Removed a stray file named `60%`**, a zero-byte artefact of a shell redirect
  that had been committed by accident.

---

## 26.37.18 — 2026-09-12

### Measured
- **Three apps, one file, three different pictures — and now three answers.**

  An export looked faded in Windows Photos, richer in GIMP, and different
  again in Argentum. Embedding an sRGB profile in a copy changed nothing in
  either viewer, so the missing tag was not the cause.

  Measured with a patch chart rather than a photograph: eight flat colours of
  known sRGB value, screenshotted out of each app. A screenshot holds what was
  handed to the panel, so a patch that comes back unchanged means the app
  converted nothing, and one that comes back different says exactly what it did.

  ```
                     file     Windows Photos   GIMP
    red         255,  0,  0     219, 54, 40    255,  0,  0
    green         0,255,  0     132,249, 80      0,255,  0
    blue          0,  0,255      49, 32,241      0,  0,255
    skin        224,172,140     208,170,142    224,172,140
  ```

  Windows Photos converts, and it converts with `UX3405CA_…_CMDEF.icm` — the
  profile `GetICMProfileW` returns for this monitor, and the one Argentum reads.
  Predicted against measured agrees within a few levels on every patch. The
  *other* profile listed for this display, `CalibratedDisplayProfile-1.icc`,
  predicts red as 255,3,2 and is not what anything uses.

  GIMP converts nothing. Not one patch moved. Its own shipped defaults say why:

  ```
    (mode display)                   colour management on
    (display-profile-from-gdk no)    never ask the system for the monitor profile
  ```

  No display profile set and told not to fetch one, so it manages colour towards
  nothing and passes sRGB numbers to a wide-gamut panel. That is the same fault
  Argentum's native preview had, and it is why GIMP looks richest.

  So the ordering is: Photos converts, Argentum converts, GIMP does not. GIMP is
  the outlier.

- **And the native surface converts, measured rather than reasoned.** The same
  chart, opened in Argentum with the native renderer live, screenshotted off the
  panel:

  ```
                  Argentum    predicted    Windows Photos
    red         222, 51, 36  222, 51, 36    219, 54, 40
    green       134,250, 80  134,250, 80    132,249, 80
    blue         44, 26,244   44, 26,244     49, 32,241
    skin        211,174,145  211,174,145    208,170,142
  ```

  Exact against its own prediction on every channel of every patch, so the
  conversion reaches the screen. Within three levels of Windows Photos, the
  residual being their tone curves against our plain sRGB curve — a difference
  visible in a table and not on a photograph.

  This took the window in front: the native surface composites behind the
  webview, so nothing that captures a window can see it. Worth knowing before
  the next time something needs measuring on that path.

### Fixed
- **A profile that failed to read once stayed failed for the session.**
  `mods/display_monitor.rs` keys its cache on the monitor, the profile path and
  the file's write time, and re-checks a second later. That covers a profile
  being reassigned or recalibrated. It did not cover the read itself failing —
  the file locked while calibration software rewrote it, a drive that had not
  woken up — because the identity that came back was cached against a path and
  a write time that were *unchanged* afterwards. Every later check matched,
  updated the clock, and handed back the identity again. Nothing about the file
  ever changes to unstick it, and the user has no way to ask.

  A resolution now records whether the profile was actually read, and only a
  real answer may be kept on the strength of an unchanged file. A failure is
  retried on the next check. An sRGB screen still counts as a real answer, so
  it is not re-read every second.

### Where this works, and where it does not

| | Converted? | Why |
|---|---|---|
| Windows 10 / 11 | yes | `GetICMProfileW` on a device context for the monitor the window is on |
| macOS, Linux | **no** | `profile_for_window` returns `None` there. Both have their own way to ask; writing them blind would be three implementations with one tested. Presents unconverted, exactly as before the feature |
| Matrix profile (`rXYZ`/`gXYZ`/`bXYZ`) | yes | The three colourant tags are read as `XYZType` s15Fixed16 and inverted. v2 and v4 alike — the tags are the same shape in both |
| LUT-only profile (`A2B0` and no colourants) | **no** | Nothing to invert, so `None`, so unconverted. Rare on displays, normal on printers |
| Broken or truncated profile | no | Refused rather than guessed at: header shorter than 132 bytes, more than 256 tags, primaries that do not invert |
| Profile tone curves (`rTRC`, `vcgt`) | read and ignored | Measured: applying them was a *worse* match to the rest of Windows than the primaries with a plain sRGB curve. This is the residual three levels against Windows Photos |

Remaining limits, all deliberate:

- **Only the main window's screen.** One window, one conversion.
- **A change takes up to three seconds to show** — a one-second cache and a
  two-second poll. There is a message Windows broadcasts for this, and receiving
  it means a window procedure of our own inside somebody else's application.
- **Exports carry no profile and no conversion.** Correct: an export is sRGB
  data for another program, not pixels for this screen. Tagging them with sRGB is
  a separate question and is on the roadmap conversation, not done.
- **One cache entry.** Dragging between two profiled monitors re-reads the
  profile on each move rather than keeping both. A file read per drag.

- **The two views agree, measured.** The whole feature was found by the crop view
  and the normal view of one photo disagreeing: the crop view goes out as a JPEG
  in an `<img>`, which WebView2 colour-manages, and the native surface did not.
  So the acceptance test is that they now agree. The same unedited frame, no
  crop, screenshotted in both views off the panel:

  ```
                  normal         crop view      delta
    skin (arm)   136, 95, 73    136, 94, 73    -0.4, -0.4, -0.2
    shirt         67, 55, 53     66, 55, 53    -0.5, -0.3, -0.4
    denim         63, 70, 85     63, 70, 84    -0.2, -0.3, -0.2
    grass         78, 85, 66     78, 85, 66    -0.2, -0.2, -0.4
    wall          46, 46, 23     46, 46, 22    -0.5, -0.5, -0.4
  ```

  Half a level out of 255, which is the audit's own figure for agreement — it
  measured 0.5 with the conversion and 3.5 without. The residual is the crop
  preview being a quality-80 JPEG, not a colour difference. Feature closed.

  The caching moved into `rows_now`, which takes the clock and both lookups as
  arguments, because the case is three things a test cannot stage: a monitor, a
  profile Windows assigned to it, and a file that reads one moment and not the
  next. `a_read_that_fails_once_recovers_with_the_file_unchanged` fails against
  the previous version and passes against this one.

---

## 26.37.17 — 2026-09-11

The preview had been showing every photo more saturated than it was, on this
screen, since before any of the colour work started.

### Fixed
- **Display colour management** — `mods/display_profile.rs`,
  `mods/display_monitor.rs`, `shaders/ag_display.wgsl`. The pipeline produces
  sRGB and the native preview handed those numbers to the panel untouched:
  `gpu_processing.rs` asks for a non-sRGB swapchain and `display.wgsl` returned
  what it sampled. Right only if the panel is sRGB. On a wide-gamut display the
  same numbers drive more saturated primaries.

  Nothing on screen said so, because nothing sat beside it — except the crop
  view, which goes out as a JPEG through WebView2 and *is* colour-managed. The
  two disagreed, and an audit measured the gap as exactly this monitor's gamut
  conversion: 0.5 of 255 with it, 3.5 without.

  Now the ICC profile of whichever monitor the window is on is read at runtime,
  the conversion derived from its colourants, and applied in linear light before
  presenting. Three confirmations: the derivation agrees with the audit's own
  matrix to 0.0003, the lookup returns the file the audit named, and the shader
  compiles under naga in a test.

  It follows the window rather than the machine. Moving to another screen is
  caught by the window's own move event, because the frontend only sends a
  transform when *its* idea of one changes and that does not include which
  monitor. A profile reassigned or recalibrated under a window that never moves
  is caught by a check every two seconds, because an idle photo has no frame to
  notice on. Both found by audit, neither reachable by the tests that existed.

  **What it is not:** a full ICC transform. The tone curves are read and ignored
  — matching what the rest of the system appears to do for this class of profile
  measured better than applying them. Matrix-shaped profiles with an sRGB curve
  are what this is measured against; anything else is untested.

### Added
- **Highlight recovery** — `mods/highlights.rs`. Rebuilds a clipped channel from
  the two that survived, using the colour of the photo's own bright pixels
  rather than the camera's white balance, which is wrong exactly where it
  matters. On by default with a switch in the Color panel; a photo with nothing
  clipped comes out bit for bit unchanged.

- **The clipping warning steps through the channels.** Off, L, R, G, B. And Ctrl
  while dragging Whites or Blacks empties the picture to show only what is about
  to go, which is how a white point is actually set — the difference between 250
  and 255 is nothing to look at on a photograph.

### Measured
- Highlight recovery does almost nothing on the test photos, and that is the honest
  result. sRAW cannot be recovered at all: the format keeps one brightness for
  all three channels, so when it saturates they all go. On full RAW there is too
  little clipped — 0.05% of a frame on average — and what does clip is blown in
  every channel at once.

### Changed
- Skin tones added to the roadmap.

---

## 26.37.16 — 2026-09-10

### Added
- **The clipping warning steps through the channels** — `mods/clipping.rs`,
  `argentum/…`, `shaders/modules.wgsl`. The button cycles off, L, R, G, B and
  shows the letter. Red is a blown highlight and blue is a crushed shadow in
  every mode; only the channel being watched changes.

  That is the question the single warning could not answer. A highlight where
  only green has gone still holds two channels of real data; one where all three
  have gone holds nothing, and no editing brings it back. Stepping through tells
  them apart.

  The first version colour-coded all three channels into one view. It answered
  the same question in one look, and it was misread within seconds — because red
  already means *blown highlight*, and there it would also have meant *the red
  channel*. Two meanings on one button. Stepping through the channels was the
  alternative, and is what shipped.

  The mode rides in their existing `showClipping` field. Their button only asks
  whether it is truthy, so a number works without their code knowing modes
  exist — and a new key in the saved adjustments is part of the thumbnail cache
  hash, so adding one would rebuild the whole library once for nothing. Sidecars
  holding `true`/`false` still mean on and off.

  The tooltip names the channel, and the letter carries a `!`, because the
  waveform's own channel selector sits beside it showing a bare `L`.

- **Highlight recovery, written but not proven** — `mods/highlights.rs`. Reads
  the highlight colour out of the photo's own bright unclipped pixels, then
  rebuilds clipped channels from the ones that survived. Runs from the decode
  anchor. A pixel with nothing clipped comes back untouched, so a photo with no
  blown highlights decodes exactly as before.

  It does nothing useful on Canon sRAW, and cannot — see **Measured**. Still
  unproven on full RAW, which is the case it was written for.

### Measured
- **How much of a real photo is clipped.** Written before the recovery rather
  than after it, because camera profiles were built on reasoning that one
  measurement afterwards demolished.

  ```
    2026-09-06   53 photos    mean 0.05%   worst 0.12%
    2023-06-25  217 photos    mean 0.08%   worst 2.6%
    2026-06-06  126 photos    mean 2.03%   worst 45.5%
  ```

  The third folder turned up later. It is 25x the others and is what any recovery has
  to be judged on.

- **Highlight recovery cannot work on Canon sRAW.** Not a limit of the method —
  a property of the format.

  On the worst frame, green clips across 45% of the picture while red and blue
  never reach their own ceilings. That looks like the ideal case. It is not:
  inside the blown region red varies by 0.03% of its mean and blue by 0.02%.
  Flat. Red and blue say the true brightness is 1.00–1.01x where green stopped,
  for 100.0% of 2.4 million clipped pixels.

  rawler's decoder says why. In the 5D Mark II sRAW path, `r`, `g` and `b` are
  each built from the same `y`. The format stores one brightness plus colour, so
  when that brightness saturates all three channels saturate together. There is
  no per-channel headroom for anyone to exploit — darktable can inpaint
  plausible detail from neighbours, but it cannot recover what the file does not
  hold.

### Fixed
- **The mergeability detector could not see half its own subject.** It matched
  `ag_stage_` in their WGSL, so a call to any other `ag_` function was invisible.
  Found by walking through the hole while adding one. It matches any `ag_…(`
  call now, and the anchor counts for `shader.wgsl` and `image_processing.rs`
  were understated and are now declared correctly.

### Changed
- **`ag_stage_display` is wired up.** It was written empty and held open so the
  first display-referred tool would not have to touch their file. The clipping
  view is that tool: one line in their shader, replacing the ten it had. The
  next display-referred tool costs nothing.

---

## 26.37.15 — 2026-09-10

### Removed
- **"Colour is close to darktable, not equal to it"** from the known issues.
  It said camera profiles were the next step in closing the 4–5% gap. They are
  not and never were: darktable renders through the same Adobe matrix rawler
  carries, so that measurement scores agreement with Adobe rather than accuracy,
  and profiles moved it the other way. Listing a gap as a fault while naming the
  wrong cure is worse than not listing it.

  The gap is still real and still unexplained. It belongs in a measurement, not
  in a promise to the user.

---

## 26.37.14 — 2026-09-10

### Fixed
- **sRAW thumbnails failed in dev builds** — `src-tauri/Cargo.toml`, explained in
  `mods/sraw_levels.rs`. The library showed placeholders and the log had
  `PANIC! … pixarray.rs:119 - assertion failed: self.initialized`.

  rawler's CR2 decoder can be asked for a *dummy* image: allocated, never
  filled, so a caller can read its dimensions without paying for a decode.
  RapidRAW asks for one to size thumbnails
  (`raw_processing::get_fast_demosaic_scale_factor`). For sRAW the decoder hands
  that uninitialised buffer to `convert_to_rgb` anyway, and `pixels_mut()`
  asserts on exactly that.

  Release builds never saw it — `debug_assert!` is compiled out — so this was a
  dev-only failure that looked like a broken library.

  Dependencies in this project are already built as release code
  (`[profile.dev.package."*"] opt-level = 3`); asserting inside them contradicts
  that, so `debug-assertions = false` now sits beside it. That silences a bug in
  rawler rather than fixing it, which is the honest description: the bug is
  theirs, the dimensions it returns are correct, and the alternative is patching
  their decoder inside a fork of a fork.

### Notes
- The empty library that led here was mostly not this. Thumbnail hashes cover
  the photo's adjustments, and `cameraProfile` joined the defaults in
  `26.37.12`, so every thumbnail regenerates once on first sight of a folder —
  134 were written while this was being looked into. Only the sRAW frames stayed
  empty.

- The rawler bug is worth reporting upstream: `decoders/cr2.rs` calls
  `pixels_mut()` on lines 167 and 241 without checking `dummy`.

---

## 26.37.13 — 2026-09-10

Two things that were wrong in the same way: a message that named the URL and
not the fault, and a label cut short with no way to see the rest.

### Fixed
- **"Find one" could not reach GitHub** — `mods/profiles_online.rs`. It reported
  "error sending request for url (…)", which is the whole of reqwest's message
  for any transport failure: a DNS failure, a refused connection, a proxy, an
  expired certificate and a timeout all print that same sentence. The cause is
  one level down in `source()`, and hyper puts the useful part one below that.
  Walking the chain turned it into "peer closed connection without sending TLS
  close_notify", which is a different problem and points somewhere.

  With that visible: every request to api.github.com failed after about ten
  seconds, four times out of four, while `curl` fetched the same URL in under
  half a second. Not the network and not the certificate — that build of curl
  cannot do HTTP/2 and so never offers it, while reqwest offers it in the
  handshake. Something between this machine and GitHub agrees to HTTP/2 and then
  abandons the connection.

  So a *transport* failure — not a 404 or a rate limit, which are perfectly good
  responses — is retried once on a client that cannot offer HTTP/2, and once
  that has happened HTTP/2 is not offered again for the session. HTTP/2 is what
  inspection proxies and older firewalls mishandle most, on every platform, and
  HTTP/1.1 is understood by everything; this is one small request either way.

  Verified against the real listing and a real download, both of which had been
  failing consistently and now succeed.

### Added
- **Full text on hover for anything the layout cut short** —
  `argentum/useTruncatedTooltips.ts`. A library card is narrower than most file
  names, so what you see is "2026-09-06_Canon EOS 5D Mark II…" and the part that
  tells one frame from another is the part that got cut.

  Not a tooltip on the file name: that costs a line in a file of theirs, and
  next week the folder path costs another. Their `GlobalTooltip` already shows a
  tooltip for any element carrying `data-tooltip`; it simply has no way to know
  an element is clipped, because that is a fact about layout and only true at
  the moment you point at it. So this fills it in from the capture phase, before
  their listener on `document` runs — ask the browser whether the text is
  actually ellipsised, write the full text into `data-tooltip`, and their
  tooltip does the rest.

  Every clipped label in the app gets it at once, including ones that do not
  exist yet. It never overrides a tooltip written on purpose, and it fires only
  on genuinely ellipsised text rather than on anything that merely overflows.

  Not exercised in the running app — verified by build and by the DOM contract
  it depends on.

### Changed
- **"Find one" is hidden once RawTherapee's own file for that body is here.**
  They publish exactly one profile per camera, so the button had nothing left to
  fetch and would have saved the same bytes beside themselves as a numbered
  duplicate. The test is on the file *name*, which is what the online search
  matches on, so an imported calibration or a hand-made profile does not answer
  the offer — a camera with one borrowed profile can still fetch the real one.

- **The roadmap shows finished work last, each stamped with its release** —
  `argentum/roadmap.ts`, `docs/ROADMAP.md`. A roadmap that deletes what it
  delivered reads as though nothing has been delivered; the interesting half is
  still the part that has not happened. The sort happens on export, so marking
  something done is one word changed and nothing moved.

  Camera profiles are marked done rather than removed. The docs row keeps the
  reason that was given for building them and says plainly that it was wrong: it
  claimed they would close the remaining 4.2% against darktable, and they cannot,
  because darktable renders through the same Adobe matrix rawler already carries.

### Measured
- The one outlier in the ten-photo set is
  `2026-09-06_Canon EOS 5D Mark II_104-6382` — R/G +8.9%, B/G −23.1%, brightness
  91%. It carries a darktable sidecar with five `temperature` and four
  `channelmixerrgb` entries, so the reference was rendered with a hand-set white
  balance. The three frames either side of it, from the same shoot and with no
  sidecar, match us to 0.0%. Not a decoder fault; the yardstick was edited.

### Internal
- The in-memory DCP used by tests takes a camera name, so `profiles` can build
  profiles for two different bodies. Its NUL terminator is appended as a byte
  rather than written as an escape — a literal NUL had already got into the file
  once, which is easy to do and impossible to see. Same class of bug as the
  literal backspaces in `check-mergeability.mjs`.

---

## 26.37.12 — 2026-09-10

Camera profiles. The last piece of the colour work the fork was started for,
and the one that taught the most by not working.

### Added
- **Camera profiles** — `mods/dcp.rs`, `profiles.rs`, `profile_matrix.rs`,
  `profile_correction.rs`. A `.dcp` describes how one camera body renders
  colour, measured rather than published. Read, matched to the camera, chosen
  per photo, applied on the GPU.

  Argentum ships none and cannot: the free collections are published with each
  author's individual permission, not a licence that lets anyone else
  redistribute them. So it fetches one on request from RawTherapee — the file
  going from the project that published it to the user's machine, which is what
  a package manager does — or imports one you already have. That request happens
  on a click and never on its own.

- **My Gear**, a settings tab of its own: the cameras you shoot with and the
  profiles you keep for each. Both lists fill themselves — every photo records
  its body, and a detected lens is added to My Lenses. Lenses moved here out of
  General, which was too broad to be a home for anything.

- **16-bit TIFF export** to the roadmap. Export writes 8 bits a channel today,
  which discards most of what a RAW holds.

### Changed
- **The profile is applied per frame, not at decode.** This was the whole
  lesson. Applying it during the decode is the obvious place and it is the
  wrong one: every other adjustment in this app is applied on the GPU, so there
  is no notion of a setting that needs the file read again. Four attempts to add
  one each broke something else — a black flash on every switch, a debounced
  save racing the choice and reverting it, a preview worker finding no image
  because the decode had cleared it, and a picture stuck blue because the stale
  writer kept winning. None of those were colour bugs.

  darktable applies the input profile as a pixelpipe module and RawTherapee
  applies DCP in its processing pipeline. Per frame is the ordinary
  arrangement; the original was the oddity.

  Identity when no profile is chosen, in the literal sense — the matrix is the
  identity and the shader multiplies by it, so every existing edit renders
  exactly as before.

- **Camera profiles are off the roadmap** — `src/argentum/roadmap.ts` and
  `docs/ROADMAP.md`. Marked done rather than deleted, so the app's roadmap still
  shows what the colour work has covered. The docs row keeps the reason that was
  given for building it and says plainly that it was wrong: it claimed profiles
  would close the gap against darktable, and they cannot, because darktable
  renders through the same Adobe matrix. See **Measured** below.

### Measured
- A camera profile is a small correction, not a new look:

  ```
                       mean      99th    worst
    real profile       2.3       10       18     of 255 levels
    a deliberately
    broken one         8.4       53      104
  ```

  The second was written on purpose — red and blue swapped, white left alone —
  because "is it even applied?" is unanswerable while the honest answer is
  "yes, by two levels".

- Against darktable, a profile scores *worse*: R/G off 4.2% without, 10.0% with.
  That is expected and says the yardstick is wrong rather than the profile:
  darktable renders with Adobe's published matrix, the same one rawler carries,
  so "closer to darktable" measures agreement with Adobe. Only a colour target
  shot on the actual body could say which is more accurate.

  So camera profiles do not close the remaining 4.2%, and were never going to.
  The roadmap said they would; that reasoning was mine and it did not survive
  contact.

### Fixed
- Profile matching, twice. rawler reports the model alone — "EOS 5D Mark II" —
  while a profile is named for the whole camera, so "Find one" reported that a
  profile it was looking straight at did not exist. The second fix depended on a
  maker that older gear lists do not have; the third reading strips the maker off
  the profile's own name and depends on nothing.
- Importing a profile no longer overwrites a different one that shares a file
  name — two people's calibrations of the same body are both called the same
  thing.
- The RawTherapee listing URL was a redirect; that repository was renamed.

---

## 26.37.11 — 2026-09-10

Housekeeping, and the architecture work that makes the rest of it cheap.

### Added
- **An About tab**, one tab of theirs with five sections of ours: About,
  Credits, Roadmap, Known issues, Releases. Two tabs were tried first and did
  not fit — their settings header has a fixed width and the fifth rendered
  clipped, as "Chang…". Their layout is not ours to rebuild, so this stops
  asking it for more room: one empty div in `SettingsPanel.tsx`, our own
  switcher inside it, and a sixth section would cost nothing.

  The acknowledgements used to sit at the bottom of the General tab, under the
  tag-clearing controls, reading as RapidRAW's own credits — wrong in both
  directions now. Only RapidRAW and darktable are named. The models and
  libraries that came with RapidRAW are RapidRAW's to credit and it does;
  repeating them under Argentum's name would be taking credit for assembling
  something we inherited. Their strings for it are restored to upstream text,
  which removed 13 lines of divergence rather than adding any.

- **Release notes people will read**, `src/argentum/releases.ts`. Rendering
  this file in the app was built, then deleted. It is an engineering record —
  file paths, module names, mergeability budgets, "removed the Ko-Fi donate
  link" — and none of that answers the only question a user has, which is
  whether their photos will look different. Filtering it did not work either:
  every pass left more of it behind, because the split is not structural and a
  single sentence can carry both. Two audiences, two documents.

- **A current known-issues list**, `src/argentum/knownIssues.ts`. The one in
  `26.37.1` still said lens auto-detection was broken long after `26.37.6`
  fixed it. A stale warning is worse than none, because it is trusted and the
  reader stops looking. The rule that comes with the file: an entry is deleted
  in the same commit as its fix.

- **Mergeability stated in the app**, not just enforced in the build. Sixteen
  connection points across twelve of RapidRAW's files, and the build fails if a
  feature adds one. That is the difference between a fork that keeps receiving
  upstream's work and one that quietly stops, and it is worth a user knowing.

### Changed
- **Every per-feature cost into their files is now zero.** A budget answers how
  much of their code we have touched; it does not answer what feature number one
  hundred costs, which is the question that decides whether the fork survives.
  Measured before this: a command cost a line of `lib.rs`, a shader tool a line
  of `shader.wgsl`, a UI string thirteen lines across their locale files.

  | | before | now |
  |---|---|---|
  | Tauri command | 1 line | 0 — a match arm in `mods/dispatch.rs` |
  | Shader tool | 1 line | 0 — inside `ag_stage_scene_linear` |
  | Decode fix | 1 line | 0 — a step in `mods/decode.rs` |
  | UI string | 13 lines | 0 — our own i18next namespace |
  | UI control | 3–4 lines | 0 — a portal into an existing marker |

  `check-mergeability.mjs` enforces it: it separates a hook — their code calling
  ours — from the one-time rebrand, and fails the build when a file gains one,
  naming the anchor and what to do instead. Verified by trying both, and writing
  it exposed a real bug in the checker itself: literal backspace bytes where
  `` was meant, which had been hiding two undeclared anchors.

- **The changelog is no longer personal.** Names, a home directory, file-sync
  arrangements and three `See DEC-nn` pointers into a private notebook are out.
  `### Internal` now marks build and repository housekeeping, which is real
  history and still not something a user has any use for.

### Removed
- **The Ko-Fi donate link** from the splash. It funds RapidRAW, which Argentum
  is not, and asking for money on someone else's behalf from a fork's welcome
  screen is the wrong place for it. The link to contribute to RapidRAW stays.

---

## 26.37.10 — 2026-09-09

The green cast and the crushed shadows were the same bug, and it was neither
white balance nor the colour matrix. The affected frames were shot as sRAW,
and sRAW is not sensor data.

Three releases went unrecorded while this was being chased — `26.37.7`
(D50/D65 matrix correction), `26.37.8` (darktable's sigmoid as the tone
curve) and `26.37.9` (reverting it). Both of those attempts are now gone.
They were built against a single frame and were compensating for the bug
below.

### Fixed
- **Canon sRAW / mRAW black and white levels** — `mods/sraw_levels.rs`. These
  formats do not store sensor data. They store luma and chroma, already
  black-subtracted and already white-balanced by the camera. rawler converts
  that back to RGB correctly and then treats the result as sensor data: it
  subtracts the sensor black level (1023 on a 5D Mark II) a second time, and
  takes the white level from its camera table rather than from the file.

  Red sits at roughly half of green at that point, so one subtraction takes
  ~18% off red and ~8% off green. White balance scales the gap up and the
  camera matrix amplifies it again — the cast. The darkest values are 100–300
  counts, so the same subtraction sends them below zero — the crushed shadows.
  On a backlit frame that was 40% of the picture, black before any tool could
  reach it.

  rawspeed, which darktable uses, is explicit about this in
  `Cr2Decoder::decodeMetaDataInternal`: for sRAW the black level is zero and
  the white point is the file's specular white shifted up two bits.

  Keyed on the format, not the camera — three channels out of a Canon decoder.
  The white level is read from the file's own ColorData using the version
  table rawspeed and rawler both carry, which covers every body that shoots
  sRAW. No camera is named anywhere in it.

  Measured linear against darktable at the same stage, one file:

  ```
                     R/G     B/G   brightness
    darktable       0.97    1.03      100%
    before          0.53    0.80       72%
    after           1.00    1.06      101%
  ```

- **The RGB readout stopped showing anything.** It found the photo by taking
  the largest `<img>` on the page. With the GPU renderer on there is no `<img>`
  for the photo — it is a native surface composited behind the webview — so
  what it actually found was the cached `_medium.jpg` thumbnail, present only
  sometimes. Clear the thumbnail cache and the readout goes silent with no
  error. On the welcome screen the same code would have picked the 2048px
  splash background.

  It now anchors on the overlay `<svg>` that `ImageCanvas` positions over the
  photo: sized in pixels to the drawn image, inside the pan/zoom transform, so
  its bounding box *is* the photo at any zoom. Verified against the running app
  over the devtools protocol rather than by eye — aspect 0.6667 against the
  file's 1872×2808, tracking correctly through a zoom.

- **Stale thumbnails after a decode change** — `mods/cache_version.rs`.
  `compute_thumbnail_cache_hash` covers the photo's path, its mtime and the
  adjustments; nothing about how it was decoded. Correct for RapidRAW, whose
  decode never changes, wrong for us. After the sRAW fix the library kept
  showing the green cast while the editor showed the correction — the same
  photo, two colours, depending on where you looked.

  The cache is now stamped with a pipeline number and cleared when it changes.
  Bump `cache_version::PIPELINE` when anything in `mods/` changes what a photo
  looks like. The version belongs in the hash, which is one line in
  `file_management.rs` — already at 30 of its 30 approved lines — so this does
  the same job from our side rather than raising a budget.

### Removed
- **The D50→D65 camera matrix correction** (`mods/colour_fix.rs`, added in
  `26.37.7`). It closed the gap on the one frame it was built against. With
  the real bug fixed it made nine of ten test photos worse: mean R/G error 6.8%
  with it against 4.2% without. rawler's composition matches dcraw and
  rawspeed's legacy path and was not the error it was taken for.

### Added
- **A linear regression harness** — `colour_compare::linear_across_a_set`. The
  earlier comparisons were of finished renders, where a tone curve reshapes
  channel ratios and mixes "the colour is wrong" with "the curve is different".
  This compares the decoders themselves against `darktable-cli` exports made
  with `workflow=none` in linear Rec709, over ten photos from nine shoots and
  both RAW formats. It is what retired the matrix correction.

  Where that leaves us:

  ```
    10 photos   mean |R/G| off 4.2%   |B/G| off 5.1%   brightness 100%
  ```

  Three of the four full RAWs now match darktable exactly.

### Kept
- The shadow toe from the previous commit (`mods/preview_encode.rs`) stays. It
  now recovers 0.0–0.8% of a frame rather than 10.9% — almost all of that was
  the sRAW bug — but their contrast line does still cross zero, so the clip is
  real and the toe never costs anything.

---

## 26.37.6 — 2026-09-09

Lens auto-detection works. darktable identified a Canon EF 135mm f/2 L
instantly while Argentum asked for it to be picked by hand every time.

### Added
- **Canon MakerNote lens reading** — `mods/makernote_lens.rs`. The lens name is
  in the file; nothing in the chain looked where it lives. `kamadak-exif` reads
  standard EXIF, where Canon does not write `LensModel` on this body, and
  rawler's CR2 decoder looks for that same tag before falling back to a numeric
  id which maps to seven candidates: *"unable to determine which lens to use"*.

  darktable succeeds because exiv2 decodes MakerNotes. Rather than take a C++
  dependency for one string, this parses the block directly — it is a standard
  TIFF IFD. Canon only for now; other makers use different layouts.
  *Verified across a folder: 53 files, 53 lenses, none missing.*
- **Detection runs when a photo loads**, not only when the Auto button is
  clicked — `argentum/useAutoDetectOnLoad.ts`. Nothing had ever triggered it on
  load, so it worked on photos where the mode was toggled and silently did not
  on the rest. That looked random and sent the search after the metadata.
- **Re-read metadata button** in the Camera Details pane. EXIF is cached inside
  a photo's `.agdata` sidecar and in a per-folder JSON keyed on mtime and size —
  neither invalidated when the app changes, only when the photo does. Photos
  edited before this fix kept a frozen empty lens and went on failing beside
  neighbours that worked.
- **Crop-factor-aware lens matching** — `mods/lens_crop.rs`. lensfun holds one
  entry per lens *per body it was calibrated on*: the EF 50mm f/1.4 USM appears
  at `cropfactor 1` and at `1.611`. Their matcher scores on the name alone, so
  both tie and the later one wins — giving *"EF 50mm f/1.4 USM (crop 1.6x)"* on
  a full-frame 5D Mark II. Correction measured over the middle of an APS-C frame
  under-corrects full-frame corners, quietly, where it shows most. Now the
  calibration nearest the camera's own crop factor wins.

### Fixed
- **The lens name was being corrupted.** A TIFF ASCII field holds NUL-*separated*
  strings and Canon packs several in; trimming only the trailing NULs glued the
  next one on, producing `EF 85mm f/1.8 USMUSM`. It looked almost right in the
  metadata pane, which is why this read as a matching problem for so long.
- **Mount and focal length are separated** before matching. Canon writes
  `EF135mm`, lensfun writes `EF 135mm`, and a fuzzy subsequence match on the
  joined form could score an unrelated lens higher.
- `.rrexif` renamed `.agexif`, and the last `.rrdata` reference in `tagging.rs`.
  No `.rr*` anything remains.

### Notes
- The refresh button first called `window.location.reload()`. It refreshed the
  data and threw away the session, returning to the welcome screen. It now
  writes the freshly-read EXIF straight into the store.
- `MetadataPanel.tsx` carries the one anchor we have added to a file of theirs —
  a bare `data-argentum` attribute, one line, because there was nothing stable to
  portal against and matching on translated heading text breaks in 12 locales.

---

## 26.37.5 — 2026-09-09

### Added
- **Render indicator** — a small spinner in the toolbar while the preview is
  still being drawn. RapidRAW's existing spinner is driven by `isViewLoading`,
  which is *opening* a photo, not re-rendering after a slider moves. That
  distinction matters when judging colour: a value read mid-render is a value
  from the previous frame.

  Icon only, no label, and only after 140ms — most renders finish in tens of
  milliseconds and a spinner that flashes on every slider tick is worse than
  none. No changes to their files: the pipeline already emits `wgpu-frame-ready`
  when the native render completes.

### Fixed
- **The readout showed the previous frame's colour.** Set temperature to −100,
  pick a neutral patch with the white balance picker, and the numbers stayed at
  the pre-correction values until the mouse was moved.

  Two causes, both ours. A reading was only ever triggered by mouse movement, so
  nothing refreshed it when the picture changed under a stationary cursor. And
  once a refresh *was* triggered, it was dropped if a read happened to be in
  flight — which is exactly the case after a picker click, since the click's own
  read was still running when the sliders moved.

  Now: a change to the adjustments re-triggers a read with the *new* values, and
  a forced refresh that arrives mid-flight is remembered and re-fired rather than
  discarded.

  Worth noting the shape of this bug — a measuring instrument quietly reporting
  stale data is the same failure as the `_medium.jpg` sampling in `26.37.3`,
  and the same failure the readout exists to catch in the first place.

---

## 26.37.4 — 2026-09-09

White balance is now correct, not just consistent — measured against darktable
on a real file rather than asserted.

### Fixed
- **The adaptation was being done in the wrong colour space.** For a RAW file,
  `shader.wgsl:1639` treats the input texture as already linear — but that
  texture has been through `apply_cpu_default_raw_processing`, gamma 2.38 then a
  1.28 contrast boost. RapidRAW's whole pipeline therefore runs on encoded data
  it treats as linear.

  That costs their three-multiplier white balance nothing, because scaling
  channels is invariant to a curve. It costs ours plenty: an sRGB→XYZ matrix and
  a Bradford cone adaptation are only meaningful on linear data. Meanwhile the
  picker solved the illuminant in properly linearised space (`to_scene_linear`).
  Solve in one space, correct in another, and the two never quite meet.

  `dt_white_balance` now decodes to scene-linear, adapts, and re-encodes so
  every tool after it still sees what it was tuned for. Guarded on `is_raw` —
  a JPEG has already been linearised at the top of `main()` and would otherwise
  be corrected twice.

  **Measured spot-white-balancing a neutral patch:** `191·196·191`
  (3% green) before, `193·196·193` (1.5%, reads neutral) after. darktable on the
  same patch: `135·135·133`.

### Added (tests)
- `a_grey_card_under_any_illuminant_comes_back_neutral` — takes a grey card
  under seven illuminants (tungsten through shade, plus two deliberately off the
  daylight locus), solves the sliders, applies what the shader applies, and
  requires the result to be neutral. Pure arithmetic, no image, no GPU.

  This is what localised the bug. It passed, which proved the model was sound
  and moved the search to the pipeline — where the mismatch actually was.

---

## 26.37.3 — 2026-09-09

Mergeability stops being a budget and becomes an architecture, and colour work
becomes measurable.

### Added
- **One mount point for all Argentum UI.** A single `<Argentum />` tag in
  `App.tsx` renders everything of ours through **portals**, anchored to DOM
  elements upstream already has for its own reasons — `data-bench-id="undo"` is
  theirs, for benchmarking. We add no anchors of our own; that would be the same
  problem with extra steps.

  The linter capped how much of their code we touch, but a cap is not a
  solution: at one or two lines per feature we run out, and six of their files
  had already been edited. Their JSX now does not change again no matter what we
  build. `EditorToolbar.tsx` and `ImageCanvas.tsx` are back to untouched.

  If an anchor disappears in an upstream update the portal renders nothing and
  the app still runs — the right failure, since a missing button beats a merge
  conflict. `useAnchor` keeps watching, so it returns if the element does.
- **RGB readout**, toggled from a pipette in the top toolbar. Shows R, G, B of
  the finished picture under the cursor plus the largest channel gap as a
  percentage; under 2% it reads `neutral`, because sensor noise alone moves the
  channels more than that.

  This is the instrument that was missing. Until now "is the white balance
  correct?" had no answer better than an impression, and every colour module
  still on the roadmap — DCP, highlight recovery, filmic — needs the same test.
- **`sample_processed_pixel`** — renders a one-texel ROI through the real
  pipeline and returns its colour. Masks are not applied; it measures global
  colour deliberately.

### Fixed
- **The readout was reading a stale file.** It sampled the only large `<img>` in
  the DOM, which is the cached `_medium.jpg` thumbnail — regenerated on save,
  never while a slider moves. The same spot on a face
  read `207·177·179` at correct white balance and `214·189·192` at temperature
  −100, on a photo that had gone completely blue.

  **The displayed photo is not in the DOM at all.** RapidRAW renders the editor
  view to a native WGPU surface composited behind the webview — which is also
  why there are no canvases in the editing view, and why `finalPreviewUrl` in
  the editor store stays `null`. No frontend code can reach those pixels, so the
  colour now comes from Rust. The thumbnail is still used for *geometry*: it
  sits exactly where the photo is drawn, so its box maps the cursor into the
  image.
- **The readout vanished when zoomed.** `position: fixed` resolves against the
  nearest *transformed* ancestor rather than the viewport, and zoom applies a
  transform — so it was clipped inside the zoomed container exactly when it was
  being used. Portalled to `document.body`.
- **The toggle could not be clicked.** It was first placed over the image, where
  the container's own mousedown handler took the event and zoomed instead. It
  now lives in the toolbar beside undo and redo, which is where a view-level
  control belongs.

---

## 26.37.2 — 2026-09-08

First harvest. The image maths now differs from RapidRAW's, so the sidecar had
to split too.

### Added
- **Real white balance** — `dt_white_balance` in `shaders/modules.wgsl`.
  Bradford chromatic adaptation on the daylight locus, replacing RapidRAW's
  three invented multipliers, which scaled channels independently and shifted
  hue as a side effect. Their `apply_white_balance` is left in place, just no
  longer called.
  *Source: darktable `src/iop/channelmixerrgb.c` @ `98a9ade9`,
  `illuminant_to_xy` and `chroma_adapt_pixel`. Simplified to Bradford linear on
  the daylight locus — darktable offers several CATs and illuminant models.*
- **Auto white balance** — `mods/auto_wb.rs`, with a one-tag button in
  `src/argentum/AutoWhiteBalanceButton.tsx`. Detects the scene illuminant from
  the image and sets the sliders that neutralise it.
  *Source: darktable `src/iop/channelmixerrgb.c` @ `98a9ade9`, function
  `_auto_detect_WB()`. Reference copy at `docs/harvest/dt_auto_detect_wb.c`.*
  Only the **surfaces** mode is exposed. **Edges** is ported faithfully but
  returns implausible illuminants in this pipeline — see the note on
  `DetectMode::Edges` and the ignored test
- **`scripts/check-mergeability.mjs`** — the mergeability rule is now enforced
  rather than trusted. Budgets added lines per upstream file, ignores
  whitespace, runs on `npm start`, and fails the build when a budget is blown.
  Approved trespasses are a named list with reasons and dates, printed on every
  run so they stay uncomfortable

### Changed
- **Sidecars are now `.agdata`**, not `.rrdata`. Once our white balance maths
  diverged, the same stored `temperature` meant two different things in the two
  apps — RapidRAW would open an Argentum edit and render it wrong with no error
  and no clue why. Existing `.rrdata` files are still *read* when no `.agdata`
  exists, so nothing already edited is lost; the first save migrates a photo
  across. See `mods/sidecar.rs`
- **The white balance picker solves in Rust**, sharing the exact inverse
  auto-WB uses, so the two can't drift. It previously computed temperature and
  tint in the canvas from two constants fitted to maths that no longer exists

### Fixed
- Three bugs in the auto white balance port: detection ran on gamma-encoded
  preview pixels rather than scene-linear data, which biased every result
  warm; the slider solve was an approximation of the shader's curve rather
  than its inverse; and the picker's constants were still the old ones
- **Tint ran backwards.** `ag_apply_tint` lowered the illuminant's `y`, which
  makes the *picture* green — while the comment above it, RapidRAW's own
  `tint_mult = (1+t*.25, 1-t*.25, 1+t*.25)`, and every other editor all say
  positive tint is magenta. Auto-WB inherited the inversion through the matching
  inverse, so it had been setting a tint that pushed the wrong way
- **The "exact inverse" wasn't one.** `solve_slider_values` recovered kelvin with
  McCamy's CCT, which follows isotherms, but the forward model only ever moves
  `y`. A tinted point therefore resolved to the wrong colour temperature and the
  `y_locus` subtracted from it was taken at that wrong kelvin. Round trip drifted
  ~15% at the ends — put in −60, got back −51. Now inverts on `x` alone by
  bisection, which is exact because the forward model leaves `x` untouched

- **The picker never settled.** Clicking one spot repeatedly kept moving the
  sliders. It sampled the *processed preview* — an image already carrying the
  current white balance, plus exposure, curves and everything else — so each
  click solved from its own previous output, moved the sliders, changed the
  preview, and gave a different answer next time. The input was wrong, so no
  correction to the maths could have fixed it. The frontend now sends click
  coordinates and Rust samples the geometry-only cache, which is keyed on crop
  and rotation and cannot be moved by a colour slider. It is also the exact
  image auto-WB reads, so the wand and the picker finally agree
- **Shift-click reached the edges mode.** `DetectMode::Edges` is documented as
  disabled — it returns implausible illuminants and pins tint at its limit — but
  the button still passed it on shift-click. Both paths now use surfaces

### Removed
- The kelvin readout added earlier the same day. It invited a false comparison:
  darktable reports the scene illuminant it corrects *from*, ours reports what
  remains after rawler has already applied the as-shot white balance. 6288K here
  and 3615K there are not the same measurement, and showing a number that looks
  comparable is worse than showing none
- `.rrdata` support, deliberately. `mods/sidecar.rs` is gone and
  `exif_processing.rs` loses its call, which also hands two lines back to one of
  their files. Argentum reads and writes `.agdata` only
- The `D50_X` / `D50_Y` constants, unused since the chromaticity plane was
  re-centred on D65. Zero compiler warnings now

### Added (tests)
- Regression tests locking the tint **direction** and the slider round trip.
  Both bugs above shipped because every existing test asserted how *large* a
  correction was and none asserted which way it pointed
- `the_same_point_always_gives_the_same_answer` — pins the property the picker
  actually violated, rather than the arithmetic around it. Two earlier fixes
  targeted the maths and left the real fault, a bad input, untouched

---

## 26.37.1 — 2026-09-08

First build. A rebranded fork of RapidRAW that compiles and runs. **No image
code changed yet** — it renders identically to RapidRAW, by design.

### Added
- **Argentum identity** — its own Tauri identifier (`co.argentum.editor`), so it
  no longer shares AppData with an installed RapidRAW. The two were one app as
  far as Windows was concerned and would have overwritten each other's settings
- **Ag icon** — periodic-table tile, silver 47, generated to every platform size
- Credits for **RapidRAW** (named as the entire foundation, Timon Käch's work)
  and **darktable**, in Special Thanks across all 13 locales
- `scripts/check-identity.mjs` — clears `src-tauri/gen` automatically when the
  identifier changes, so the asset-protocol scope can't go stale
- Project docs: architecture, the add-a-tool recipe, harvest menu, roadmap

### Changed
- Version scheme to `yyyy.isoWeek.release`, replacing RapidRAW's inherited
  `1.6.3`, which claimed a maturity this doesn't have
- Crate, lib and binary renamed to Argentum

### Fixed
- **Thumbnails silently broken after an identifier change.** Tauri bakes the
  asset scope into generated schemas and they went stale. Full images still
  opened, so it looked like broken RAW decoding rather than a config problem.
  Now handled automatically by the identity check

### Internal
- **Dev build crashing on start.** A leftover `.cargo/config.toml` put `target/`
  at the repo root, where Vite tried to watch 819 crates of build output and
  died on a locked `.exe`. Tauri's default `src-tauri/target` is already ignored
- `setup.ps1` unparseable — two em-dashes in a UTF-8 file, which PowerShell 5.1
  reads as ANSI
- `tauri.conf.json` unparseable — a UTF-8 BOM, written by PowerShell's
  `-Encoding utf8`

### Known issues
- **Zoom and pan feel sluggish** next to darktable, and the mask overlay moves
  before the image does. Present in released RapidRAW too, so it's inherited
  rather than something we caused. Not yet diagnosed — settings, the overlay, or
  the render pipeline.
- **Lens auto-detection fails** for EF glass — the decoder can't map Canon's lens
  ID, so lensfun is never consulted even though the profile is bundled. Manual
  selection works.
- **Click-to-select masking can't be refined.** Each click makes its own
  sub-mask; SAM supports multiple positive/negative points but only one is ever
  sent.
