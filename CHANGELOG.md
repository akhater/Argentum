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
