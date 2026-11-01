# Roadmap

Order is chosen so the app is useful early and each step makes the next easier.
**Nothing here is a commitment — stop at any line and what you have still works.**

Status: ⬜ not started · 🟨 in progress · ✅ done · ❌ removed

---

## Shipped without a row

Work that went out without being planned on this page, newest first.
[CHANGELOG.md](../CHANGELOG.md) has the detail and the source of each.

| Version | What |
|---|---|
| `26.44.5` | Arrows on the thumbnail bar and a button to switch between the photo and the thumbnails. Apple Silicon Mac builds move to macOS 15. Tethering downloads removed |
| `26.44.4` | Left and Right keep working when the photo on show leaves the filtered list. Import Images removed: Shoebox does the importing |
| `26.44.3` | Lens correction uses the aperture and distance a photo was shot at. Less colour cast in the deepest shadows of very high ISO photos (RAWmakase). The active library filter shows on the View button, with Show all when the grid is empty. A missing LUT no longer renders a photo black |
| `26.44.2` | An eye on Crop > Lens Correction. The export size estimate back in the Modern window. Safer saves: no file cut short by a crash, no starred looks or snapshots wiped after a failed read, an XMP kept on import. Light palette notifications and Curves histogram. Lean sweep: Android, RapidRAW Cloud sign-in at start, Google Fonts, the old Presets panel and Classic layout state removed; colour maths, cache keys, file names and models defined once; stricter CI (golden photos, translations, lint, installer size) |
| `26.44.1` | Face and Text masks, from RapidRAW 1.6.7. RAW decoder choice, rawler or FlashRaw. A RAW's colour built the way the DNG specification builds it, for new edits. The camera's own lens correction centred on the lens's optical centre and framed as Lightroom frames it (RAWmakase 0.4.6). Canon EOS R8 Mark II. Undo when a photo leaves the filter. The rest of RapidRAW 1.6.7's harvest: Heal texture, Relight on rotated photos, faster batch paste and reopen. Two new CI checks: a compatibility corpus and a UI walk |
| `26.43.4` | The camera's own lens correction for Nikon Z and Coolpix, Olympus and OM System, Panasonic and Leica, Samsung, and Canon R5 Mark II / R6 Mark III. My Gear: "Correct new photos" per lens, and a profile for new photos per camera. The update notice beside the Settings gear. The golden-photos look check in CI. RapidRAW no longer merged; its releases are harvested by hand |
| `26.43.3` | History kept per photo. LUTs suggested for each photo. Linear rendering for film LUTs |
| `26.43.1` | The Modern window, now the only layout. B&W with an Auto mix. Subject, Sky and Background masks in one click and click-to-select Object, on SAM 2.1. The Looks tab. Snapshots. Color Grading as Lightroom renders it. The camera's own lens correction for DNG, Fujifilm and Sony. Relight, Fog and Auto straighten from RapidRAW 1.6.6. The engine crate, `src-tauri/engine/` |

---

## Groundwork

| | What | Why now | Effort |
|---|---|---|---|
| ✅ | Fork, build, confirm it runs | Nothing else can start | 1 day |
| ⬜ | **Serve our own dependencies** | Argentum builds and runs on infrastructure belonging to one person, and it already bit us: on 2026-09-12 a HuggingFace 429 took down CI because every build downloads the ONNX runtime from his account. **Build-time:** the ONNX runtime (HuggingFace `CyberTimon/RapidRAW-Models`) and `gphoto2` (`github.com/CyberTimon/libgphoto2-rs`, tethering builds only, still followed by branch rather than pinned). `rawler`, the RAW decoder everything rests on, is done: it builds from Argentum's own fork, `akhater/RapidRAW-DngLab`, pinned to a commit. **Runtime, for every user:** nine AI models from that same HuggingFace repo - SAM encoder and decoder, u2net, skyseg, CLIP and its tokeniser, NIND denoise, LaMa, Depth Anything, MoGe-2 for Relight - plus the preset manifest and the community sample image from `CyberTimon/RapidRAW-Presets`. If any of it is renamed, made private or deleted, Argentum stops building and masking stops working for everyone who installed it, with nothing we can do from here. This is not a criticism of him: they are his repositories to manage and he owes us nothing. **Separately, a packaging bug:** `packaging/io.github.CyberTimon.RapidRAW.yml` is his Flatpak manifest and builds *his* application from *his* git, so anyone packaging Argentum that way gets RapidRAW. Worth doing in the order the risk sits: mirror the models and the runtime somewhere we control, pin `gphoto2` to a commit as `rawler` now is, and write our own Flatpak manifest. The selection masks' models (SAM 2.1, DETR) already come from commit-pinned repositories, checked by SHA-256 | 1–2 days |
| 🟨 | **Memory usage** | **Editing done in `26.41.10`, export still to do.** Since `26.43.4` the develop hands the photo on as RGB instead of RGBA, a quarter less memory per open RAW. Measured on a Canon R6 Mark III raw (32 MP) with a `[memory]` log line that now says where the memory is: one AI erase took the process from 2.6 GB to 12 GB and it settled at 8.5 GB. 6.1 GB of that was the five AI mask models, whose ONNX Runtime sessions kept their CPU memory arena for the whole run; they are now built without it (`mods/ai_session.rs`) and unloaded when idle (`mods/memory.rs`), and the same work settles at 3-4.5 GB. Two copies of a photo nothing had changed are shared instead. Still open: export, below, and the AI eraser, which composites and converts the whole photo at full precision for every stroke to fix one spot - doing it on the stroke's region means reworking the four patch commands inherited from RapidRAW, a decision of its own. What follows is the original note. Nobody had measured the high-water mark on a large RAW, and the pipeline has been adding full-resolution copies rather than removing them: the upload, the patched/warped preview, and now an `rgba32float` export target that costs four bytes a channel where the preview costs two. Going f32 all the way in doubles the input too, and `26.38.1` added one more: keeping metadata on a TIFF parses and re-serialises the whole encoded file, 3-4x its size during the write - about 2.5GB on a 60MP 16-bit export. The work is to measure first - peak resident set across decode, edit and export on a 45MP file - and only then decide what to drop, reuse or stream, because every candidate here is a guess until something is on a graph | measure: 1 day |
| ✅ | **Auto backend that measures** - not needed, done another way in `26.43.1`. | Asked for 2026-10-10: Auto takes Vulkan, which on an Intel Arc 140T rendered a 7 MP preview in about 0.7 s against 0.16 s on OpenGL. The cause was not the driver choosing badly but the main shader passing its curves and HSL bands as arrays, which wgpu copies for every pixel. Reading them in place made Vulkan and DirectX 12 20-45x faster (16 ms, 17 ms and 24 ms on OpenGL for the same preview), so Auto is fast and there is nothing left to measure. | - |
| ⬜ | One-folder data location | Cheap now, annoying to retrofit | ½ day |
| ⬜ | Portable build (no installer) | Same reason | ½ day |

## First tools — the colour block

These three are one session's worth of thinking. They all sit in the same part of
the pipeline, so doing them together means understanding it once.

| | What | Source | Why | Effort |
|---|---|---|---|---|
| ✅ | **White balance** | darktable, then RapidRAW | Done 26.37.2, plus auto-WB. Built the sRGB↔XYZ↔Bradford conversion every later tool reuses. **Engine replaced in 26.41.6 by RapidRAW 1.6.5's own** (Kelvin from the camera's as-shot data, per-mask white balance, an area picker): it does what ours did and more, and ours turned out to decode the RAW twice and clip highlights above white whenever a slider was off zero. Auto-WB stays ours and answers in their units; old edits are converted (`mods/wb_legacy.rs`) | 3–4 days |
| ✅ | **White balance presets** | Lightroom | Done 26.41.8. As Shot, Auto, Daylight, Cloudy, Shade, Tungsten, Fluorescent, Flash and Custom in a Preset row above Temperature, replacing the auto wand (`src/argentum/WhiteBalanceMenu.tsx`). Adobe's numbers, which mean the same here because RapidRAW's kelvin and tint are the DNG SDK's | ½ day |
| ⬜ | **White balance on RAW files only** | Lightroom | Next, asked for 2026-10-09. For anything but a RAW, RapidRAW's as-shot white balance is a D65 placeholder, so a preset on a JPEG corrects colours the camera already balanced. Lightroom offers only As Shot, Auto and Custom there. `SelectedImage.isRaw` is already in the editor store | ½ day |
| ✅ | **DCP camera profiles** | RawTherapee | Done 26.37.12. Per photo in Edit > Profile, found online or imported, applied on the GPU per frame; since 26.43.4 also a profile per camera for new photos, in Settings > My Gear. It did **not** close the 4.2% gap against darktable, and could not have: darktable renders through the same Adobe matrix rawler already carries, so that measurement scores agreement with Adobe, not accuracy. The reasoning in the line above was wrong | 3–4 days |
| ✅ | **Highlight recovery** | darktable | Biggest visible rescue on real photos. RapidRAW has a Highlights *slider*, which is a different thing — it can only move detail that survived. This rebuilds a channel that clipped from the two that did not. Raw domain, before demosaic, through the decode anchor we already have | 2 days |
| ✅ | **Clipping preview** | Lightroom | Hold a key on Blacks/Whites and see which pixels are about to lose everything. Recovering highlights without it is guesswork. Shift and Alt on a slider are taken (fine adjustment), so it is Ctrl, held while dragging Whites or Blacks | 1 day |

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
| 🟨 | **16-bit TIFF export** | Shipped in `26.37.20`, not finished. The render writes into an `rgba32float` storage texture for TIFF and quantises once at the encode; the encoder was already correct - it was being handed 8-bit data. See `mods/export_precision.rs`. It stays in progress because the file holds 16 bits the pipeline cannot yet fill: **f32 all the way in**, four rows down, bounds it at about 11 significant bits, so the header is honest about the container and optimistic about the content. Since `26.38.1` the container is also a choice - see the row directly below | rest below |
| ✅ | **Export precision selector, step 1: 8-bit TIFF and the control** | Done 2026-09-14, released in `26.38.1`. Every TIFF was 16-bit whether that was wanted or not; a delivery is not a master. The control is `src/argentum/ExportPrecision.tsx`, shown only for TIFF and tagged once into `ExportPanel.tsx`, with the depth stored as Argentum's own preference rather than on their `ExportSettings` - so **zero locale files of theirs and no fourth parameter on `encode_image_to_bytes`**, which is exactly what #1466 does. 8-bit is `Precision::Preview`, the same render a JPEG gets; the container comes from **their** encoder arm, borrowed between `// upstream #1466` markers, which also fixed a pre-existing gap - theirs matches `"tif" | "tiff"` and ours matched `"tiff"` only. **Two bugs caught in review rather than after:** dispatching on image type would have quoted the 8-bit size for a 16-bit export, so both estimate sites now go through `render_for_export`; and `argentum-processing.json` was written whole on every save, so saving an export depth would have switched highlight recovery back on - reads and writes now go through `mods/ag_settings.rs`, which changes one key and leaves the rest alone. **Still not covered, as planned:** the depth is not in their export presets and there is no `--tiff-bit-depth` headless flag; `run_headless_export` uses the persisted mode | done |
| ⬜ | **Resize-aware export sharpening** | Add an optional sharpening control for exported files, applied after resizing so it restores edge contrast lost during downsampling without changing the editing preview. First compare a sharper resampling filter (such as bicubic-sharper or Lanczos) with a separate mild unsharp-mask pass; choose the least artifact-prone approach, then expose a small Off / Standard / Strong control rather than another dense adjustment panel. Avoid double-sharpening when no resize is requested. | 1 week |
| ⬜ | **Step 2: measure before promising** | The equality sweep becomes per-mode, and a new test tells an `rgba16float` render target from an `rgba32float` one: a ramp spanning one encoded stop must return more than 1024 distinct levels, which theirs cannot and ours does. That test is what stops a later change, or a hunk harvested from RapidRAW, silently swapping our target for a half-float one. Then the same sweep with sharpening at a real value, so the blur-intermediate question is settled by a number rather than an argument. Nothing user-facing changes. | ½ day |
| ⬜ | **Step 3: "16-bit, maximum" — f32 upload, previews untouched** | Design and cost in the row below. `Precision::Full` uploads `Rgba32Float`; the flare pass gets a CPU-made `Rgba16Float` copy bound only in its two bind groups (4 lines of theirs, no layout change, no shader change, no device feature); blur intermediates follow only if step 2 measured that they must. **Ships only when the sweep for `Full` asserts the file is the limit**, and needs an up-front VRAM estimate that errors before allocating — never a silent fallback to half-float. | 2 days |
| ⬜ | **f32 all the way in, not just out** | The input is still uploaded as `Rgba16Float`, so the export is bounded at about 11 significant bits by the *upload* and the f16 intermediates rather than by the output. A 14-bit sensor holds 13 bits in its top stop, so the brightest two are thinned for rendering. **Not** before highlight recovery, which an earlier version of this row claimed: `raw_processing.rs` runs `mods::decode::on_raw_decoded` - and `highlights::recover` inside it - on the Bayer data, before anything reaches a texture. Reconstruction sees full precision; only its rendering is thinned. **Constraint:** the preview path does not change - no 32-bit input, no extra bandwidth per interactive frame. Full precision is for the export plumbing only; a 32-bit input across the whole pipeline is four times the bandwidth on every frame to serve a file written once. **Confirmed architecturally free** (reviewed 2026-09-13): `render_high_precision` builds its own input texture and its own `GpuProcessor`, and never reads `state.gpu_processor` or `state.gpu_image_cache`. Every bind-group layout, pipeline, blur and flare texture, tile output, per-run mask array and LUT texture is owned by that processor. The only things an export shares with a preview are `context.device`, `context.queue`, `context.limits` and two CPU caches read before the render. Nothing GPU-side is shared, so previews stay byte-for-byte as they are. **The only blocker is the flare pass.** Of the four binds of `input_texture_view`, the main pass and both blur passes are `filterable: false` and use `textureLoad` only, so `Rgba32Float` binds there with no device feature and no shader change. The two flare passes go through `flare_bgl_0`, which is `filterable: true` with a `Filtering` sampler, and `flare.wgsl` calls `textureSampleLevel` - and that is the only thing needing `FLOAT32_FILTERABLE`, and only when `flare_amount > 0`. **Fix:** in the High processor only, upload a second `Rgba16Float` copy via the existing `to_rgba_f16` - bit-identical to what a preview samples - and bind it in the two flare bind groups. About 4 lines in their file (one `Option<TextureView>` and two `unwrap_or`), no layout change, no shader change, no new feature, no per-machine variation. Declaring the High flare layout non-filterable instead was rejected: it makes an export's bloom nearest-sampled where the preview's is bilinear. **Intermediates:** the four blur textures are read unconditionally but contribute only through sharpness/tonal/clarity/structure, which is why the measurement below landed exactly on the upload's grid at neutral settings. With those tools on, their f16 error rides on the blurred term only. To close that too: the reusable texture descriptor and the blur storage format become precision-dependent (2 lines) and `blur.wgsl` gets the same textual format rewrite `export_shader_source` already does. Flare's three 512x512 textures and the LUT stay f16 - an additive glow and a LUT's own resolution both dwarf 2^-11. **Expected result:** the decoded RAW is already 32-bit float (`ImageRgb32F` since `26.43.4`) and nothing narrows it before upload, and the shader is f32 throughout, so at neutral settings the ceiling becomes the `u16` file itself. With blur-based tools on and the blur textures still f16, 11 bits on the blurred component - measure it rather than assume. **Cost at 100 MP:** f32 input 1.63 GB (was 0.81), plus a 0.81 GB f16 flare copy only when flare is on, plus blur/ping-pong at f32 425 MB (was 212), plus the existing 85 MB tile - 2.1-2.9 GB for the export processor. Use `as_rgba32f()` rather than `to_rgba32f()` or peak RAM grows 1.6 GB. Exports get slower and heavier; previews do not. **What a test will not catch:** VRAM exhaustion on a 4 GB card with a 100 MP file, which is device loss for the whole session - the honest guard is an up-front size estimate that errors before allocating, never a silent fallback to f16; driver bugs in `rgba32float` storage writes on hardware the test machine is not; and export-vs-preview divergence, which nothing tests today | 2 days |
| ✅ | **EXIF in an exported TIFF** | Done 2026-09-14, released in `26.38.1`. The **Keep metadata** checkbox was on by default and silently did nothing for TIFF, and was not even shown for it. The FIXME was not stale in the way it looked: `little_exif` 0.6.23 does list TIFF, but it treats a TIFF as one big EXIF structure, so a write replaces the whole file with an encoding of the `Metadata` - and upstream's fresh `Metadata::new()`, correct for JPEG's spliced segment, holds no strips and none of the ten structural tags a TIFF must carry, so the library refuses it. `mods/export_metadata.rs` seeds from the encoded TIFF instead and merges upstream's tags in, reusing their gathering through a one-pixel JPEG carrier so `exif_processing.rs` is untouched. Ten tests, including the multi-strip path every real export takes and a source that tries to stamp its own dimensions on ours. **Known cost:** the whole file is parsed and re-serialised, so peak memory is 3-4x the encoded size during the write - about 2.5GB on a 60MP 16-bit export. Writing the Exif IFD directly through the `tiff` crate would avoid that and belongs with the memory row above | done |
| ✅ | **A watermark blends in 8 bits** | Fixed 2026-09-13, same day it was written down, because two reviews independently said it belonged in the feature rather than after it - and both were right: `imageops::overlay` rewrote every pixel in the stamp's *bounding box* through `Rgba<u8>`, transparent ones included, so a watermark at zero opacity quantised a rectangle of the photograph and put nothing there. `overlay_preserving_precision` uses upstream's own blend arithmetic at f32 and delegates to theirs unchanged for 8-bit images | done |

## Library

| | What | Why | Effort |
|---|---|---|---|
| ⬜ | **Group by date, camera or lens** | The grid is one flat list, or grouped by folder in the Subfolders view. `useSortedLibrary.ts` does group, but only RAW/JPEG pairs by `group_id` through `utils/imageGrouping.ts` - upstream's pair grouping, which folds two files of the same frame into one card and has nothing to say about days, cameras or lenses. Needs no index, so it can be done before the catalogue. **Upstream #1577 "film rolls" is not this either** - hand-made date ranges | 2 days |
| 🟨 | **Organize the LUT library** | **Partly done:** LUTs sit in named folders (`26.39.1`), the Looks tab (`26.43.1`) has All and Favourites chips, Move to and rename, and since `26.43.3` moves and deletes several at once. **Still to do:** a Recent view, keeping a folder's hierarchy on import, and checksums for duplicates. The original plan follows. Custom LUTs currently collapse into one flat grid. Add logical collections/packs plus virtual **All**, **Favorites**, **Recent** and **Uncategorized** views, backed by a small versioned `luts/library.json` manifest. Keep imported LUT files at stable paths - do not physically move them, because existing `.agdata` files store absolute `lutPath` pointers. Importing a folder should suggest a collection named after that folder and preserve its subfolder hierarchy; loose files should start in Uncategorized. Use `.cube` titles, source-folder names and filename tokens only for editable suggestions, never to silently guess categories or scene/display processing mode. Migrate existing flat imports into Uncategorized, use checksums for duplicates, and add stable LUT IDs only for new sidecars with legacy `lutPath` fallback | 2–3 days |
| ✅ | **Sort by capture time, not file time** | Arrived from upstream (`0e8029bd`), not built here: the `date_taken` sort key reads `DateTimeOriginal` and falls back to the file's modified time only when a photo has no capture date. It is why the row says *arrived* rather than *built* | done upstream |

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
| ✅ | **Persist the history stack to the sidecar** - done in `26.43.3` | 2 days |

RapidRAW's 50-step History lived in the frontend store only and was lost when the
photo closed. Since `26.43.3` it is kept in the photo's sidecar as `agHistory`, at
the step it was left on, so undo and redo carry on after opening another photo or
restarting (`mods/history.rs`, `src/argentum/history/`). Each step keeps its name
and only what it changed, so a 50-step edit adds about 11 kB. A History is used
only with the edit it was saved beside: after a paste, a reset or a change made
elsewhere, the photo starts a fresh one.

## Look and feel

| | What | Effort |
|---|---|---|
| ✅ | **DNG Standard** - done in `26.43.1`. The default picture of a DNG renderer - each camera's exposure offset (RAWmakase's table, 280 bodies) and Adobe's default tone curve, applied so hues hold - with Lightroom's default sharpening. Every RAW now starts in it; RapidRAW's rendering stays under Edit > Profile > Rendering > Default, beside Linear (`26.43.3`), the base film-simulation LUTs are made for. | done |
| ⬜ | Filmic / Sigmoid tone curve. AgX already ships as a tone mapper in Edit > Profile; this would be darktable's as a second choice | 2 days |
| ⬜ | Color balance rgb | 2 days |
| ⬜ | Color equalizer | 2 days |

## Masking

| | What | Why | Effort |
|---|---|---|---|
| ✅ | **Linear and radial masks that make sense** | Done 26.41.9, asked for 2026-10-09: the edges showed as lines, and which side got the effect was unclear. The linear mask is two lines, full and none, marked 100% and 0%, drawn as a graduated filter, from where the effect is full to where it has gone (`src/argentum/LinearMask.tsx`). The radial mask gets an inner ellipse that follows Feather (`RadialFeather.tsx`). Both fade with smoothstep instead of a clamped ramp, so neither edge shows as a line (`mods/mask_falloff.rs`). darktable's erf gradient was tried and dropped: it runs past its lines | 1 day |
| ❌ | **Paint to select an object** | **Removed 2026-10-07, in `26.41.5`.** On the first real test a stroke along an eye took the lids and brow, where RapidRAW's Subject box round the other eye took that eye alone: a box tells SAM how big the thing is, paint only roughly where it is, and five guessed prompts did not close that gap. Its ~100 MB edge model is deleted from the models folder at the next start. The code is in git at `c15e53f0`. What it was: done 2026-10-06, released in `26.41.4`. Lightroom's Select Object as a brush: the Object tile in the masks toolbox, after Subject. Strokes become SAM points and a box around the paint; the boxed answer stands unless its box plainly cut the object off. Small things get a second SAM pass on a crop, and every edge is redrawn by ViTMatte (about 100 MB, downloaded on first use). CPU only: 1-3 s a stroke, 2-7 s for small things - running these models on the graphics card would be the next speed-up. Zero lines in their files - the tile is added to `MASK_AI_TYPES` at runtime and the result is an ordinary `ai-subject` mask carrying `objectStrokes` | removed |
| ⬜ | **Name the objects, then mask one** | Masking is already a click since `26.43.1`: Subject, Sky and Background find themselves, and Object picks what is clicked, with more clicks to add or leave out parts, on SAM 2.1 and DETR panoptic (RAWmakase's selection crate, `src-tauri/rm_selection`). What is missing is the list: DETR already labels what it finds in COCO's classes, so the frame's contents could be offered as names to tick - the dog, the bicycle - rather than places to click. Tags fall out of it for free and make library search work on content. A grounding model (Grounding DINO / YOLO-World) would be needed only for things outside those classes | ~1 week |
| ⬜ | **Stop the sidecar re-sending every mask on every keystroke** | Measured on one real photo: an AI mask is a full-resolution 8-bit PNG — 5616x3744, 254KB, 340KB once base64'd into the JSON. Linear and radial masks cost nothing, because they are stored as geometry. So the whole cost is AI and brush masks at ~330KB each, and ten of them on one photo is a 3.4MB sidecar. Nothing crashes at that size; what hurts is that `debouncedSave` re-serialises the entire adjustments object, bitmaps included, 300ms after *any* change and ships it over IPC — so nudging a slider moves megabytes, and OneDrive re-uploads the lot. Four options, in the order they are worth doing: **(1) store masks at reduced resolution** — half res is a quarter of the bytes and a feathered selection is resampled on use anyway, taking ten masks from 3.4MB to 650KB, and this is small and self-contained; **(2) send only masks that changed** and merge the rest in Rust, which is the real fix and takes a slider nudge from 3.4MB to 3KB — but it touches `useImageProcessing.ts` and `file_management.rs`; **(3) split bitmaps into files beside the `.agdata`**, which also removes the 33% base64 tax and stops OneDrive re-uploading untouched masks, at the cost of orphan cleanup and keeping them travelling with the photo; **(4) a database, which does not solve it** — the cost is per-keystroke serialisation rather than storage, and it would break edits living next to the pictures. Ten AI masks on one photo is expected, not hypothetical, and since `26.43.3` the sidecar also holds the photo's History and Snapshots, whose steps can carry masks of their own. **Migration, which is why the order is what it is:** (1) needs none, because a mask is resampled when it is applied, so old full-resolution ones keep working beside new small ones; (2) needs none either, being a transport change with the file format untouched; (3) migrates itself one photo at a time - read `maskDataBase64` when it is there, write the PNG beside the sidecar on the next save and drop the field, no batch job and no flag day; (4) is the only one needing a real migration, with no way back, which is a further reason against it. **Measured on real masks, 2026-09-12, rather than assumed.** The worry was that a lower-resolution mask would soften detail on a 21MP photo, and it does not, because there is no detail in the mask to soften: the steepest change between adjacent pixels is 11 levels of 255, so the edge ramps over roughly 23 pixels, and 95% of the mask is flat 0 or 255. Downsampled and brought back up: half resolution is off by at most **4 levels of 255** with one pixel in 100,000 differing by more than two; quarter is at most 8; an eighth at most 18. Sizes re-encoded from the same mask - full 121KB, half 53KB, quarter 18KB, eighth 6KB. **A second saving found while measuring, free of everything:** that mask is stored at 254KB but re-encodes to 121KB with no change to a single pixel, so whatever writes it is not optimising the PNG. Halving it needs no resolution change, no schema change and no migration. Half resolution plus a proper encoder is 254KB to 53KB, about 4.8x, and takes ten masks from 3.4MB to 700KB. **Caveat, untested:** this was measured on AI masks only. A brush at zero feather may genuinely have hard edges, so key the resolution on mask type rather than applying it to everything | 1–3 days depending on option |
| ⬜ | **Never let a failed load save over a good sidecar** | Autosave's only guard is `prev.adjustments !== adjustments` in `useImageProcessing.ts` — a reference comparison. It answers "is this a different object than last time", not "did this photo's real data finish loading". Masks live inside `adjustments`, so if the editor state is ever reset to defaults while a photo is selected — after a crash in the mask overlay, say — that counts as a change and is written straight to the sidecar, and a 768KB file with three masks becomes a 6KB file with none. Not proven to be what emptied one real photo's sidecar, but it is a mechanism that exists in the code today and the shape matches exactly. A save should be refused unless the adjustments being written came from a completed load of that same path. **Partly covered since `26.43.4`:** a sidecar or settings file that does not read is kept as `.bak` and salvaged rather than replaced with blanks; a reset editor state written over a good file is still possible | 1 day |


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
| ✅ | **Sharpening** - done in `26.41.11`, replacing RapidRAW's. RawTherapee's capture sharpening (Richardson-Lucy, auto radius from the RAW), darktable's sharpen, RawTherapee's contrast mask with an eye that shows it, and a Sharpen per mask, on the GPU through `mods/input_stage.rs`. The next whole-image tool here (denoise, diffuse) goes into the engine, `src-tauri/engine/`, as one file and one registry line. Output sharpening after a resize is its own row, under Export | 2 days |
| ✅ | **Restore** - done in `26.41.12`. OmniLens (SwinIR on a simulated lens library) undoes lens softness at the photo's own size, through a detail mask, before capture sharpening; in Restore & Enlarge, opened from Edit > Details > Enhance. Enlarging a RAW capture-sharpens it first. Next lever if the gain is too small: OmniLens's domain adaptation to one lens from 25-50 of its photos | 2 days |
| ⬜ | Diffuse effect for the editor | ~1 week |
| ⬜ | Profiled denoise | ~1 week |
| ⬜ | Local laplacian contrast | ~1 week |


## RapidRAW pull requests to harvest

RapidRAW's open pull requests, triaged against what Argentum already has. Each
claim below was checked against this repository on 2026-09-12 rather than taken
from the pull request's description - three of them did not survive that check,
which is why the order differs from the obvious one. Rows marked done were
re-checked on 2026-10-26. Argentum no longer merges RapidRAW (since 2026-10-25),
so anything here arrives only if it is harvested by hand.

| | Take | Verified here | Effort |
|---|---|---|---|
| ✅ | **#1307, the AI patch cache key** - done in `26.37.20`, marked `// upstream #1307` | `cache_utils.rs` hashes `.len()` of `color`, `mask` and `patchDataBase64` rather than their contents, so two patches of equal length collide and the wrong one is rendered from cache. Visibly wrong output, which is why it goes first | 1 hour |
| ⬜ | **#1307, the unbounded LUT cache** | `app_state.rs` holds `Mutex<HashMap<String, Arc<Lut>>>` with no eviction | 1 hour |
| ⬜ | **#1307, the unchecked mask index** | `export_processing.rs` does `all.mask_adjustments[mask_index]` with no bounds check - a panic, not an error | 1 hour |
| ⬜ | **#1619, `Arc<DynamicImage>` for the geometry cache** | `app_state.rs` stores a bare `HashMap<u64, DynamicImage>` and clones the whole pixel buffer on every hit, while the two caches either side of it already use `Arc` | 2 hours |
| ✅ | **#1633, the sRGB exponent** - done in `26.37.20`, marked `// upstream #1633` | Real: `raw_processing.rs` uses `powf(3.0)` where every other site in the tree uses `2.4`. Worth taking because it is one line and standards-correct - **not** because it matters here: the function is file-private and only reached under `is_linear_format && apply_ungamma`, which a Canon CR2 never is | 5 minutes |
| ⬜ | **#1705 second commit, Lensfun evaluation** | `image_processing.rs` multiplies `lens_distortion_amount` by an arbitrary `2.5` in four places. The PR also corrects radius normalisation, coefficient rescaling and crop-factor matching. Take the maths, leave the bundled #1687 embedded-profile work. Put as much of it as possible in `mods/` and leave one call behind | 1 day |
| ⬜ | **#1608, lens geometry after EXIF orientation** | Argentum warps and lens-blurs first and applies `orientationSteps` afterwards, which is the wrong order for portrait-oriented files. Argentum no longer merges RapidRAW, so it arrives only if it is harvested by hand; until then portrait files get the wrong order | 1 day |
| ✅ | **#1466, 16-bit TIFF** | Done 2026-09-13, and the triage above was half wrong about it. #1466's *idea* is what was worth taking - build the export pipeline by rewriting the storage format in the shader source, gate the dither behind a pipeline constant - and that is marked under `// upstream #1466`. The idea, not the text: their override is a `u32` tested with `== 0u`, ours is a `bool` and a negation. Its encoder was not needed *for this*: `DynamicImage::to_rgb16` already quantises f32 correctly and was only ever being handed 8-bit data - though its encoder arm **was** borrowed a day later for the 8/16 selector above, where dispatching on the image type is the whole mechanism. Its `rgba16float` target was not taken either; ours is `rgba32float`. **Nothing of #1395 is used**: its bounded intermediate textures are already here as `clamped_tile_size`, and its capability gate belongs to the half-float design this did not follow - not re-read against a 32-bit target, so look at it rather than dismiss it if the f32 input above is ever attempted. Estimated at a week. Three of their files take part - two at one import each, the shader at a marked block - but that is the anchor count, not the maintenance cost: a constructor parameter, four descriptors reading their format from it, the readback strides, a marked block in the shader and four export call sites | done |
| ⬜ | **#1569, missing sidecars** | Useful data-integrity logic, manual port only - it assumes RapidRAW's sidecar naming and we use `.agdata`. Raised in priority by what was found on 2026-09-12: autosave can write back state it never finished loading, which is the same family of fault. Its other half, filename collisions on copy and move, was fixed in `26.43.4` with RapidRAW 1.6.7's file fixes (`mods/file_ops.rs`) | 1 day |
| ✅ | #1626 export XMP keywords, #1246 EXIF timezone offset - done in `26.43.1` | Exports carry the rating, colour label and keywords as XMP in JPEG, PNG and WebP, and the capture time zone for RAW sources | ½ day each |

**Already done here, and done better - do not take these.** ~~#1676 colour-managed
white balance~~ - **superseded 2026-10-07:** RapidRAW 1.6.5 shipped its own white
balance, dual-illuminant interpolation included, and on review it was the better
engine (ours decoded the RAW twice before adapting). Taken in 26.41.6; auto-WB stays ours. #1219 highlight clipping
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

## Known gaps from the merging years

Left open when RapidRAW was still merged, because the fix needed a line in a file
whose line count could only go down. Those files are ours to change now.

| | What | Notes |
|---|---|---|
| ⬜ | **Kelvin readout on Canon 1D/1Ds** | Their `read_as_shot_white_balance` decodes on its own and never reaches `mods::decode`, so the Kelvin readout is off on those bodies (the picture and the corrections are right). One line in `raw_processing.rs`; listed in `knownIssues.ts`. |
| ⬜ | **Apple RAW 9 (macOS, off by default)** | Never reaches our decode step: no sRAW levels, highlight recovery or camera profile on that path. |
| ⬜ | **Super-resolution and white balance** | An absolute `whiteBalance` is carried onto the enlarged non-RAW file, where the as-shot is D65; relative edits carry correctly. |
| ⬜ | **TIFF ICC profile (RapidRAW #1752)** | Lands in the encoder arm Argentum replaced: port it to `mods/export_precision.rs` when harvested. |

## Already covered — nothing to build

**Lens correction is not missing.** RapidRAW ships a complete one: auto-detect
from EXIF, manual mode and distortion / vignetting / CA correction, with the full
lensfun database bundled. Argentum adds the camera's own correction read from the
file, for the makers that store one (the row below), and *Settings > My Gear > My
Lenses*, which fills itself as photos are opened and can correct every new photo
taken with a lens.

---

## For other cameras and other computers

Argentum has been public since `26.37.19`. These matter most to people whose
cameras or systems are not the ones it was first built on.

| | What | Who needs it | Effort |
|---|---|---|---|
| ✅ | **Lens correction from embedded RAW metadata** - done in `26.43.1` and `26.43.4`. Read from the file and applied the way Lightroom applies it: DNG, Fujifilm, Sony, Nikon Z and Coolpix P950/P1000/P1100, Olympus and OM System, Panasonic and Leica, Samsung, Canon EOS R5 Mark II and R6 Mark III. The readers are RAWmakase's (`src-tauri/engine/src/rm_lens_builtin/`) | Modern mirrorless shooters. That glass is often designed assuming software correction and is frequently absent from lensfun | 3–4 days |
| ⬜ | **Demosaic (RCD / Markesteijn)** | Fuji X-Trans shooters, for whom standard demosaic is genuinely poor | ~1 week |
| 🟨 | Cross-platform build and test (Linux, macOS) | Everyone not on Windows. macOS runs on Apple Silicon; Linux builds in CI and has never been run. The one-folder data layout is Windows-shaped | unknown |
| ⬜ | Sensible defaults for the data folder | Current setup hardcodes local paths | small |

**Don't build these speculatively.** Each earns its time when someone with that
camera or system needs it.

---

## Deferred — wanted, but not now

| | What | Note |
|---|---|---|
| ⬜ | **Offline editing** (Lightroom-style smart previews) | Keep a medium-res copy so photos on an absent drive stay editable, applying the changes when the drive comes back. ~1 week on top of the catalog. Revisit only if the offline indicator and remap turn out not to be enough in practice |

---


### Export precision: what each option may claim

Each string is allowed only what its test measures.

| Option | Control label | Line under it | Release-note sentence |
|---|---|---|---|
| `Preview` | **8-bit** | Smaller file. The same precision as a JPEG — fine for delivery, not for re-editing. | "A TIFF can now be exported at 8 bits, for a smaller file when it is a delivery and not a master." |
| `High` | **16-bit** | For re-editing elsewhere. 1024 tonal levels in every stop of exposure. | "16-bit TIFFs carry 1024 tonal levels in every stop — the precision of the screen render, measured." |
| `Full` | **16-bit, maximum** | Every level the file can hold. Slower, and uses about twice the graphics memory while exporting. | Not written until the sweep for this mode passes. It will say what that sweep measured and nothing else. |

**Never** `f16`, `f32`, `half-float`, or a bit count other than the container's, in
any label. **Never** "true 16-bit" until step 3 has its measurement, and not in a
title even then.

### Theirs against ours, part by part

| Piece | #1466 | Ours (shipped) | Keep |
|---|---|---|---|
| Render target | `rgba16float` — the finished value is re-quantised to half-float *before* the file, a 32-code grid in the top encoded stop | `rgba32float` — no output quantisation | **ours** |
| Pipeline lifetime | second pipeline compiled inside every `GpuProcessor::new`, i.e. on the preview path | compiled for an export only, dropped after | **ours** |
| Shader source | `include_str!("shader.wgsl")` alone — drops `modules.wgsl`, every `ag_` call undefined here | composed `SHADER_SOURCE` | **ours** |
| Readback | f16→u16 per sample in Rust; non-finite becomes 0, i.e. black | f32 bytes → `Rgba32F` → `to_rgb16`; non-finite becomes white, the `image` crate's convention | **ours** |
| Watermark | full-image typed copy, then `overlay` | in place on `Rgba32F`, transparent pixels skipped, delegates to theirs for 8-bit | **ours** |
| Shader gate | `override … : u32`, `== 0u` | `bool`, negation | either |
| **Encoder** | `"tif" \| "tiff"`, 8-bit arm writes `Rgb8` | `"tiff"` only, always `Rgb16` | **theirs** |
| **Selector** | 8/16 dropdown in their panel, 13 locale files | none | ours, in `src/argentum/` |
| Headless | `--tiff-bit-depth` | none | later, optional |

## Open questions

### A toggle in the export panel does not stay where you put it

**Noticed 2026-09-13, inherited, undecided.**

Turn "image resizing" off, restart, and it is on again. Not a regression and not
ours: `useExportSettings` starts every switch at its default, and on opening the
panel `ExportPanel.tsx` applies a hidden `__last_used__` preset. That preset is
written by `saveLastUsedPreset`, which is called from `handleExport` **and
nowhere else** - so the panel restores the settings you last *exported* with, and
a switch you changed but never exported with is forgotten. Verified untouched by
us: zero commits and an empty diff against `5ad3ba0b` for that file.

Defensible as "remember my last export". It also reads as a broken switch, which
is how it was found.

If it is changed, it is a line or two, and the
question to answer first is which behaviour is wanted: persist on toggle, or keep
persisting on export and make the panel say that is what it does. **Decide before
building.** Left alone for now.


- **`.agdata` next to photos, or in the data folder?** Next to photos = edits
  travel with the pictures. In the folder = nothing left behind if you bin the project.
- **Does white balance decode RAW that was never encoded?** Found 2026-09-13
  while scoping the high-precision export, and *not confirmed* - written down so
  it is checked rather than forgotten.

  **Answered 2026-10-07: yes, it did.** The preview input is `loaded_image.image`
  through patches, warp and lens blur, with no encode on the way; RapidRAW 1.6.5's
  picker reads it as linear for the same reason. `dt_white_balance` therefore
  decoded linear data a second time, applied about three quarters of its
  correction in the midtones, and clipped everything above white while a slider
  was off zero. Retired with the 1.6.5 merge.

- **History depth in the sidecar** — answered in `26.43.3`: each step stores only
  what it changed, about 11 kB for a 50-step edit.
