# Argentum

RapidRAW's interface and AI, darktable's image quality — in one app.

A fork of [RapidRAW](https://github.com/CyberTimon/RapidRAW) (Rust / Tauri / WGPU),
with image-processing modules harvested from
[darktable](https://github.com/darktable-org/darktable) one at a time —
and later RawTherapee, GIMP, or anywhere else worth raiding.

**Status:** builds and runs. First harvested module (white balance, plus auto-WB) shipped in `2026.37.2`.

---

## Known limitations

**The screen conversion is Windows-only, and needs a matrix profile.** Argentum
reads the ICC profile Windows holds for the monitor the window is on and converts
the preview for it, so what you see matches what you export. On macOS and Linux
nothing is read and nothing is converted, and the same is true of a monitor
profile built as a lookup table rather than from three primaries. In those cases
the picture is shown the way RapidRAW always showed it — correct on an sRGB
screen, over-saturated on a wide-gamut one.

**macOS and Linux builds are untested.** The code compiles for them and CI
builds them, but no one has run Argentum on either yet. Reports welcome.

---

## Docs

| File | What's in it |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Code layout, and why upstream updates won't hurt |
| [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md) | The recipe. Same four steps every time |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What order, and what's done |
| [docs/MENU.md](docs/MENU.md) | Everything available to harvest, from each source |
| [docs/FEASIBILITY.md](docs/FEASIBILITY.md) | The original go/no-go research |
| [CHANGELOG.md](CHANGELOG.md) | Every module, with its upstream source and commit |

---

## Setup

Install the toolchain once, then restart the terminal:

```bash
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

```bash
winget install Rustlang.Rustup
```

```bash
winget install OpenJS.NodeJS.LTS
```

Then install the front-end dependencies:

```bash
npm install
```

---

## Running it

Development — opens a window, reloads as code changes:

```bash
npm run tauri dev
```

Build a portable `.exe`:

```bash
npm run tauri build
```

---

## Pulling in RapidRAW's updates

```bash
git fetch upstream && git merge upstream/main
```

Should be clean most times. If it isn't, see
[ARCHITECTURE.md](docs/ARCHITECTURE.md#staying-mergeable) — the answer is
almost always "keep both sides".

---

## The app's own data

One folder, chosen in Settings, defaulting to `data/` beside the checkout:

```
data\
  catalog.db          index of every photo seen — lets you browse offline
  thumbnails\         cached previews
  ai-models\          AI masking models
  settings.json
  presets\
```

Deleting that folder removes the app's state completely. No registry, no AppData.

The only thing outside it: `.agdata` edit files sit next to your photos, so
edits travel with the pictures.
