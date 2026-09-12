# Roadmap

Order is chosen so the app is useful early and each step makes the next easier.
**Nothing here is a commitment — stop at any line and what you have still works.**

Status: ⬜ not started · 🟨 in progress · ✅ done

---

## Groundwork

| | What | Why now | Effort |
|---|---|---|---|
| ✅ | Fork, build, confirm it runs | Nothing else can start | 1 day |
| ⬜ | One-folder data location | Cheap now, annoying to retrofit | ½ day |
| ⬜ | Portable build (no installer) | Same reason | ½ day |

## First tools — the colour block

These three are one session's worth of thinking. They all sit in the same part of
the pipeline, so doing them together means understanding it once.

| | What | Source | Why | Effort |
|---|---|---|---|---|
| ✅ | **White balance** | darktable | Done 2026.37.2, plus auto-WB. Built the sRGB↔XYZ↔Bradford conversion every later tool reuses | 3–4 days |
| ✅ | **DCP camera profiles** | RawTherapee | Done 2026.37.12. Per photo, found online or imported, applied on the GPU per frame. It did **not** close the 4.2% gap against darktable, and could not have: darktable renders through the same Adobe matrix rawler already carries, so that measurement scores agreement with Adobe, not accuracy. The reasoning in the line above was wrong | 3–4 days |
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
| ⬜ | **16-bit TIFF export** | Export writes 8 bits a channel today — `image::ImageFormat::Tiff` on an 8-bit buffer. That discards most of what a RAW holds, and banding shows in skies as soon as the file is edited again elsewhere. Confirmed missing 2026-09-10 | 1 day |

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

## Detail

| | What | Effort |
|---|---|---|
| ⬜ | Diffuse or sharpen | ~1 week |
| ⬜ | Profiled denoise | ~1 week |
| ⬜ | Local laplacian contrast | ~1 week |

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
- **History depth in the sidecar** — full snapshots bloat the file. Cap it,
  store deltas, or both. Decide when building it.
