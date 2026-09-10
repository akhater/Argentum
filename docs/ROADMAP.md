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
| ⬜ | **DCP camera profiles** | RawTherapee | Per-camera colour calibration — makes the 5D Mark II render *as itself*, not generically. Neither RapidRAW nor darktable has this | 3–4 days |
| ⬜ | **Highlight recovery** | darktable | Biggest visible rescue on real photos | 2 days |

**Order matters here:** white balance first (it builds the colour conversion),
then DCP (same pipeline stage, and it changes what "correct" white balance even
looks like), then highlight recovery.

**After these three, it's an editor worth using.** Reasonable place to stop and
just take pictures for a while.

On DCP specifically — RawTherapee ships hand-made DCP profiles that auto-match a
camera on open, so this is partly about *using* existing profiles rather than
building anything from scratch. Found by comparing against MeraRAW; see DEC-25
and DEC-26 in the brain.

## Export

| | What | Why | Effort |
|---|---|---|---|
| ⬜ | **16-bit TIFF export** | Export writes 8 bits a channel today — `image::ImageFormat::Tiff` on an 8-bit buffer. That discards most of what a RAW holds, and banding shows in skies as soon as the file is edited again elsewhere. Confirmed missing 2026-09-10 | 1 day |

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
