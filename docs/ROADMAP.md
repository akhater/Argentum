# Roadmap

Order is chosen so the app is useful early and each step makes the next easier.
**Nothing here is a commitment — stop at any line and what you have still works.**

Status: ⬜ not started · 🟨 in progress · ✅ done

---

## Groundwork

| | What | Why now | Effort |
|---|---|---|---|
| ✅ | Fork, build, confirm it runs | Nothing else can start | 1 day |
| ⬜ | **Serve our own dependencies** | Argentum builds and runs on infrastructure belonging to one person, and it already bit us: on 2026-09-12 a HuggingFace 429 took down CI because every build downloads the ONNX runtime from his account. **Build-time:** the ONNX runtime (HuggingFace `CyberTimon/RapidRAW-Models`), `rawler` (`github.com/CyberTimon/RapidRAW-DngLab`, the RAW decoder everything rests on) and `gphoto2` (`github.com/CyberTimon/libgphoto2-rs`, tethering builds only). **Runtime, for every user:** eight AI models from that same HuggingFace repo - SAM encoder and decoder, u2net, skyseg, CLIP and its tokeniser, NIND denoise, LaMa, Depth Anything - plus the preset manifest and the community sample image from `CyberTimon/RapidRAW-Presets`. If any of it is renamed, made private or deleted, Argentum stops building and masking stops working for everyone who installed it, with nothing we can do from here. This is not a criticism of him: they are his repositories to manage and he owes us nothing. **Separately, a packaging bug:** `packaging/io.github.CyberTimon.RapidRAW.yml` is his Flatpak manifest and builds *his* application from *his* git, so anyone packaging Argentum that way gets RapidRAW. Worth doing in the order the risk sits: mirror the models and the runtime somewhere we control, pin the git dependencies to a commit rather than a branch, and write our own Flatpak manifest | 1–2 days |
| ⬜ | One-folder data location | Cheap now, annoying to retrofit | ½ day |
| ⬜ | Portable build (no installer) | Same reason | ½ day |

## First tools — the colour block

These three are one session's worth of thinking. They all sit in the same part of
the pipeline, so doing them together means understanding it once.

| | What | Source | Why | Effort |
|---|---|---|---|---|
| ✅ | **White balance** | darktable | Done 26.37.2, plus auto-WB. Built the sRGB↔XYZ↔Bradford conversion every later tool reuses | 3–4 days |
| ✅ | **DCP camera profiles** | RawTherapee | Done 26.37.12. Per photo, found online or imported, applied on the GPU per frame. It did **not** close the 4.2% gap against darktable, and could not have: darktable renders through the same Adobe matrix rawler already carries, so that measurement scores agreement with Adobe, not accuracy. The reasoning in the line above was wrong | 3–4 days |
| ✅ | **Highlight recovery** | darktable | Biggest visible rescue on real photos. RapidRAW has a Highlights *slider*, which is a different thing — it can only move detail that survived. This rebuilds a channel that clipped from the two that did not. Raw domain, before demosaic, through the decode anchor we already have | 2 days |
| ✅ | **Clipping preview** | Lightroom | Hold a key on Blacks/Whites and see which pixels are about to lose everything. Recovering highlights without it is guesswork. Shift and Alt on a slider are taken (fine adjustment), so the key has to be chosen | 1 day |

**Order matters here:** white balance first (it builds the colour conversion),
then DCP (same pipeline stage, and it changes what "correct" white balance even
looks like), then highlight recovery. That ordering held; the reason given for
DCP did not — see the row above.

**After these three, it's an editor worth using.** Reasonable place to stop and
just take pictures for a while.

On DCP specifically — RawTherapee ships hand-made DCP profiles that auto-match a
camera on open, so this is partly about *using* existing profiles rather than
building anything from scratch. Found by comparing against MeraRAW.

## Export

| | What | Why | Effort |
|---|---|---|---|
| ✅ | **16-bit TIFF export** | Done 2026-09-13. The render writes into an `rgba32float` storage texture for TIFF and quantises once at the encode. The encoder was already correct - it was being handed 8-bit data. See `mods/export_precision.rs` and the `high-precision-export` registry entry | done |
| ⬜ | **f32 export input, previews untouched** | **Constraint, from AK:** the preview path does not change - no 32-bit input, no extra bandwidth per interactive frame. Full precision is for the export plumbing only; a 32-bit input across the whole pipeline is four times the bandwidth on every frame to serve a file written once. **Confirmed architecturally free** (reviewed 2026-09-13): `render_high_precision` builds its own input texture and its own `GpuProcessor`, and never reads `state.gpu_processor` or `state.gpu_image_cache`. Every bind-group layout, pipeline, blur and flare texture, tile output, per-run mask array and LUT texture is owned by that processor. The only things an export shares with a preview are `context.device`, `context.queue`, `context.limits` and two CPU caches read before the render. Nothing GPU-side is shared, so previews stay byte-for-byte as they are. **The only blocker is the flare pass.** Of the four binds of `input_texture_view`, the main pass and both blur passes are `filterable: false` and use `textureLoad` only, so `Rgba32Float` binds there with no device feature and no shader change. The two flare passes go through `flare_bgl_0`, which is `filterable: true` with a `Filtering` sampler, and `flare.wgsl` calls `textureSampleLevel` - and that is the only thing needing `FLOAT32_FILTERABLE`, and only when `flare_amount > 0`. **Fix:** in the High processor only, upload a second `Rgba16Float` copy via the existing `to_rgba_f16` - bit-identical to what a preview samples - and bind it in the two flare bind groups. About 4 lines in their file (one `Option<TextureView>` and two `unwrap_or`), no layout change, no shader change, no new feature, no per-machine variation. Declaring the High flare layout non-filterable instead was rejected: it makes an export's bloom nearest-sampled where the preview's is bilinear. **Intermediates:** the four blur textures are read unconditionally but contribute only through sharpness/tonal/clarity/structure, which is why the measurement below landed exactly on the upload's grid at neutral settings. With those tools on, their f16 error rides on the blurred term only. To close that too: the reusable texture descriptor and the blur storage format become precision-dependent (2 lines) and `blur.wgsl` gets the same textual format rewrite `export_shader_source` already does. Flare's three 512x512 textures and the LUT stay f16 - an additive glow and a LUT's own resolution both dwarf 2^-11. **Expected result:** the decoded RAW is already `ImageRgba32F` and nothing narrows it before upload, and the shader is f32 throughout, so at neutral settings the ceiling becomes the `u16` file itself. With blur-based tools on and the blur textures still f16, 11 bits on the blurred component - measure it rather than assume. **Cost at 100 MP:** f32 input 1.63 GB (was 0.81), plus a 0.81 GB f16 flare copy only when flare is on, plus blur/ping-pong at f32 425 MB (was 212), plus the existing 85 MB tile - 2.1-2.9 GB for the export processor. Use `as_rgba32f()` rather than `to_rgba32f()` or peak RAM grows 1.6 GB. Exports get slower and heavier; previews do not. **What a test will not catch:** VRAM exhaustion on a 4 GB card with a 100 MP file, which is device loss for the whole session - the honest guard is an up-front size estimate that errors before allocating, never a silent fallback to f16; driver bugs in `rgba32float` storage writes on hardware the test machine is not; and export-vs-preview divergence, which nothing tests today | 2 days |
| ⬜ | **EXIF in an exported TIFF** | `write_image_with_metadata` returns early for TIFF behind an upstream `FIXME: temporary solution until I find a way to write metadata to TIFF`, so a TIFF export carries none. That FIXME looks stale: `little_exif` 0.6.23, already a dependency, lists TIFF in `FileExtension` and matches it both by extension and by magic bytes. Needs a round-trip test against a file the `tiff` crate wrote before the early return is removed - which is why it was not folded into the precision work | ½ day |
| ✅ | **A watermark blends in 8 bits** | Fixed 2026-09-13, same day it was written down, because two reviews independently said it belonged in the feature rather than after it - and both were right: `imageops::overlay` rewrote every pixel in the stamp's *bounding box* through `Rgba<u8>`, transparent ones included, so a watermark at zero opacity quantised a rectangle of the photograph and put nothing there. `overlay_preserving_precision` uses upstream's own blend arithmetic at f32 and delegates to theirs unchanged for 8-bit images | done |

## Library

| | What | Why | Effort |
|---|---|---|---|
| ⬜ | **Group by date, camera or lens** | The grid is one flat list — `useSortedLibrary.ts` sorts and filters, and nothing groups. Needs no index, so it can be done before the catalogue | 2 days |
| ✅ | **Sort by capture time, not file time** | `useSortedLibrary.ts` now prefers `DateTimeOriginal` and falls back to the file's modified time only when a photo has no capture date | ½ day |

## Catalog

Browse-only offline. **No smart previews** — decided.

| | What | Why | Effort |
|---|---|---|---|
| ⬜ | Photo index (SQLite) | Browse photos with the drive unplugged | 1–2 weeks |
| ⬜ | Offline thumbnails | Mostly already cached — needs the index to reach them | included |
| ⬜ | **Offline indicator** | A badge on photos whose drive isn't connected | small |
| ⬜ | **Remap location** | Relink a moved folder or a re-lettered drive. The recurring real-world pain | 2 days |

Largest single piece. Also the only part we build rather than copy.

## Edit history

| | What | Effort |
|---|---|---|
| ⬜ | **Persist the history stack to the sidecar** | 2 days |

Not building history — RapidRAW already has a 50-step stack with undo/redo and a
right-click History panel. But it lives in the frontend store only
(`useEditorStore.ts`) and is never written to the sidecar, so closing the photo
loses it. This is about making it survive.

Open question when we build it: 50 full snapshots per photo bloats the sidecar.
Cap the depth, store deltas, or both.

## Look and feel

| | What | Effort |
|---|---|---|
| ⬜ | Filmic / Sigmoid tone curve | 2 days |
| ⬜ | Color balance rgb | 2 days |
| ⬜ | Color equalizer | 2 days |

## Masking

| | What | Why | Effort |
|---|---|---|---|
| ⬜ | **Name the objects, then mask one** | `generate_ai_subject_mask` takes a `start_point`/`end_point` — SAM only segments what you have already boxed by hand. A detection pass would list what is in the frame and hand SAM the box, so masking the dog is a tick rather than a drag. RAM++ **classifies** and does not localise, so a grounding model (Grounding DINO / YOLO-World) sits between it and SAM — which is the Grounded-SAM pipeline, not something new. `ort` and a SAM download path already exist; the cost is two more models on disk and the picker UI. Tags fall out of it for free and make library search work on content | ~1 week |
| ⬜ | **Stop the sidecar re-sending every mask on every keystroke** | Measured on one real photo: an AI mask is a full-resolution 8-bit PNG — 5616x3744, 254KB, 340KB once base64'd into the JSON. Linear and radial masks cost nothing, because they are stored as geometry. So the whole cost is AI and brush masks at ~330KB each, and ten of them on one photo is a 3.4MB sidecar. Nothing crashes at that size; what hurts is that `debouncedSave` re-serialises the entire adjustments object, bitmaps included, 300ms after *any* change and ships it over IPC — so nudging a slider moves megabytes, and OneDrive re-uploads the lot. Four options, in the order they are worth doing: **(1) store masks at reduced resolution** — half res is a quarter of the bytes and a feathered selection is resampled on use anyway, taking ten masks from 3.4MB to 650KB, and this is small and self-contained; **(2) send only masks that changed** and merge the rest in Rust, which is the real fix and takes a slider nudge from 3.4MB to 3KB — but it touches `useImageProcessing.ts` and `file_management.rs`, and that file is at 31/31 of its budget, so the logic has to sit on our side of the line; **(3) split bitmaps into files beside the `.agdata`**, which also removes the 33% base64 tax and stops OneDrive re-uploading untouched masks, at the cost of orphan cleanup and keeping them travelling with the photo; **(4) a database, which does not solve it** — the cost is per-keystroke serialisation rather than storage, and it would break edits living next to the pictures. Ten AI masks on one photo is expected, not hypothetical. **Migration, which is why the order is what it is:** (1) needs none, because a mask is resampled when it is applied, so old full-resolution ones keep working beside new small ones; (2) needs none either, being a transport change with the file format untouched; (3) migrates itself one photo at a time - read `maskDataBase64` when it is there, write the PNG beside the sidecar on the next save and drop the field, no batch job and no flag day; (4) is the only one needing a real migration, with no way back, which is a further reason against it. **Measured on AK's own masks, 2026-09-12, rather than assumed.** The worry was that a lower-resolution mask would soften detail on a 21MP photo, and it does not, because there is no detail in the mask to soften: the steepest change between adjacent pixels is 11 levels of 255, so the edge ramps over roughly 23 pixels, and 95% of the mask is flat 0 or 255. Downsampled and brought back up: half resolution is off by at most **4 levels of 255** with one pixel in 100,000 differing by more than two; quarter is at most 8; an eighth at most 18. Sizes re-encoded from the same mask - full 121KB, half 53KB, quarter 18KB, eighth 6KB. **A second saving found while measuring, free of everything:** that mask is stored at 254KB but re-encodes to 121KB with no change to a single pixel, so whatever writes it is not optimising the PNG. Halving it needs no resolution change, no schema change and no migration. Half resolution plus a proper encoder is 254KB to 53KB, about 4.8x, and takes ten masks from 3.4MB to 700KB. **Caveat, untested:** this was measured on AI masks only. A brush at zero feather may genuinely have hard edges, so key the resolution on mask type rather than applying it to everything | 1–3 days depending on option |
| ⬜ | **Never let a failed load save over a good sidecar** | Autosave's only guard is `prev.adjustments !== adjustments` in `useImageProcessing.ts` — a reference comparison. It answers "is this a different object than last time", not "did this photo's real data finish loading". Masks live inside `adjustments`, so if the editor state is ever reset to defaults while a photo is selected — after a crash in the mask overlay, say — that counts as a change and is written straight to the sidecar, and a 768KB file with three masks becomes a 6KB file with none. Not proven to be what cost `104-6535` its masks, because the evidence for that is in OneDrive version history rather than here, but it is a mechanism that exists in the code today and the shape matches exactly. A save should be refused unless the adjustments being written came from a completed load of that same path | 1 day |


### Sidecar size — the counter-case, and the decision

The two rows above argue for shrinking masks. This is the argument against doing
it now, and it is the one that won. Recorded because a plan that keeps only the
winning side is useless when the question comes back.

**Keep the sidecars.** Do not switch storage architecture or reduce mask
resolution just before a release. The current format can evolve without
abandoning existing edits.

"Sidecar" is a standard *approach* — a companion file beside the photo. It does
not imply a universal editing format. `.agdata` being application-specific is
normal: even editors using XMP keep application-specific editing instructions in
it, and Lightroom now separates heavier data into an additional ACR sidecar.

**There is no universal acceptable sidecar size.** These are engineering
guidelines, not format limits:

| Size per edited photo | Assessment |
|---|---|
| A few KB to hundreds of KB | No reason to worry about storage alone |
| 1–5 MB with several AI masks | Reasonable; benchmark repeated saves |
| Tens of MB | Investigate serialisation, memory, syncing, asset separation |

3.4MB is not inherently a problem. Ten thousand such sidecars would be ~34GB,
so library scale is the thing to watch, not one file.

**The performance question is how often that data is processed, not how big it
is.** Rendering already avoids retransmitting some cached mask data, though it
clones the adjustments first. Autosave still sends the full adjustments, reads
the existing sidecar, serialises the replacement and rewrites it — and also
schedules thumbnail generation. So size alone cannot say whether anyone will
feel lag. **There are no measurements yet that justify calling this a release
blocker, nor calling it seamless.**

**On the measurements in the row above:** they support "half resolution produced
a small error on those samples". They do **not** prove imperceptibility across
photographs and adjustments — a mask's error affects the final image more
strongly the stronger the adjustment behind it. The claim that a maximum step of
11 proves there is no fine detail was also too strong.

The finding worth keeping is the **lossless PNG re-encode**: 254KB to 121KB with
identical decoded pixels costs no resolution, no schema change and no
compatibility. It still needs verifying across more masks, and the ratio will
not be identical for every one.

**Decision, 2026-09-12 — revisit when there is a measurement, not before:**

1. Keep `.agdata` and full-quality masks.
2. Evaluate better lossless PNG compression, applied when a mask is created or
   changed — not on every slider save.
3. Benchmark a demanding photo with 10–20 masks on the minimum supported
   hardware: slider responsiveness, save completion, reopening, thumbnail load.
4. Establish compatibility tests against existing sidecars *before* any format
   change.

For a later storage upgrade the seamless path is straightforward to design: keep
reading embedded masks, introduce a versioned format with external references,
write and verify the assets before replacing the sidecar, keep the original
during conversion. Moving, copying and renaming a photo must carry those assets
too. **New versions reading old edits is achievable; old releases reading a new
format is a separate promise** — leaving existing files untouched until an
explicit or safe conversion keeps that distinction manageable.

## Detail

| | What | Effort |
|---|---|---|
| ⬜ | Diffuse or sharpen | ~1 week |
| ⬜ | Profiled denoise | ~1 week |
| ⬜ | Local laplacian contrast | ~1 week |


## Upstream pull requests worth taking

RapidRAW's open pull requests, triaged against what Argentum already has. Each
claim below was checked against this repository on 2026-09-12 rather than taken
from the pull request's description - three of them did not survive that check,
which is why the order differs from the obvious one.

| | Take | Verified here | Effort |
|---|---|---|---|
| ⬜ | **#1307, the AI patch cache key** | `cache_utils.rs` hashes `.len()` of `color`, `mask` and `patchDataBase64` rather than their contents, so two patches of equal length collide and the wrong one is rendered from cache. Visibly wrong output, which is why it goes first | 1 hour |
| ⬜ | **#1307, the unbounded LUT cache** | `app_state.rs` holds `Mutex<HashMap<String, Arc<Lut>>>` with no eviction | 1 hour |
| ⬜ | **#1307, the unchecked mask index** | `export_processing.rs` does `all.mask_adjustments[mask_index]` with no bounds check - a panic, not an error | 1 hour |
| ⬜ | **#1619, `Arc<DynamicImage>` for the geometry cache** | `app_state.rs` stores a bare `HashMap<u64, DynamicImage>` and clones the whole pixel buffer on every hit, while the two caches either side of it already use `Arc` | 2 hours |
| ⬜ | **#1633, the sRGB exponent** | Real: `raw_processing.rs` uses `powf(3.0)` where every other site in the tree uses `2.4`. Worth taking because it is one line and standards-correct - **not** because it matters here: the function is file-private and only reached under `is_linear_format && apply_ungamma`, which a Canon CR2 never is | 5 minutes |
| ⬜ | **#1705 second commit, Lensfun evaluation** | `image_processing.rs` multiplies `lens_distortion_amount` by an arbitrary `2.5` in four places. The PR also corrects radius normalisation, coefficient rescaling and crop-factor matching. Take the maths, leave the bundled #1687 embedded-profile work. Put as much of it as possible in `mods/` and leave one call behind | 1 day |
| ⬜ | **#1608, lens geometry after EXIF orientation** | Argentum warps and lens-blurs first and applies `orientationSteps` afterwards, which is the wrong order for portrait-oriented files. The earlier plan was to wait for upstream to merge it and receive it in a merge - **that plan is now expired**: the 2026-09-12 merge brought ten upstream commits and this was not among them, so waiting means shipping wrong output on portrait files indefinitely | 1 day |
| ✅ | **#1466, 16-bit TIFF** | Done 2026-09-13, and the triage above was half wrong about it. #1466's *idea* is what was worth taking - build the export pipeline by rewriting the storage format in the shader source, gate the dither behind a pipeline constant - and that is marked under `// upstream #1466`. The idea, not the text: their override is a `u32` tested with `== 0u`, ours is a `bool` and a negation. Its encoder was not needed: `DynamicImage::to_rgb16` already quantises f32 correctly and was only ever being handed 8-bit data. Its `rgba16float` target was not taken either; ours is `rgba32float`. **Nothing of #1395 is used**: its bounded intermediate textures are already here as `clamped_tile_size`, and its capability gate belongs to the half-float design this did not follow - not re-read against a 32-bit target, so look at it rather than dismiss it if the f32 input above is ever attempted. Estimated at a week. Three of their files take part - two at one import each, the shader at a marked block - but that is the anchor count, not the maintenance cost: a constructor parameter, four descriptors reading their format from it, the readback strides, a marked block in the shader and four export call sites. See the `high-precision-export` registry entry, which lists all of it | done |
| ⬜ | **#1569, missing sidecars and filename collisions** | Useful data-integrity logic, manual port only - it assumes RapidRAW's sidecar naming and we use `.agdata`. Raised in priority by what was found on 2026-09-12: autosave can write back state it never finished loading, which is the same family of fault. `file_management.rs` is at 31/31 of its budget, so the logic must live on our side | 1 day |
| ⬜ | #1626 export XMP keywords, #1246 EXIF timezone offset | Small metadata-correctness fixes. We read `dc:subject` and `OffsetTime*` but do not clearly carry either into exported files | ½ day each |

**Already done here, and done better - do not take these.** #1676 colour-managed
white balance (we have scene-linear Bradford adaptation, Kelvin/tint inversion,
auto-WB, a stable picker and regression tests; only its dual-illuminant matrix
interpolation is a separate idea worth considering). #1219 highlight clipping
(a soft knee *after* development, where we reconstruct the clipped channel
*before* demosaic - a harder problem, already solved). #1557 automatic lens
correction (we detect on load, wait for EXIF, read Canon MakerNotes, save the
result and add it to My Gear). #1128 embedded JPEG previews (present, and used
as a guarded fallback when RAW decoding panics).

**Sounds like a roadmap item but is not.** #1578 app-data consolidation is a
thirty-commit branch that centralises under OS app data rather than producing a
portable one-folder install. #1577 "film rolls" are hand-made date ranges, not
grouping by capture date, camera or lens. #727 is grouped recursive folders plus
unrelated colour changes. #1335 is named snapshots in the adjustment object, not
persistence of our undo stack. #1713 is face recognition and scene-aware
adjustments across 140 files, not "name the objects then mask one". #1263
replaces SAM with foreground-only BiRefNet, which works *against* selecting one
named object.

**One correction to the original triage, for the record:** it listed four bugs in
#1307. There are three. The fourth, a zero-size divide in the resize path, does
not exist - `downscale_f32_image` already guards both the zero output and the
zero ratio. Checked before it was believed.

## Already covered — nothing to build

**Lens correction is not missing.** RapidRAW ships a complete one: auto-detect
from EXIF, manual mode, a "my lenses" shortlist in Settings, and distortion /
vignetting / CA correction. It bundles the full lensfun database.

Checked against the actual kit — every item is in that database:

| | In bundled lensfun |
|---|---|
| EF 135mm f/2 L | ✅ |
| EF 85mm f/1.8 | ✅ |
| EF 50mm f/1.4 | ✅ |
| EF 24mm f/2.8 | ✅ |
| EOS 5D Mark II | ✅ |

Works on day one. Revisit only if it proves weak on real files — that's a
day-one test, not a guess.

---

## Someday — only if Argentum is ever open-sourced

Not needed here. Kept on the list because other people *would* need them, and
they'd have to exist before a public release is worth making.

| | What | Who needs it | Effort |
|---|---|---|---|
| ⬜ | **Lens correction from embedded RAW metadata** | Modern mirrorless shooters — Canon RF, Sony E, Nikon Z. That glass is often designed assuming software correction and is frequently absent from lensfun. RapidRAW is lensfun-only today | 3–4 days |
| ⬜ | **Demosaic (RCD / Markesteijn)** | Fuji X-Trans shooters, for whom standard demosaic is genuinely poor | ~1 week |
| ⬜ | Cross-platform build and test (Linux, macOS) | Everyone not on Windows. The one-folder / NoCloudZone layout is Windows-shaped | unknown |
| ⬜ | Sensible defaults for the data folder | Current setup hardcodes local paths | small |

**Don't build these speculatively.** They only earn their time if a public
release is actually on the table. Listed so it stays a choice, not an oversight.

---

## Deferred — wanted, but not now

| | What | Note |
|---|---|---|
| ⬜ | **Offline editing** (Lightroom-style smart previews) | Keep a medium-res copy so photos on an absent drive stay editable, applying the changes when the drive comes back. ~1 week on top of the catalog. Revisit only if the offline indicator and remap turn out not to be enough in practice |

---

## Open questions

- **`.agdata` next to photos, or in the data folder?** Next to photos = edits
  travel with the pictures. In the folder = nothing left behind if you bin the project.
- **Does white balance decode RAW that was never encoded?** Found 2026-09-13
  while scoping the high-precision export, and *not confirmed* - written down so
  it is checked rather than forgotten.

  `modules.wgsl` (`dt_white_balance`) calls `ag_to_scene_linear(color)` when
  `is_raw_image != 0`, with the comment "Only RAW arrives encoded. A JPEG has
  already been through srgb_to_linear at the top of main()". That assumption
  holds for the CPU auto-WB path, which reads the warped cache that `lib.rs`
  encodes with `apply_cpu_default_raw_processing` before caching.

  It may not hold for the render. `raw_processing.rs` develops to `ImageRgba32F`
  in scene-linear, and neither `compute_full_transformed_res` nor
  `process_image_for_export_pipeline` calls `apply_cpu_default_raw_processing` -
  the only call on that side is the mask-warp cache. If RAW reaches the GPU
  already linear, the shader decodes it a second time and every white balance on
  a RAW is solved against the wrong signal.

  Against that: `shader.wgsl` main() treats the texel as linear for
  `is_raw == 1`, so something may already compensate, and the auto-WB tests pass
  today. Which means either the two paths disagree and the tests only cover one,
  or there is a compensation that makes this comment misleading rather than
  wrong. Both are worth knowing.

  **To settle it:** render one RAW with a known illuminant, log the value
  reaching `dt_white_balance` and compare it against the same photo's decoded
  scene-linear pixel. One number tells you which it is.

- **History depth in the sidecar** — full snapshots bloat the file. Cap it,
  store deltas, or both. Decide when building it.
