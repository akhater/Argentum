# Changelog

Newest first.

**Based on RapidRAW `1.6.3` @ `97fada3`** — updated whenever upstream is merged.

## Versioning: `yyyy.isoWeek.release`

```
2026.37.1
  │    │ └── release within that week, counting from 1
  │    └──── ISO week number
  └───────── ISO year
```

No arguments about what counts as major or minor. The version says when it was
built, which is the thing you actually want to know a year later.

Current week:

```bash
date +%G.%V
```

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

## 2026.37.12 — 2026-09-10

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

## 2026.37.11 — 2026-09-10

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
  `2026.37.1` still said lens auto-detection was broken long after `2026.37.6`
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

## 2026.37.10 — 2026-09-09

The green cast and the crushed shadows were the same bug, and it was neither
white balance nor the colour matrix. The affected frames were shot as sRAW,
and sRAW is not sensor data.

Three releases went unrecorded while this was being chased — `2026.37.7`
(D50/D65 matrix correction), `2026.37.8` (darktable's sigmoid as the tone
curve) and `2026.37.9` (reverting it). Both of those attempts are now gone.
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
  `2026.37.7`). It closed the gap on the one frame it was built against. With
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

## 2026.37.6 — 2026-09-09

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

## 2026.37.5 — 2026-09-09

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
  stale data is the same failure as the `_medium.jpg` sampling in `2026.37.3`,
  and the same failure the readout exists to catch in the first place.

---

## 2026.37.4 — 2026-09-09

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

## 2026.37.3 — 2026-09-09

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

## 2026.37.2 — 2026-09-08

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

## 2026.37.1 — 2026-09-08

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
