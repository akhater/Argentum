# Argentum

RapidRAW's interface and AI, darktable's image quality — in one app.

A fork of [RapidRAW](https://github.com/CyberTimon/RapidRAW) (Rust / Tauri / WGPU),
with image-processing modules harvested from
[darktable](https://github.com/darktable-org/darktable) one at a time —
and later RawTherapee, GIMP, or anywhere else worth raiding.

**Status:** research complete, go decision made, nothing built yet.

---

## Where things live

| | Path | What |
|---|---|---|
| **Work** | `C:\Users\you\NoCloudZone\Argentum` (here) | Everything — source, `node_modules`, build output, app data |
| **Backup** | `…\OneDrive\…\Personal\Argentum.git` | Bare git repo. Source history only, ~7MB, never a binary |

**Why the working tree isn't in OneDrive.** It was, briefly. OneDrive doesn't
tolerate directory junctions inside a synced folder — it silently replaced the
`node_modules` junction with a real folder and started syncing 182 packages. No
link-based trick survives that.

So the split moved up a level: work happens entirely outside OneDrive, and
OneDrive holds the *git history* instead of the files. It still has every version
of every source file, in a fraction of the space, and it can never pick up a
binary because git never tracks one.

Back up after committing:

```bash
git push
```

---

## Docs

| File | What's in it |
|---|---|
| [CLAUDE.md](CLAUDE.md) | Working rules — read first |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Code layout, and why upstream updates won't hurt |
| [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md) | The recipe. Same four steps every time |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What order, and what's done |
| [docs/MENU.md](docs/MENU.md) | Everything available to harvest, from each source |
| [docs/FEASIBILITY.md](docs/FEASIBILITY.md) | The original go/no-go research |
| [CHANGELOG.md](CHANGELOG.md) | Every module, with its upstream source and commit |

Full project brain — decisions, scope, research — lives in OpenViking at
`viking://resources/projects/Argentum/`.

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

Then wire up the redirects and check everything's present:

```bash
powershell -ExecutionPolicy Bypass -File setup.ps1
```

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

One folder, chosen in Settings, default `C:\Users\you\NoCloudZone\Argentum\data`:

```
data\
  catalog.db          index of every photo seen — lets you browse offline
  thumbnails\         cached previews
  ai-models\          AI masking models
  settings.json
  presets\
```

Deleting that folder removes the app's state completely. No registry, no AppData.

The only thing outside it: `.rrdata` edit files sit next to your photos, so
edits travel with the pictures.
