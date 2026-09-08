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

**Every harvested module gets an entry naming its source file and upstream
commit hash.** That line is the only thing connecting our code back to where it
came from — without it there's no way to tell later whether upstream moved on.

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
  never while a slider moves. Caught by AK, not by me: the same spot on a nose
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
- Backup via a bare git repo in OneDrive — `git push`

### Changed
- Version scheme to `yyyy.isoWeek.release`, replacing RapidRAW's inherited
  `1.6.3`, which claimed a maturity this doesn't have
- Crate, lib and binary renamed to Argentum
- Working tree moved out of OneDrive entirely — see below

### Fixed
- **Working tree in OneDrive.** OneDrive doesn't tolerate directory junctions
  inside a synced folder: it replaced the `node_modules` junction with a real
  folder and began syncing 182 packages. Moved the tree out; OneDrive now holds
  a bare git repo instead, ~7MB, which can never pick up a binary because git
  doesn't track them
- **Dev build crashing on start.** A leftover `.cargo/config.toml` put `target/`
  at the repo root, where Vite tried to watch 819 crates of build output and
  died on a locked `.exe`. Tauri's default `src-tauri/target` is already ignored
- **Thumbnails silently broken after an identifier change.** Tauri bakes the
  asset scope into generated schemas and they went stale. Full images still
  opened, so it looked like broken RAW decoding rather than a config problem.
  Now handled automatically by the identity check
- `setup.ps1` unparseable — two em-dashes in a UTF-8 file, which PowerShell 5.1
  reads as ANSI
- `tauri.conf.json` unparseable — a UTF-8 BOM, written by PowerShell's
  `-Encoding utf8`

### Known issues
- **Zoom and pan feel sluggish** next to darktable, and the mask overlay moves
  before the image does. Present in released RapidRAW too, so it's inherited
  rather than something we caused. Not yet diagnosed — settings, the overlay, or
  the render pipeline. See DEC-32
- **Lens auto-detection fails** for EF glass — the decoder can't map Canon's lens
  ID, so lensfun is never consulted even though the profile is bundled. Manual
  selection works. See DEC-27
- **Click-to-select masking can't be refined.** Each click makes its own
  sub-mask; SAM supports multiple positive/negative points but only one is ever
  sent. See DEC-29
